//! Voice Isolation: keeps dialogue and removes everything else (steady noise, room tone, hum,
//! rumble, crowd and — partly — music beds), in one effect.
//!
//! A multichannel streaming STFT (√Hann, 75 % overlap). Every frame one mask is estimated from
//! the channel sum and applied to every channel, so the stereo image is preserved. The mask
//! combines four cues:
//!
//! * **Stationary noise** — the noise PSD of every bin is tracked with minimum statistics (no
//!   "learn" step); a decision-directed a-priori SNR drives an MMSE log-spectral-amplitude gain,
//!   which keeps speech at moderate SNR and leaves smooth residual noise instead of musical noise. Digital silence (clip edges, pre-roll)
//!   is not taken as the noise floor.
//! * **Voicing** — the normalised autocorrelation of the band-limited (60 Hz–1.5 kHz) spectrum
//!   gives a voicing strength and pitch; in voiced frames a soft harmonic comb attenuates the
//!   energy between the voice's harmonics (below 4 kHz).
//! * **Voice activity** — voicing and speech-band SNR give a per-frame speech-presence
//!   probability with look-ahead (the gate opens [`LOOKAHEAD`] hops before an onset, so
//!   consonants are not clipped), a 60 ms hold for word endings and a 30 ms release. Between
//!   words everything is pulled down to the floor.
//! * **Centre focus** (stereo only) — dialogue is normally centred; bins whose left/right
//!   difference dominates their sum are attenuated.
//!
//! `amount` scales the reduction in decibels (`G = g^amount`), `floor` caps it, `listen`
//! switches the output to the removed background (`1 − G`, so voice + background = the delayed
//! input). Latency is `n + LOOKAHEAD · hop` samples.

use crate::fft::Fft;
use crate::{AudioEffect, ParamSpec, ParamValues, Smoothed, Unit, block_len, db_to_gain, param_plumbing};
use std::f64::consts::PI;

/// Look-ahead of the voice-activity gate, in hops.
const LOOKAHEAD: usize = 2;
/// Spectral history slots (the frame being synthesised plus the look-ahead frames).
const SLOTS: usize = LOOKAHEAD + 1;
/// Minimum-statistics sub-windows / window length / bias (as in `DeNoise`).
const MS_SUBWINDOWS: usize = 8;
const MS_WINDOW_S: f32 = 1.5;
const MS_BIAS: f32 = 2.5;
/// Decision-directed smoothing of the a-priori SNR.
const DD_ALPHA: f32 = 0.98;
/// Lowest a-priori SNR (−25 dB): limits how far the Wiener stage alone cuts.
const XI_MIN: f32 = 0.003_162_3;
/// Pitch search range (Hz).
const F0_MIN: f32 = 70.0;
const F0_MAX: f32 = 400.0;
/// Harmonic comb upper limit (Hz).
const COMB_MAX_HZ: f32 = 4000.0;
/// Formant band protected by `preserve_formants` (Hz) and its minimum gain while speaking.
const FORMANT_LO_HZ: f32 = 200.0;
const FORMANT_HI_HZ: f32 = 4000.0;
const FORMANT_PROTECT_DB: f32 = -14.0;
/// Frames whose mean power per bin is below this are digital silence (not noise).
const SILENCE_POWER: f32 = 1e-14;
/// Internal sample-rate domain for design math; hostile rates are mapped into it.
const SR_MIN: f32 = 4000.0;
const SR_MAX: f32 = 768_000.0;

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Exponential integral E₁(x), x > 0 (Abramowitz & Stegun 5.1.53 / 5.1.56; |error| < 5e-5).
fn expint_e1(x: f64) -> f64 {
    let x = x.max(1e-7);
    if x <= 1.0 {
        -x.ln() - 0.577_215_66 + x * (0.999_991_93 + x * (-0.249_910_55 + x * (0.055_199_68 + x * (-0.009_760_04 + x * 0.001_078_57))))
    } else {
        let r = (x * x + 2.334_733 * x + 0.250_621) / (x * x + 3.330_657 * x + 1.681_534);
        (-x).exp() / x * r
    }
}

/// MMSE log-spectral-amplitude gain (Ephraim & Malah 1985) for a-priori SNR `xi` and
/// a-posteriori SNR `gamma`, at most 1.
fn lsa_gain(xi: f32, gamma: f32) -> f32 {
    let w = xi / (1.0 + xi);
    let v = f64::from(w * gamma).clamp(1e-7, 80.0);
    (f64::from(w) * (0.5 * expint_e1(v)).min(10.0).exp()).clamp(0.0, 1.0) as f32
}

/// Per-hop one-pole coefficient (fraction moved towards the target) for a time constant.
fn hop_coef(tau_s: f32, hop_s: f32) -> f32 {
    if tau_s <= 0.0 { 1.0 } else { 1.0 - (-hop_s / tau_s).exp() }
}

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-25 { 0.0 } else { x }
}

/// FFT size for speech: the power of two nearest above ~40 ms, 256…8192.
fn frame_size(sr: f32) -> usize {
    ((sr * 0.04) as usize).clamp(256, 8192).next_power_of_two().min(8192)
}

/// Snapshot of the analysis of the most recently synthesised frame (for meters and editors).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VoiceIsoMeter {
    /// Speech-presence probability 0…1.
    pub voice: f32,
    /// Voicing strength 0…1 (normalised autocorrelation peak).
    pub voicing: f32,
    /// Estimated pitch in Hz (0 when unvoiced).
    pub pitch_hz: f32,
    /// Mean gain applied over 200 Hz–8 kHz, in dB (≤ 0 in voice mode).
    pub reduction_db: f32,
}

/// Dialogue isolation by multi-cue spectral masking. See the module docs.
pub struct VoiceIsolator {
    pv: ParamValues,
    sr: f32,
    n: usize,
    hop: usize,
    bins: usize,
    fft: Fft,
    window: Vec<f32>,
    /// Window autocorrelation normalised to 1 at lag 0 (bias correction of the voicing).
    win_ac: Vec<f32>,
    // ---- streaming buffers (per channel, `n` samples each, laid out channel-major)
    input: Vec<f32>,
    output: Vec<f32>,
    pos: usize,
    channels: usize,
    // ---- spectral history: SLOTS × channels × bins (re, im)
    hist_re: Vec<f32>,
    hist_im: Vec<f32>,
    /// Per slot (SLOTS × bins): the broadband pre-mask (smoothed over time and frequency) and
    /// the fine structure (harmonic comb × centre focus, applied unsmoothed).
    hist_mask: Vec<f32>,
    hist_fine: Vec<f32>,
    hist_vad: [f32; SLOTS],
    hist_voicing: [f32; SLOTS],
    hist_pitch: [f32; SLOTS],
    frames: u64,
    // ---- scratch
    re: Vec<f32>,
    im: Vec<f32>,
    mix_p: Vec<f32>,
    gains: Vec<f32>,
    // ---- noise tracker
    psd: Vec<f32>,
    cur_min: Vec<f32>,
    ring: Vec<f32>,
    ring_pos: usize,
    frames_in_sub: usize,
    sub_frames: usize,
    noise_started: bool,
    g_prev: Vec<f32>,
    gamma_prev: Vec<f32>,
    // ---- smoothing state
    g_state: Vec<f32>,
    vad_state: f32,
    /// Hops left before the voice gate starts to release.
    vad_hold: u32,
    meter: VoiceIsoMeter,
    // ---- derived parameters
    psd_coef: f32,
    attack: f32,
    release: f32,
    vad_release: f32,
    vad_hold_hops: u32,
    oversub: f32,
    voicing_thr: f32,
    snr_thr: f32,
    comb_depth: f32,
    floor: f32,
    amount: f32,
    centre: f32,
    preserve: bool,
    listen_background: bool,
    out_gain: Smoothed,
}

impl VoiceIsolator {
    pub const PARAMS: &'static [ParamSpec] = &[
        ParamSpec::new("amount", "Amount", 0.0, 100.0, 85.0, Unit::Percent),
        ParamSpec::new("sensitivity", "Sensitivity", 0.0, 100.0, 65.0, Unit::Percent),
        ParamSpec::new("floor", "Background Floor", -80.0, -6.0, -48.0, Unit::Decibels),
        ParamSpec::toggle("preserve_formants", "Preserve Formants", true),
        ParamSpec::new("center_focus", "Center Focus", 0.0, 100.0, 30.0, Unit::Percent),
        ParamSpec::new("output", "Output Gain", -24.0, 24.0, 0.0, Unit::Decibels),
        ParamSpec::choice("listen", "Output", &["Isolated Voice", "Removed Background"], 0),
    ];

    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let sr = if sample_rate.is_finite() { sample_rate.clamp(SR_MIN, SR_MAX) } else { 48_000.0 };
        let n = frame_size(sr);
        let hop = n / 4;
        let bins = n / 2 + 1;
        let ch = channels.clamp(1, 64);
        let window: Vec<f32> = (0..n).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).sqrt() as f32).collect();
        let fft = Fft::new(n);
        // Autocorrelation of the window via |W|² (same pipeline as the voicing analysis).
        let (mut wr, mut wi) = (window.clone(), vec![0.0f32; n]);
        fft.forward(&mut wr, &mut wi);
        for k in 0..n {
            wr[k] = wr[k] * wr[k] + wi[k] * wi[k];
            wi[k] = 0.0;
        }
        fft.inverse(&mut wr, &mut wi);
        let w0 = wr[0].max(1e-20);
        let win_ac = wr.iter().map(|v| (v / w0).max(1e-3)).collect();
        let hop_s = hop as f32 / sr;
        let sub_frames = ((MS_WINDOW_S / hop_s / MS_SUBWINDOWS as f32).round() as usize).max(1);
        let mut s = VoiceIsolator {
            pv: ParamValues::new(Self::PARAMS),
            sr,
            n,
            hop,
            bins,
            fft,
            window,
            win_ac,
            input: vec![0.0; n * ch],
            output: vec![0.0; n * ch],
            pos: 0,
            channels: ch,
            hist_re: vec![0.0; SLOTS * ch * bins],
            hist_im: vec![0.0; SLOTS * ch * bins],
            hist_mask: vec![1.0; SLOTS * bins],
            hist_fine: vec![1.0; SLOTS * bins],
            hist_vad: [0.0; SLOTS],
            hist_voicing: [0.0; SLOTS],
            hist_pitch: [0.0; SLOTS],
            frames: 0,
            re: vec![0.0; n],
            im: vec![0.0; n],
            mix_p: vec![0.0; bins],
            gains: vec![1.0; bins],
            psd: vec![0.0; bins],
            cur_min: vec![f32::INFINITY; bins],
            ring: vec![f32::INFINITY; bins * MS_SUBWINDOWS],
            ring_pos: 0,
            frames_in_sub: 0,
            sub_frames,
            noise_started: false,
            g_prev: vec![1.0; bins],
            gamma_prev: vec![1.0; bins],
            g_state: vec![1.0; bins],
            vad_state: 0.0,
            vad_hold: 0,
            meter: VoiceIsoMeter::default(),
            psd_coef: hop_coef(0.048, hop_s),
            attack: hop_coef(0.008, hop_s),
            release: hop_coef(0.025, hop_s),
            vad_release: (-hop_s / 0.03).exp(),
            vad_hold_hops: (0.06 / hop_s).round() as u32,
            oversub: 1.0,
            voicing_thr: 0.45,
            snr_thr: 9.0,
            comb_depth: 0.6,
            floor: db_to_gain(-48.0),
            amount: 0.85,
            centre: 0.3,
            preserve: true,
            listen_background: false,
            out_gain: Smoothed::with_ms(1.0, sr, 20.0),
        };
        s.apply_params(true);
        s
    }

    fn apply_params(&mut self, snap: bool) {
        let sens = self.pv.v("sensitivity") / 100.0;
        self.oversub = 1.0 + 1.5 * sens;
        self.voicing_thr = 0.30 + 0.25 * sens;
        self.snr_thr = 3.0 + 9.0 * sens;
        self.comb_depth = 0.3 + 0.5 * sens;
        self.floor = db_to_gain(self.pv.v("floor"));
        self.amount = self.pv.v("amount") / 100.0;
        self.centre = self.pv.v("center_focus") / 100.0;
        self.preserve = self.pv.on("preserve_formants");
        self.listen_background = self.pv.idx("listen") == 1;
        self.out_gain.set(db_to_gain(self.pv.v("output")));
        if snap {
            self.out_gain.snap();
        }
    }

    /// Analysis of the most recently output frame.
    pub fn meter(&self) -> VoiceIsoMeter {
        self.meter
    }

    fn bin_hz(&self) -> f32 {
        self.sr / self.n as f32
    }

    /// Window and FFT the newest `n` input samples of every channel into history slot `slot`,
    /// accumulating the channel-sum power spectrum. Returns the mix spectrum in `re`/`im`.
    fn analyse(&mut self, slot: usize, nch: usize) -> bool {
        let (n, bins) = (self.n, self.bins);
        // samples that are exactly zero on every channel: digital silence (clip edges, pre-roll)
        let zeros = (0..n).filter(|&i| (0..nch).all(|c| self.input.get(c * n + i).is_none_or(|v| *v == 0.0))).count();
        let mut mix_re = std::mem::take(&mut self.gains);
        mix_re.iter_mut().for_each(|v| *v = 0.0);
        let mut mix_im = std::mem::take(&mut self.mix_p);
        mix_im.iter_mut().for_each(|v| *v = 0.0);
        for c in 0..self.channels {
            let base = (slot * self.channels + c) * bins;
            if c >= nch {
                self.hist_re[base..base + bins].iter_mut().for_each(|v| *v = 0.0);
                self.hist_im[base..base + bins].iter_mut().for_each(|v| *v = 0.0);
                continue;
            }
            let inp = &self.input[c * n..(c + 1) * n];
            for i in 0..n {
                self.re[i] = inp[i] * self.window[i];
                self.im[i] = 0.0;
            }
            self.fft.forward(&mut self.re, &mut self.im);
            self.hist_re[base..base + bins].copy_from_slice(&self.re[..bins]);
            self.hist_im[base..base + bins].copy_from_slice(&self.im[..bins]);
            for k in 0..bins {
                mix_re[k] += self.re[k];
                mix_im[k] += self.im[k];
            }
        }
        self.re[..bins].copy_from_slice(&mix_re);
        self.im[..bins].copy_from_slice(&mix_im);
        self.gains = mix_re;
        self.mix_p = mix_im;
        zeros * 20 > n
    }

    /// Compute the pre-mask and frame data of history slot `slot` from the mix spectrum in
    /// `re`/`im` and the per-channel spectra in the slot.
    fn estimate(&mut self, slot: usize, nch: usize, edge: bool) {
        let bins = self.bins;
        let bin_hz = self.bin_hz();
        for k in 0..bins {
            self.mix_p[k] = self.re[k] * self.re[k] + self.im[k] * self.im[k];
        }
        let total: f32 = self.mix_p.iter().sum::<f32>() / bins as f32;
        let silent = !total.is_finite() || total <= SILENCE_POWER;
        // a frame that is partly digital silence is not a noise observation (it would drag the
        // minimum down for a whole tracking window at every clip start)
        let observe = !silent && !edge;

        // ---- noise tracking (minimum statistics on the smoothed periodogram)
        let pc = self.psd_coef;
        if observe {
            for k in 0..bins {
                let p = self.mix_p[k];
                self.psd[k] = if self.noise_started { flush(self.psd[k] + pc * (p - self.psd[k])) } else { p };
                self.cur_min[k] = self.cur_min[k].min(self.psd[k]);
            }
            self.noise_started = true;
            self.frames_in_sub += 1;
        }
        let rotate = observe && self.frames_in_sub >= self.sub_frames;

        // ---- MMSE-LSA gain from a decision-directed a-priori SNR
        let mut sig = 0.0f32;
        let mut noi = 0.0f32;
        let (snr_lo, snr_hi) = ((200.0 / bin_hz) as usize, ((8000.0 / bin_hz) as usize).min(bins - 1));
        for k in 0..bins {
            let mut m = self.cur_min[k];
            for u in 0..MS_SUBWINDOWS {
                m = m.min(self.ring[u * bins + k]);
            }
            let noise = if m.is_finite() { (m * MS_BIAS).max(1e-20) } else { 1e-20 };
            if rotate {
                self.ring[self.ring_pos * bins + k] = self.cur_min[k];
                self.cur_min[k] = f32::INFINITY;
            }
            if (snr_lo..=snr_hi).contains(&k) {
                sig += self.mix_p[k];
                noi += noise;
            }
            let gamma = (self.mix_p[k] / (noise * self.oversub)).min(1e8);
            let xi = (DD_ALPHA * self.g_prev[k] * self.g_prev[k] * self.gamma_prev[k] + (1.0 - DD_ALPHA) * (gamma - 1.0).max(0.0)).max(XI_MIN);
            let g = if silent { self.g_prev[k] } else { lsa_gain(xi, gamma) };
            self.g_prev[k] = g;
            self.gamma_prev[k] = if silent { self.gamma_prev[k] } else { gamma };
            self.gains[k] = g;
        }
        if rotate {
            self.ring_pos = (self.ring_pos + 1) % MS_SUBWINDOWS;
            self.frames_in_sub = 0;
        }
        let snr_db = if silent || noi <= 0.0 { -60.0 } else { 10.0 * (sig.max(1e-30) / noi).log10() };

        // ---- voicing and pitch: autocorrelation of the 60 Hz–1.5 kHz power spectrum
        let (voicing, pitch) = if silent { (0.0, 0.0) } else { self.voicing() };

        // ---- frame voice activity
        let voiced = smoothstep(self.voicing_thr - 0.12, self.voicing_thr + 0.12, voicing);
        let snr_conf = smoothstep(self.snr_thr - 6.0, self.snr_thr + 6.0, snr_db);
        let vad = if silent { 0.0 } else { (voiced * smoothstep(-3.0, 6.0, snr_db)).max(snr_conf * (0.55 + 0.45 * voiced)) };

        // ---- per-bin pre-mask: LSA × harmonic comb × centre focus × low-cut prior
        let comb_on = voiced > 0.0 && pitch > 0.0;
        let sigma0 = (1.5 * bin_hz).max(0.18 * pitch);
        let stereo = nch == 2 && self.channels >= 2 && self.centre > 0.0;
        let mbase = slot * bins;
        for k in 0..bins {
            let f = k as f32 * bin_hz;
            let mut m = 1.0f32;
            if comb_on && f >= 0.5 * pitch && f <= COMB_MAX_HZ {
                let h = f / pitch;
                let d = (h - h.round()).abs() * pitch;
                let sigma = sigma0 * (1.0 + 0.04 * h);
                let comb = (-0.5 * (d / sigma) * (d / sigma)).exp();
                m *= 1.0 - self.comb_depth * voiced * (1.0 - comb);
            }
            if stereo {
                let l = (slot * self.channels) * bins + k;
                let r = l + bins;
                let (lr, li, rr, ri) = (self.hist_re[l], self.hist_im[l], self.hist_re[r], self.hist_im[r]);
                let d = (lr - rr) * (lr - rr) + (li - ri) * (li - ri);
                let s = (lr + rr) * (lr + rr) + (li + ri) * (li + ri);
                let side = d / (d + s + 1e-30);
                let centredness = (1.0 - 2.0 * side).clamp(0.0, 1.0);
                m *= 1.0 - self.centre * (1.0 - centredness);
            }
            self.hist_fine[mbase + k] = if m.is_finite() { m.clamp(0.0, 1.0) } else { 0.0 };
            // speech low-cut prior: 40 Hz → 0, 90 Hz → 1
            let b = self.gains[k] * smoothstep(40.0, 90.0, f);
            self.hist_mask[mbase + k] = if b.is_finite() { b.clamp(0.0, 1.0) } else { 0.0 };
        }
        self.hist_vad[slot] = vad;
        self.hist_voicing[slot] = voicing;
        self.hist_pitch[slot] = pitch;
    }

    /// Normalised autocorrelation peak (0…1) and pitch (Hz) of the mix spectrum in `mix_p`.
    fn voicing(&mut self) -> (f32, f32) {
        let (n, bins) = (self.n, self.bins);
        let bin_hz = self.bin_hz();
        for k in 0..bins {
            let f = k as f32 * bin_hz;
            let w = smoothstep(40.0, 80.0, f) * (1.0 - smoothstep(1200.0, 1800.0, f));
            self.re[k] = self.mix_p[k] * w;
            self.im[k] = 0.0;
        }
        for k in 1..n / 2 {
            self.re[n - k] = self.re[k];
            self.im[n - k] = 0.0;
        }
        self.fft.inverse(&mut self.re, &mut self.im);
        let r0 = self.re[0];
        if !r0.is_finite() || r0 <= 1e-30 {
            return (0.0, 0.0);
        }
        let lag_min = ((self.sr / F0_MAX).floor() as usize).max(2);
        let lag_max = ((self.sr / F0_MIN).ceil() as usize).min(n / 2 - 2);
        if lag_min + 2 >= lag_max {
            return (0.0, 0.0);
        }
        let norm = |re: &[f32], ac: &[f32], t: usize| re[t] / r0 / ac[t];
        let (mut best, mut best_lag) = (f32::MIN, lag_min);
        for t in lag_min..=lag_max {
            let v = norm(&self.re, &self.win_ac, t);
            if v > best {
                best = v;
                best_lag = t;
            }
        }
        // Parabolic interpolation around the peak for a fractional lag.
        let (a, b, c) = (norm(&self.re, &self.win_ac, best_lag - 1), best, norm(&self.re, &self.win_ac, best_lag + 1));
        let den = a - 2.0 * b + c;
        let off = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
        let lag = best_lag as f32 + off;
        let voicing = if best.is_finite() { best.clamp(0.0, 1.0) } else { 0.0 };
        (voicing, if voicing > 0.0 && lag > 0.0 { self.sr / lag } else { 0.0 })
    }

    /// Final gains for history slot `slot` (look-ahead voice gate, formant protection,
    /// temporal and frequency smoothing, amount, listen mode) applied and overlap-added.
    fn synthesise(&mut self, slot: usize, newest: usize, nch: usize) {
        let (n, bins) = (self.n, self.bins);
        let bin_hz = self.bin_hz();
        // look-ahead: the gate opens for the frame if any frame up to the newest is speech
        let mut onset = 0.0f32;
        for i in 0..SLOTS {
            onset = onset.max(self.hist_vad[(slot + i) % SLOTS]);
            if (slot + i) % SLOTS == newest {
                break;
            }
        }
        if onset >= self.vad_state {
            self.vad_state = onset;
            self.vad_hold = self.vad_hold_hops;
        } else if self.vad_hold > 0 {
            self.vad_hold -= 1;
        } else {
            self.vad_state = flush(onset.max(self.vad_state * self.vad_release));
        }
        let p = self.vad_state;
        let floor = self.floor;
        let protect = if self.preserve { db_to_gain(FORMANT_PROTECT_DB).max(floor) * p } else { 0.0 };
        let (f_lo, f_hi) = ((FORMANT_LO_HZ / bin_hz) as usize, (FORMANT_HI_HZ / bin_hz) as usize);
        let mbase = slot * bins;
        for k in 0..bins {
            let mut t = self.hist_mask[mbase + k] * (floor + (1.0 - floor) * p);
            if (f_lo..=f_hi).contains(&k) {
                t = t.max(protect);
            }
            let t = t.max(floor);
            let g = &mut self.g_state[k];
            let c = if t > *g { self.attack } else { self.release };
            *g = flush(*g + (t - *g) * c).max(floor);
        }
        // frequency smoothing against musical noise; while someone speaks it keeps gain peaks
        // (the voice's harmonics) and only fills in isolated dips
        let mut sum_db = 0.0f32;
        let mut cnt = 0usize;
        let (m_lo, m_hi) = ((200.0 / bin_hz) as usize, ((8000.0 / bin_hz) as usize).min(bins - 1));
        let a = self.amount;
        for k in 0..bins {
            let g = &self.g_state;
            let at = |j: isize| g[(k as isize + j).clamp(0, bins as isize - 1) as usize];
            let avg = 0.25 * at(-1) + 0.5 * at(0) + 0.25 * at(1);
            let s = avg + p * (at(0) - avg).max(0.0);
            let s = (s * self.hist_fine[mbase + k]).max(floor);
            // amount scales the reduction in dB: g^a
            let gv = if a >= 1.0 {
                s
            } else if a <= 0.0 {
                1.0
            } else {
                s.max(1e-12).powf(a)
            };
            let out = if self.listen_background { 1.0 - gv } else { gv };
            self.gains[k] = out;
            if (m_lo..=m_hi).contains(&k) {
                sum_db += 20.0 * out.max(1e-6).log10();
                cnt += 1;
            }
        }
        self.meter = VoiceIsoMeter {
            voice: p,
            voicing: self.hist_voicing[slot],
            pitch_hz: if self.hist_voicing[slot] >= self.voicing_thr { self.hist_pitch[slot] } else { 0.0 },
            reduction_db: if cnt > 0 { sum_db / cnt as f32 } else { 0.0 },
        };
        let scale = 1.0 / (n as f32 * 2.0);
        for c in 0..nch.min(self.channels) {
            let base = (slot * self.channels + c) * bins;
            for k in 0..bins {
                self.re[k] = self.hist_re[base + k] * self.gains[k];
                self.im[k] = self.hist_im[base + k] * self.gains[k];
            }
            self.im[0] = 0.0;
            self.im[n / 2] = 0.0;
            for k in 1..n / 2 {
                self.re[n - k] = self.re[k];
                self.im[n - k] = -self.im[k];
            }
            self.fft.inverse(&mut self.re, &mut self.im);
            let out = &mut self.output[c * n..(c + 1) * n];
            for i in 0..n {
                out[i] += self.re[i] * self.window[i] * scale;
            }
        }
    }

    /// One hop boundary: shift buffers, analyse the newest frame, synthesise the look-ahead one.
    fn hop_frame(&mut self, nch: usize) {
        let (n, hop) = (self.n, self.hop);
        for c in 0..self.channels {
            let out = &mut self.output[c * n..(c + 1) * n];
            out.copy_within(hop.., 0);
            out[n - hop..].iter_mut().for_each(|v| *v = 0.0);
        }
        let newest = (self.frames % SLOTS as u64) as usize;
        let edge = self.analyse(newest, nch);
        self.estimate(newest, nch, edge);
        let slot = ((self.frames + 1) % SLOTS as u64) as usize;
        if self.frames >= LOOKAHEAD as u64 {
            self.synthesise(slot, newest, nch);
        }
        self.frames += 1;
        for c in 0..self.channels {
            self.input[c * n..(c + 1) * n].copy_within(hop.., 0);
        }
    }
}

impl AudioEffect for VoiceIsolator {
    param_plumbing!("voice_isolation");

    fn latency(&self) -> usize {
        self.n + LOOKAHEAD * self.hop
    }

    fn reset(&mut self) {
        for v in [&mut self.input, &mut self.output, &mut self.hist_re, &mut self.hist_im, &mut self.psd] {
            v.iter_mut().for_each(|x| *x = 0.0);
        }
        for v in [&mut self.hist_mask, &mut self.hist_fine, &mut self.g_prev, &mut self.gamma_prev, &mut self.g_state, &mut self.gains] {
            v.iter_mut().for_each(|x| *x = 1.0);
        }
        self.cur_min.iter_mut().for_each(|x| *x = f32::INFINITY);
        self.ring.iter_mut().for_each(|x| *x = f32::INFINITY);
        self.hist_vad = [0.0; SLOTS];
        self.hist_voicing = [0.0; SLOTS];
        self.hist_pitch = [0.0; SLOTS];
        self.pos = 0;
        self.frames = 0;
        self.ring_pos = 0;
        self.frames_in_sub = 0;
        self.noise_started = false;
        self.vad_state = 0.0;
        self.vad_hold = 0;
        self.meter = VoiceIsoMeter::default();
        self.out_gain.snap();
    }

    fn process(&mut self, channels: &mut [&mut [f32]]) {
        let len = block_len(channels);
        let nch = channels.len().min(self.channels);
        let (n, hop) = (self.n, self.hop);
        for i in 0..len {
            let gain = self.out_gain.tick();
            let pos = self.pos;
            for (c, chan) in channels.iter_mut().take(nch).enumerate() {
                let x = chan[i];
                // hostile media: NaN/inf samples must not poison the recursive state
                self.input[c * n + n - hop + pos] = if x.is_finite() { x } else { 0.0 };
                chan[i] = self.output[c * n + pos] * gain;
            }
            self.pos += 1;
            if self.pos == hop {
                self.pos = 0;
                self.hop_frame(nch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    const SR: f32 = 48000.0;

    /// A synthetic "talker": a harmonic voice (f0 gliding 110…160 Hz with vibrato, formant-
    /// shaped harmonics) amplitude-modulated into ~4 syllables per second, with 300 ms pauses
    /// every second. Returns (signal, per-sample "is speaking" flags).
    fn talker(n: usize, sr: f32, amp: f32) -> (Vec<f32>, Vec<bool>) {
        let mut out = vec![0.0f32; n];
        let mut on = vec![false; n];
        let mut phase = 0.0f64;
        for i in 0..n {
            let t = i as f64 / sr as f64;
            let in_word = (t % 1.0) < 0.7;
            let f0 = 135.0 + 25.0 * (2.0 * PI * 0.7 * t).sin() + 3.0 * (2.0 * PI * 5.5 * t).sin();
            phase += 2.0 * PI * f0 / sr as f64;
            let syll = 0.5 - 0.5 * (2.0 * PI * 4.0 * t).cos();
            let env = if in_word { syll.powf(0.6) } else { 0.0 };
            let mut v = 0.0f64;
            for h in 1..=30 {
                let fh = f0 * h as f64;
                if fh > sr as f64 * 0.45 {
                    break;
                }
                // two formant bumps (~500 Hz and ~1.5 kHz) over a −6 dB/oct tilt
                let form = 1.0 + 2.0 * (-((fh - 500.0) / 250.0).powi(2)).exp() + 1.2 * (-((fh - 1500.0) / 400.0).powi(2)).exp();
                v += form / h as f64 * (phase * h as f64).sin();
            }
            out[i] = (amp as f64 * 0.25 * env * v) as f32;
            on[i] = in_word && syll > 0.3;
        }
        (out, on)
    }

    fn noise(seed: u64, n: usize, amp: f32) -> Vec<f32> {
        let mut r = Rng::new(seed);
        (0..n).map(|_| r.uniform() * amp).collect()
    }

    /// Energy of `y` (delayed by `lat`) over the samples of `x` selected by `sel`.
    fn energy_where(y: &[f32], lat: usize, sel: impl Fn(usize) -> bool) -> f64 {
        let mut e = 0.0f64;
        let mut c = 0usize;
        for i in 0..y.len().saturating_sub(lat) {
            if sel(i) {
                e += (y[i + lat] as f64).powi(2);
                c += 1;
            }
        }
        if c == 0 { 0.0 } else { e / c as f64 }
    }

    fn run(fx: &mut VoiceIsolator, chans: &mut [Vec<f32>]) {
        let mut refs: Vec<&mut [f32]> = chans.iter_mut().map(|c| c.as_mut_slice()).collect();
        fx.process(&mut refs);
    }

    #[test]
    fn zero_amount_is_a_delayed_identity() {
        for sr in [8000.0f32, 16000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            let mut fx = VoiceIsolator::new(sr, 2);
            fx.set_param("amount", 0.0);
            fx.set_param("output", 0.0);
            let lat = fx.latency();
            let x = noise(3, (sr as usize).max(lat * 3), 0.5);
            let mut y = vec![x.clone(), x.clone()];
            run(&mut fx, &mut y);
            for i in lat..x.len() {
                assert!((y[0][i] - x[i - lat]).abs() < 1e-4, "sr {sr} i {i}: {} vs {}", y[0][i], x[i - lat]);
            }
        }
    }

    #[test]
    fn voice_plus_removed_background_reconstructs_the_input() {
        let (v, _) = talker(SR as usize * 3, SR, 0.5);
        let bg = noise(4, v.len(), 0.05);
        let x: Vec<f32> = v.iter().zip(&bg).map(|(a, b)| a + b).collect();
        let mut voice = VoiceIsolator::new(SR, 1);
        let mut back = VoiceIsolator::new(SR, 1);
        back.set_param("listen", 1.0);
        let lat = voice.latency();
        let (mut a, mut b) = (vec![x.clone()], vec![x.clone()]);
        run(&mut voice, &mut a);
        run(&mut back, &mut b);
        for i in lat..x.len() {
            let s = a[0][i] + b[0][i];
            assert!((s - x[i - lat]).abs() < 1e-4, "i {i}: {s} vs {}", x[i - lat]);
        }
    }

    #[test]
    fn removes_steady_noise_and_keeps_the_voice() {
        let n = SR as usize * 8;
        let (v, on) = talker(n, SR, 0.5);
        let bg = noise(5, n, 0.03);
        let x: Vec<f32> = v.iter().zip(&bg).map(|(a, b)| a + b).collect();
        let mut fx = VoiceIsolator::new(SR, 1);
        fx.set_param("amount", 100.0);
        let lat = fx.latency();
        let mut y = vec![x.clone()];
        run(&mut fx, &mut y);
        // after warm-up (2 s): pauses are pulled down hard once a word's tail (100 ms) is over,
        // the voice keeps its level. Words end at 0.7 s of every second.
        let warm = SR as usize * 2;
        let red_in = |a: f32, b: f32| {
            let sel = |i: usize| i > warm && (i as f32 / SR) % 1.0 > a && (i as f32 / SR) % 1.0 < b;
            10.0 * (energy_where(&y[0], lat, sel) / energy_where(&x, 0, sel)).log10()
        };
        let (gap, whole) = (red_in(0.80, 0.96), red_in(0.70, 0.98));
        assert!(gap < -25.0, "background between words only reduced {gap:.1} dB");
        // the whole pause includes the 60 ms word-ending hold, kept at in-speech reduction
        assert!(whole < -12.0, "background over the whole pause only reduced {whole:.1} dB");
        let speech = |i: usize| i > warm && on[i];
        let sp_in = energy_where(&v, 0, speech);
        let sp_out = energy_where(&y[0], lat, speech);
        let kept = 10.0 * (sp_out / sp_in).log10();
        assert!(kept.abs() < 2.0, "voice level changed by {kept:.1} dB");
        // SNR during speech improves too (output vs the clean voice)
        let err: f64 = (warm..n - lat).filter(|&i| on[i]).map(|i| (y[0][i + lat] as f64 - v[i] as f64).powi(2)).sum();
        let ref_err: f64 = (warm..n - lat).filter(|&i| on[i]).map(|i| (bg[i] as f64).powi(2)).sum();
        assert!(err < ref_err * 0.7, "in-speech noise not reduced: {err} vs {ref_err}");
    }

    #[test]
    fn amount_and_floor_scale_the_reduction() {
        let n = SR as usize * 5;
        let bg = noise(6, n, 0.05);
        let red = |amount: f32, floor: f32| {
            let mut fx = VoiceIsolator::new(SR, 1);
            fx.set_param("amount", amount);
            fx.set_param("floor", floor);
            let lat = fx.latency();
            let mut y = vec![bg.clone()];
            run(&mut fx, &mut y);
            let tail = SR as usize * 3;
            10.0 * (energy_where(&y[0], lat, |i| i > tail) / energy_where(&bg, 0, |i| i > tail)).log10()
        };
        let full = red(100.0, -48.0);
        let half = red(50.0, -48.0);
        let shallow = red(100.0, -12.0);
        assert!(full < -35.0, "full {full}");
        assert!((half - full / 2.0).abs() < 4.0, "half {half} vs full {full}");
        assert!((shallow + 12.0).abs() < 2.5, "floor −12 dB → {shallow}");
    }

    /// A melody (a new note every 200 ms, decaying, three harmonics): non-stationary, so the
    /// noise tracker does not take it for noise.
    fn melody(n: usize, sr: f32, amp: f32) -> Vec<f32> {
        let notes = [262.0, 330.0, 392.0, 523.0, 440.0, 349.0, 294.0];
        let step = (sr * 0.2) as usize;
        (0..n)
            .map(|i| {
                let f = notes[(i / step) % notes.len()];
                let t = (i % step) as f64 / sr as f64;
                let env = (1.0 - (-t / 0.005).exp()) * (-t / 0.15).exp();
                let ph = 2.0 * PI * f * i as f64 / sr as f64;
                (amp as f64 * env * (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin())) as f32
            })
            .collect()
    }

    #[test]
    fn center_focus_removes_side_panned_sound() {
        let n = SR as usize * 5;
        let (v, on) = talker(n, SR, 0.4);
        // centre voice, a melody hard-left
        let bed = melody(n, SR, 0.15);
        let l: Vec<f32> = v.iter().zip(&bed).map(|(a, b)| a + b).collect();
        let r = v.clone();
        let warm = SR as usize * 2;
        let run_focus = |c: f32| {
            let mut fx = VoiceIsolator::new(SR, 2);
            fx.set_param("center_focus", c);
            let lat = fx.latency();
            let mut y = vec![l.clone(), r.clone()];
            run(&mut fx, &mut y);
            // one mask on both channels: L − R of the output is the processed bed alone
            let side: Vec<f32> = y[0].iter().zip(&y[1]).map(|(a, b)| a - b).collect();
            let bed_out = energy_where(&side, lat, |i| i > warm) / energy_where(&bed, 0, |i| i > warm);
            let voice = energy_where(&y[1], lat, |i| i > warm && on[i]);
            (10.0 * bed_out.log10(), voice)
        };
        let (bed0, voice0) = run_focus(0.0);
        let (bed1, voice1) = run_focus(100.0);
        assert!(bed1 < bed0 - 8.0, "centre focus: side bed {bed0:.1} dB → {bed1:.1} dB");
        assert!((10.0 * (voice1 / voice0).log10()).abs() < 2.0, "centre voice changed");
    }

    #[test]
    fn clean_studio_dialogue_is_left_alone() {
        let n = SR as usize * 6;
        let (v, on) = talker(n, SR, 0.5);
        // a −75 dBFS noise floor, as in a quiet booth
        let x: Vec<f32> = v.iter().zip(noise(11, n, 3e-4)).map(|(a, b)| a + b).collect();
        let mut fx = VoiceIsolator::new(SR, 1);
        let lat = fx.latency();
        let mut y = vec![x];
        run(&mut fx, &mut y);
        let warm = SR as usize * 2;
        let kept = 10.0 * (energy_where(&y[0], lat, |i| i > warm && on[i]) / energy_where(&v, 0, |i| i > warm && on[i])).log10();
        assert!(kept.abs() < 1.0, "clean dialogue changed by {kept:.2} dB");
    }

    #[test]
    fn meter_reports_voice_and_pitch() {
        let n = SR as usize * 4;
        let (v, _) = talker(n, SR, 0.5);
        let mut fx = VoiceIsolator::new(SR, 1);
        let mut saw_voice = false;
        let mut pitches = Vec::new();
        for chunk in v.chunks(1024) {
            let mut c = vec![chunk.to_vec()];
            run(&mut fx, &mut c);
            let m = fx.meter();
            assert!(m.voice.is_finite() && m.reduction_db.is_finite() && m.reduction_db <= 1e-3);
            saw_voice |= m.voice > 0.9;
            if m.pitch_hz > 0.0 {
                pitches.push(m.pitch_hz);
            }
        }
        assert!(saw_voice);
        assert!(!pitches.is_empty());
        let in_range = pitches.iter().filter(|p| (95.0..=180.0).contains(*p)).count();
        assert!(in_range * 10 >= pitches.len() * 8, "pitch mostly within the talker's range: {in_range}/{}", pitches.len());
        // pure noise: no voice
        let mut fx = VoiceIsolator::new(SR, 1);
        let mut x = vec![noise(7, n, 0.1)];
        run(&mut fx, &mut x);
        assert!(fx.meter().voice < 0.2, "noise flagged as voice: {:?}", fx.meter());
    }

    #[test]
    fn hostile_input_and_construction() {
        // NaN / inf samples don't poison later output
        let mut fx = VoiceIsolator::new(SR, 2);
        let mut l = noise(1, 20000, 0.3);
        let mut r = noise(2, 20000, 0.3);
        l[500] = f32::NAN;
        r[900] = f32::INFINITY;
        l[1200] = f32::NEG_INFINITY;
        fx.process(&mut [&mut l, &mut r]);
        let mut l = noise(3, 48000, 0.3);
        let mut r = noise(4, 48000, 0.3);
        fx.process(&mut [&mut l, &mut r]);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        assert!(rms(&l[24000..]) > 1e-6, "effect recovered after non-finite input");
        // hostile sample rates and channel counts construct and run
        for sr in [0.0f32, -48000.0, f32::NAN, f32::INFINITY, 1.0, 1e9] {
            for ch in [0usize, 1, 2, 6, 1000] {
                let mut fx = VoiceIsolator::new(sr, ch);
                let mut a = vec![0.1f32; 3000];
                let mut b = vec![-0.1f32; 3000];
                fx.process(&mut [&mut a, &mut b]);
                assert!(a.iter().chain(&b).all(|v| v.is_finite()), "sr {sr} ch {ch}");
                assert!(fx.latency() > 0);
            }
        }
        // mismatched channel lengths use the shortest
        let mut fx = VoiceIsolator::new(SR, 2);
        let mut a = vec![0.1f32; 100];
        let mut b = vec![0.1f32; 50];
        fx.process(&mut [&mut a, &mut b]);
    }

    #[test]
    fn digital_silence_is_not_learned_as_the_noise_floor() {
        // 2 s of zeros (pre-roll before a clip), then noise: the noise must be reduced quickly
        // instead of passing until the tracker's window forgets the zeros.
        let pre = SR as usize * 2;
        let n = pre + SR as usize * 2;
        let mut x = vec![0.0f32; n];
        x[pre..].copy_from_slice(&noise(8, n - pre, 0.05));
        let mut fx = VoiceIsolator::new(SR, 1);
        fx.set_param("amount", 100.0);
        let lat = fx.latency();
        let mut y = vec![x.clone()];
        run(&mut fx, &mut y);
        let s = pre + SR as usize / 2;
        let red = 10.0 * (energy_where(&y[0], lat, |i| i > s) / energy_where(&x, 0, |i| i > s)).log10();
        assert!(red < -25.0, "noise after silence reduced only {red:.1} dB");
    }

    #[test]
    fn output_gain_and_reset() {
        let mut fx = VoiceIsolator::new(SR, 1);
        fx.set_param("amount", 0.0);
        fx.set_param("output", -6.0);
        fx.reset();
        let lat = fx.latency();
        let x = noise(9, 30000, 0.3);
        let mut y = vec![x.clone()];
        run(&mut fx, &mut y);
        let g = db_to_gain(-6.0);
        for i in lat..x.len() {
            assert!((y[0][i] - x[i - lat] * g).abs() < 1e-4);
        }
    }

    /// Throughput on one core:
    /// `cargo test --release -p filmcraft-audio-dsp voice_isolation_realtime -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn voice_isolation_realtime() {
        let secs = 60usize;
        let (v, _) = talker(SR as usize * secs, SR, 0.5);
        let mut l: Vec<f32> = v.iter().zip(noise(12, v.len(), 0.03)).map(|(a, b)| a + b).collect();
        let mut r = l.clone();
        let mut fx = VoiceIsolator::new(SR, 2);
        let t0 = std::time::Instant::now();
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            fx.process(&mut [a, b]);
        }
        let t = t0.elapsed().as_secs_f64();
        println!("voice isolation: {secs} s stereo 48 kHz in {t:.3} s = {:.0}× realtime (one core)", secs as f64 / t);
    }
}
