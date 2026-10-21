//! Headless UI tests of Voice Isolation: its Clip Fx Editor (before/after view, presets, listen
//! switch, sliders, reset) opened from Effect Controls, its Track Fx Editor opened from a mixer
//! slot, and Essential Sound ▸ Repair ▸ Isolate Voice.
//!
//! Set `FILMCRAFT_UI_SNAPSHOT_DIR=<dir>` to also write PNGs of the windows (`voice-isolation-*.png`).

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
    snapshots: Option<std::path::PathBuf>,
}

impl Driver {
    fn demo() -> Self {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let snapshots = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR").map(std::path::PathBuf::from);
        let mut b = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000);
        if snapshots.is_some() {
            b = b.wgpu();
        }
        let harness = b.build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx, snapshots };
        d.frames(4);
        d
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                return v;
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
        v["result"].clone()
    }

    fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({"command": command, "params": params}))
    }

    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(2);
    }

    fn element(&mut self, id: &str) -> Option<[f32; 4]> {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array()?.iter().find(|e| e["id"] == id).map(|e| {
            let r = &e["rect"];
            [r[0].as_f64().unwrap() as f32, r[1].as_f64().unwrap() as f32, r[2].as_f64().unwrap() as f32, r[3].as_f64().unwrap() as f32]
        })
    }

    fn ids(&mut self, prefix: &str) -> Vec<String> {
        let v = self.ok("ui.elements", json!({"prefix": prefix}));
        v.as_array().unwrap().iter().filter_map(|e| e["id"].as_str().map(str::to_string)).collect()
    }

    /// Press on `id` at `fx` of its width, move by `dx` over several frames, release.
    fn drag(&mut self, id: &str, fx: f32, dx: f32, steps: usize) {
        let r = self.element(id).unwrap_or_else(|| panic!("no element {id}"));
        let mut p = egui::pos2(r[0] + r[2] * fx, r[1] + r[3] / 2.0);
        let push = |d: &mut Self, e: egui::Event| d.harness.input_mut().events.push(e);
        push(self, egui::Event::PointerMoved(p));
        self.frames(1);
        push(self, egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        self.frames(1);
        for _ in 0..steps {
            p.x += dx / steps as f32;
            push(self, egui::Event::PointerMoved(p));
            self.frames(1);
        }
        push(self, egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        self.frames(3);
    }

    fn snapshot(&mut self, name: &str) {
        let Some(dir) = self.snapshots.clone() else { return };
        self.frames(2);
        match self.harness.render() {
            Ok(img) => {
                std::fs::create_dir_all(&dir).unwrap();
                img.save(dir.join(format!("{name}.png"))).unwrap();
            }
            Err(e) => eprintln!("snapshot {name} skipped: {e}"),
        }
    }

    fn undo_len(&mut self) -> usize {
        self.exec("history.list", json!({}))["undo"].as_array().unwrap().len()
    }

    /// Audio clip on A1 (id) with Voice Isolation applied and selected; Effect Controls showing it.
    fn audio_clip_with_voice_isolation(&mut self) -> u64 {
        self.ok("ui.set", json!({"workspace": "Effects"}));
        let seq = self.exec("sequence.inspect", json!({}));
        let clip = &seq["audio"][0]["items"][0];
        let (id, start, dur) = (clip["clip"].as_u64().unwrap(), clip["start"].as_i64().unwrap(), clip["duration"].as_i64().unwrap_or(0));
        // the playhead well inside the clip, so the editor has audio on both sides
        self.exec("playhead.set", json!({"time": start + dur / 2}));
        self.exec("sequence.linkedSelection", json!({"on": false}));
        self.exec("timeline.select", json!({"clips": [id]}));
        self.exec("effects.apply", json!({"clips": [id], "effect": "voice_isolation"}));
        self.frames(3);
        for fx in ["volume", "channel_volume", "panner"] {
            self.click(&format!("effectControls.effect.{fx}.twirl"));
        }
        id
    }

    fn clip_param(&mut self, clip: u64, param: &str) -> Value {
        let seq = self.exec("sequence.inspect", json!({}));
        let items = seq["audio"][0]["items"].as_array().unwrap().clone();
        let it = items.iter().find(|i| i["clip"] == clip).unwrap().clone();
        let fx = it["effects"].as_array().unwrap().iter().find(|e| e["effect"] == "voice_isolation").unwrap().clone();
        fx["params"][param]["value"].clone()
    }
}

/// A parameter value from `sequence.inspect` (`"Float(3.5)"`, `"Choice(1)"`) or plain JSON.
fn num(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim_start_matches("Float(").trim_start_matches("Choice(").trim_end_matches(')').parse().ok()))
        .unwrap_or(f64::NAN)
}

#[test]
fn clip_editor_view_presets_listen_and_reset() {
    let mut d = Driver::demo();
    let clip = d.audio_clip_with_voice_isolation();
    // Effect Controls lists every parameter of the effect
    let rows = d.ids("effectControls.");
    assert!(rows.iter().any(|i| i == "effectControls.effect.voice_isolation.edit"), "{rows:?}");
    d.click("effectControls.effect.voice_isolation.edit");
    d.frames(3);
    let ids = d.ids("fxEditor.voice_isolation.");
    for id in [
        "view",
        "voice",
        "preset.gentle",
        "preset.balanced",
        "preset.strong",
        "preset.music",
        "listen.0",
        "listen.1",
        "param.amount",
        "param.sensitivity",
        "param.floor",
        "param.center_focus",
        "param.output",
        "toggle.preserve_formants",
        "reset",
        "close",
    ] {
        assert!(ids.iter().any(|i| *i == format!("fxEditor.voice_isolation.{id}")), "{id} missing from {ids:?}");
    }
    d.snapshot("voice-isolation-editor");
    // a preset writes four parameters as one undo step
    let undo0 = d.undo_len();
    d.click("fxEditor.voice_isolation.preset.strong");
    assert_eq!(num(&d.clip_param(clip, "amount")), 100.0);
    assert_eq!(num(&d.clip_param(clip, "sensitivity")), 85.0);
    assert_eq!(num(&d.clip_param(clip, "floor")), -70.0);
    assert_eq!(num(&d.clip_param(clip, "center_focus")), 40.0);
    assert_eq!(d.undo_len(), undo0 + 1, "a preset is one undo step");
    d.exec("edit.undo", json!({}));
    assert_eq!(num(&d.clip_param(clip, "amount")), 85.0);
    // listen to what is removed
    d.click("fxEditor.voice_isolation.listen.1");
    assert_eq!(num(&d.clip_param(clip, "listen")), 1.0);
    d.snapshot("voice-isolation-editor-background");
    // dragging the Amount slider left lowers it — one undo step for the drag
    let undo1 = d.undo_len();
    d.drag("fxEditor.voice_isolation.param.amount", 0.6, -80.0, 6);
    let a = num(&d.clip_param(clip, "amount"));
    assert!(a < 85.0, "amount after drag: {a}");
    assert_eq!(d.undo_len(), undo1 + 1);
    // reset brings every parameter back
    d.click("fxEditor.voice_isolation.reset");
    assert_eq!(num(&d.clip_param(clip, "amount")), 85.0);
    assert_eq!(num(&d.clip_param(clip, "listen")), 0.0);
    d.click("fxEditor.voice_isolation.close");
    assert!(d.ids("fxEditor.voice_isolation.").is_empty(), "window closed");
}

#[test]
fn track_editor_from_the_mixer_slot() {
    let mut d = Driver::demo();
    d.exec("mixer.addInsert", json!({"strip": "A1", "effect": "voice_isolation"}));
    d.ok("ui.set", json!({"workspace": "Audio"}));
    d.frames(3);
    d.click("mixer.showEffects");
    d.click("mixer.A1.fx.0");
    d.click("mixer.A1.fx.0.edit");
    d.frames(3);
    assert!(d.element("fxEditor.voice_isolation.view").is_some());
    d.click("fxEditor.voice_isolation.preset.music");
    let m = d.exec("mixer.inspect", json!({}));
    let a1 = m["strips"].as_array().unwrap().iter().find(|s| s["ref"] == "A1").unwrap().clone();
    let p = &a1["inserts"][0]["params"];
    assert_eq!(num(&p["center_focus"]), 70.0, "{a1}");
    assert_eq!(num(&p["sensitivity"]), 90.0, "{a1}");
    d.snapshot("voice-isolation-track-editor");
}

#[test]
fn essential_sound_isolate_voice_switch() {
    let mut d = Driver::demo();
    d.ok("ui.set", json!({"workspace": "Audio"}));
    d.frames(3);
    let seq = d.exec("sequence.inspect", json!({}));
    let a1 = seq["audio"][0]["items"][0]["clip"].as_u64().unwrap();
    d.exec("timeline.select", json!({"clips": [a1]}));
    d.frames(3);
    d.click("essentialSound.type.Dialogue");
    d.frames(2);
    let ids = d.ids("essentialSound.repair.");
    for id in ["essentialSound.repair.isolate.on", "essentialSound.repair.isolate.amount", "essentialSound.repair.noise.on"] {
        assert!(ids.iter().any(|i| i == id), "{id} missing from {ids:?}");
    }
    d.click("essentialSound.repair.isolate.on");
    let c = d.exec("essentialSound.inspect", json!({"clips": [a1]}))["clips"][0].clone();
    assert_eq!(c["settings"]["repair"]["isolate"]["on"], true);
    assert!(c["effects"].as_array().unwrap().iter().any(|e| e["effect"] == "voice_isolation"), "{c}");
    d.snapshot("voice-isolation-essential-sound");
}
