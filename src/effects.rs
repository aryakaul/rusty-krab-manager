//! Side effects (desktop notifications + sound) run on a dedicated worker
//! thread so they never block the async UI loop. The UI sends `Effect` messages
//! over a channel (`notify`/`play_sound` helpers in `session`); the worker owns
//! the `!Send` audio output. macOS notifications go through `osascript` (a
//! non-bundled CLI can't post via notify-rust's deprecated backend).

#[cfg(not(target_os = "macos"))]
use notify_rust::Notification;
use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;

// Side effects are run off the UI thread so that notifications and sound never
// block rendering or input handling (notify-rust in particular can stall for a
// noticeable beat on some platforms).
pub enum Effect {
    Notify { summary: String, body: String },
    Sound { path: PathBuf, volume: f32 },
}

// Spawn the background effects worker. It owns the audio output stream (which
// is `!Send` and must live on a single thread) and processes effects until all
// senders are dropped. Audio is best-effort: if no device is available, sound
// is skipped while notifications still fire.
pub fn spawn() -> Sender<Effect> {
    let (tx, rx) = mpsc::channel::<Effect>();
    thread::spawn(move || {
        let audio = DeviceSinkBuilder::open_default_sink().ok();
        for effect in rx {
            match effect {
                Effect::Notify { summary, body } => show_notification(&summary, &body),
                Effect::Sound { path, volume } => {
                    let Some(device) = audio.as_ref() else {
                        continue;
                    };
                    if let Ok(file) = File::open(&path) {
                        if let Ok(source) = Decoder::try_from(file) {
                            let player = Player::connect_new(device.mixer());
                            player.set_volume(volume);
                            player.append(source);
                            // Detach so the sound plays to completion in the
                            // background without us holding the player.
                            player.detach();
                        }
                    }
                }
            }
        }
    });
    tx
}

// macOS: a non-bundled CLI binary can't reliably post notifications through the
// deprecated NSUserNotification API that notify-rust uses by default (the call
// succeeds but nothing is displayed on recent macOS). Delegate to `osascript`,
// which posts through an authorized, bundled system app.
#[cfg(target_os = "macos")]
fn show_notification(summary: &str, body: &str) {
    // Escape for an AppleScript double-quoted string literal.
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(summary)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

// Other platforms: notify-rust's backends work for an unbundled binary.
#[cfg(not(target_os = "macos"))]
fn show_notification(summary: &str, body: &str) {
    let _ = Notification::new()
        .summary(summary)
        .body(body)
        .icon("clock")
        .show();
}
