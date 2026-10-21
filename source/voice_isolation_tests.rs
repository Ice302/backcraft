//! Voice Isolation through the engine: listed in its Effects-panel folder, applied by id and by
//! name to clips and as a mixer insert, parameters set by id (and refused, not crashed, when
//! hostile), latency compensated sample-exactly by the render path, Essential Sound ▸ Repair ▸
//! Isolate Voice, undo, and the project round trip.

use std::sync::Arc;

use filmcraft_project::{ClipId, ParamValue, TrackItem};
use serde_json::json;

use super::*;

const SR: u32 = 48_000;

/// A talker under steady noise: a voiced source (f0 gliding around 135 Hz, harmonics falling
/// 6 dB/oct) in ~4.5 syllables/s, speaking during the first 0.6 s of every second, plus noise.
fn noisy_voice(secs: f64, noise: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * SR as f64) as usize;
    let mut phase = 0.0f64;
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut voice = Vec::with_capacity(n);
    let mut mixed = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / SR as f64;
        let f0 = 135.0 + 25.0 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
        phase += f0 / SR as f64;
        let mut v = 0.0;
        let mut k = 1;
        while k as f64 * f0 < 4000.0 {
            v += (2.0 * std::f64::consts::PI * phase * k as f64).sin() / k as f64;
            k += 1;
        }
        let syl = 0.6 - 0.4 * (2.0 * std::f64::consts::PI * 4.5 * t).cos();
        let v = if t % 1.0 < 0.6 { (v * syl * 0.15) as f32 } else { 0.0 };
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let r = ((seed >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0;
        voice.push(v);
        mixed.push(v + r * noise);
    }
    (voice, mixed)
}

/// A session with `x` as a stereo clip on A1 at `start` seconds.
fn session_with(x: &[f32], start: f64) -> (Session, ClipId) {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "vi", "audio": 2, "video": 1})).unwrap();
    let inter: Vec<f32> = x.iter().flat_map(|v| [*v, *v]).collect();
    let bytes: Arc<[u8]> = crate::previews::write_wav_f32(&inter, SR).into();
    let item = crate::commands::import_bytes(&mut s, "/voice.wav", bytes, None).unwrap();
    let r = s.execute("timeline.place", json!({"item": item.0, "audioTrack": "A1", "seconds": start})).unwrap();
    s.execute("edit.deselectAll", json!({})).unwrap();
    (s, ClipId(r["clips"][0].as_u64().unwrap()))
}

fn item(s: &Session, c: ClipId) -> TrackItem {
    s.active_sequence().unwrap().find_item(c).unwrap().1.clone()
}

/// The clip's own signal (left channel) for `[a0, a0 + n)` timeline samples, requested in `chunk`s.
fn clip_out(s: &Session, c: ClipId, a0: i64, n: usize, chunk: usize) -> Vec<f32> {
    let it = item(s, c);
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    let mut out = Vec::new();
    let mut pos = a0;
    while pos < a0 + n as i64 {
        let m = chunk.min((a0 + n as i64 - pos) as usize);
        out.extend_from_slice(&filmcraft_render::audio::clip_signal(&it, pos, m, SR, &provider).unwrap()[0]);
        pos += m as i64;
    }
    out
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64
}

fn set(s: &mut Session, c: ClipId, param: &str, value: serde_json::Value) {
    s.execute("effects.setParam", json!({"clip": c.0, "effect": "voice_isolation", "param": param, "value": value})).unwrap();
}

#[test]
fn listed_in_noise_reduction_and_applicable_by_id_and_name() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let list = s.execute("effects.list", json!({"kind": "Audio"})).unwrap();
    let e = list.as_array().unwrap().iter().find(|e| e["id"] == "voice_isolation").expect("listed").clone();
    assert_eq!(e["name"], "Voice Isolation");
    assert_eq!(e["category"], json!(["Audio Effects", "Noise Reduction/Restoration"]));
    let clip = s.active_sequence().unwrap().audio_tracks[0].items[0].id;
    for name in ["voice_isolation", "Voice Isolation"] {
        let n0 = item(&s, clip).effects.len();
        s.execute("effects.apply", json!({"clips": [clip.0], "effect": name})).unwrap();
        let it = item(&s, clip);
        assert_eq!(it.effects.len(), n0 + 1, "{name}");
        assert_eq!(it.effects.last().unwrap().effect, "voice_isolation");
    }
    // as a track insert, set through the mixer, and the sequence still renders
    s.execute("mixer.addInsert", json!({"strip": "A1", "effect": "voice_isolation"})).unwrap();
    s.execute("mixer.setInsert", json!({"strip": "A1", "slot": 0, "params": {"amount": 100.0, "listen": 1}})).unwrap();
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    let b = filmcraft_render::audio::mix_sequence(&s.project, s.active_sequence().unwrap(), 0, 9600, &provider);
    assert_eq!(b.frames(), 9600);
    assert!(b.channels.iter().flatten().all(|v| v.is_finite()));
    // undo takes the insert and both clip effects away again
    for _ in 0..4 {
        s.execute("edit.undo", json!({})).unwrap();
    }
    assert!(item(&s, clip).effects.iter().all(|e| e.effect != "voice_isolation"));
}

#[test]
fn hostile_parameters_are_refused_or_clamped_never_crash() {
    let (x, _) = noisy_voice(2.0, 0.02);
    let (mut s, c) = session_with(&x, 0.0);
    s.execute("effects.apply", json!({"clips": [c.0], "effect": "voice_isolation"})).unwrap();
    for (param, value) in
        [("amount", json!("loud")), ("amount", json!([1, 2])), ("nope", json!(1.0)), ("listen", json!("Karaoke")), ("preserve_formants", json!("yes"))]
    {
        let r = s.execute("effects.setParam", json!({"clip": c.0, "effect": "voice_isolation", "param": param, "value": value}));
        assert!(r.is_err(), "{param} = {value} accepted");
    }
    // out-of-range numbers are kept by the project but sanitised by the DSP: the render stays finite
    for (param, value) in [("amount", 1e30), ("floor", -1e30), ("sensitivity", -5.0), ("output", 1e9), ("center_focus", f64::MAX)] {
        set(&mut s, c, param, json!(value));
    }
    set(&mut s, c, "listen", json!("Removed Background"));
    set(&mut s, c, "preserve_formants", json!(false));
    let y = clip_out(&s, c, 0, SR as usize * 2, 4096);
    assert!(y.iter().all(|v| v.is_finite()));
    assert!(y.iter().all(|v| v.abs() <= 64.0), "output gain is clamped to +24 dB");
}

/// The render path compensates the effect's latency exactly: at Amount 0 the clip sounds the same
/// with and without the effect, from the clip's first sample, however the requests are cut, from
/// a cold start in the middle too.
///
/// `render::audio_fx` caches effect chains process-wide by (clip id, effect chain, rate, width),
/// and clip ids repeat across the sessions of parallel tests, so tests that compare sample-exactly
/// use an effect chain of their own (here a 0 dB Amplify in front) or whole-range requests.
#[test]
fn latency_is_compensated_sample_exactly() {
    let (_, x) = noisy_voice(3.0, 0.02);
    let (mut s, c) = session_with(&x, 0.5);
    let a0 = item(&s, c).start.to_units_floor(SR as i64);
    let n = SR as usize * 2;
    let dry = clip_out(&s, c, a0, n, n);
    s.execute("effects.apply", json!({"clips": [c.0], "effect": "amplify"})).unwrap();
    s.execute("effects.apply", json!({"clips": [c.0], "effect": "voice_isolation"})).unwrap();
    set(&mut s, c, "amount", json!(0.0));
    for chunk in [n, 4800, 1031] {
        let wet = clip_out(&s, c, a0, n, chunk);
        let err = dry.iter().zip(&wet).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-4, "chunk {chunk}: max deviation {err}");
    }
    // cold start one second in (a seek): a fresh chain with pre-roll lands on the same samples
    let mid = clip_out(&s, c, a0 + SR as i64, SR as usize / 2, 4800);
    let err = dry[SR as usize..SR as usize * 3 / 2].iter().zip(&mid).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(err < 1e-4, "after a seek: max deviation {err}");
}

#[test]
fn isolates_the_voice_and_the_background_is_the_rest() {
    let (voice, x) = noisy_voice(5.0, 0.03);
    let (mut s, c) = session_with(&x, 0.0);
    s.execute("effects.apply", json!({"clips": [c.0], "effect": "voice_isolation"})).unwrap();
    set(&mut s, c, "amount", json!(100.0));
    // whole-range requests: always a fresh chain (see `latency_is_compensated_sample_exactly`)
    let n = SR as usize * 5;
    let kept = clip_out(&s, c, 0, n, n);
    set(&mut s, c, "listen", json!(1));
    let removed = clip_out(&s, c, 0, n, n);
    // voice + removed background = the original, sample-aligned
    let err = kept.iter().zip(&removed).zip(&x).map(|((a, b), o)| (a + b - o).abs()).fold(0.0f32, f32::max);
    assert!(err < 1e-4, "voice + background differs from the input by {err}");
    // pauses (0.75–0.95 s of every second, after warm-up) are pulled down; the voice stays
    let sel = |sig: &[f32], a: f64, b: f64| -> Vec<f32> {
        (2..5).flat_map(|k| sig[((k as f64 + a) * SR as f64) as usize..((k as f64 + b) * SR as f64) as usize].to_vec()).collect()
    };
    let gap = 10.0 * (energy(&sel(&kept, 0.8, 0.96)) / energy(&sel(&x, 0.8, 0.96))).log10();
    let speech = 10.0 * (energy(&sel(&kept, 0.1, 0.5)) / energy(&sel(&voice, 0.1, 0.5))).log10();
    println!("Voice Isolation: background in pauses {gap:.1} dB, voice {speech:+.2} dB");
    assert!(gap < -25.0, "pauses only {gap:.1} dB");
    assert!(speech.abs() < 1.5, "voice changed by {speech:.2} dB");
}

#[test]
fn essential_sound_isolate_voice_drives_the_effect() {
    let (_, x) = noisy_voice(2.0, 0.02);
    let (mut s, c) = session_with(&x, 0.0);
    s.execute("essentialSound.setType", json!({"clips": [c.0], "type": "dialogue"})).unwrap();
    let es = |s: &Session| item(s, c).effects.into_iter().filter(|e| e.essential).collect::<Vec<_>>();
    assert!(es(&s).iter().all(|e| e.effect != "voice_isolation"), "off by default");
    s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.isolate.on": true, "repair.rumble.on": true, "repair.noise.on": true}})).unwrap();
    let ids: Vec<String> = es(&s).iter().map(|e| e.effect.clone()).collect();
    assert_eq!(ids, ["highpass", "voice_isolation", "denoise"], "canonical order");
    let amount = |s: &Session| es(s).iter().find(|e| e.effect == "voice_isolation").and_then(|e| e.param("amount").map(|p| p.value.clone()));
    assert_eq!(amount(&s), Some(ParamValue::Float(80.0)), "default slider 8 → 80 %");
    s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.isolate.amount": 3.0}})).unwrap();
    assert_eq!(amount(&s), Some(ParamValue::Float(30.0)));
    // out of range is clamped, the wrong type refused
    s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.isolate.amount": 1e9}})).unwrap();
    assert_eq!(amount(&s), Some(ParamValue::Float(100.0)));
    assert!(s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.isolate.on": "on"}})).is_err());
    // the Repair switch disables it without removing it; Isolate Voice off removes it
    s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.enabled": false}})).unwrap();
    assert!(es(&s).iter().any(|e| e.effect == "voice_isolation" && !e.enabled));
    s.execute("essentialSound.set", json!({"clips": [c.0], "values": {"repair.enabled": true, "repair.isolate.on": false}})).unwrap();
    assert!(es(&s).iter().all(|e| e.effect != "voice_isolation"));
    let y = clip_out(&s, c, 0, SR as usize, 4800);
    assert!(y.iter().all(|v| v.is_finite()));
}

#[test]
fn projects_round_trip_and_older_settings_load() {
    let (_, x) = noisy_voice(1.0, 0.02);
    let (mut s, c) = session_with(&x, 0.0);
    s.execute("effects.apply", json!({"clips": [c.0], "effect": "voice_isolation"})).unwrap();
    set(&mut s, c, "sensitivity", json!(90.0));
    set(&mut s, c, "listen", json!(1));
    let text = serde_json::to_string(&*s.project).unwrap();
    let back: filmcraft_project::Project = serde_json::from_str(&text).unwrap();
    let seq = s.state.active_sequence.and_then(|id| back.sequence(id)).unwrap();
    let it = seq.find_item(c).unwrap().1.clone();
    let e = it.effects.iter().find(|e| e.effect == "voice_isolation").unwrap();
    assert_eq!(e.param("sensitivity").unwrap().value, ParamValue::Float(90.0));
    assert_eq!(e.param("listen").unwrap().value, ParamValue::Choice(1));
    // Essential Sound settings saved before Isolate Voice existed load with it off
    let old = json!({"kind": "Dialogue", "repair": {"enabled": true, "noise": {"on": true, "amount": 4.0}}});
    let es: filmcraft_project::essential::EssentialSound = serde_json::from_value(old).unwrap();
    assert!(!es.repair.isolate.on);
    assert!(es.repair.noise.on);
}
