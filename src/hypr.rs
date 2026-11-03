//! Which monitors currently show an empty workspace, kept up to date from Hyprland's event socket.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

/// Monitor connector name -> (active workspace id, window count on it).
pub type Monitors = HashMap<String, (i64, u32)>;

#[derive(Deserialize)]
struct Monitor {
    name: String,
    #[serde(rename = "activeWorkspace")]
    active: WorkspaceRef,
    #[serde(rename = "specialWorkspace")]
    special: Option<WorkspaceRef>,
}

#[derive(Deserialize)]
struct WorkspaceRef {
    id: i64,
}

#[derive(Deserialize)]
struct Workspace {
    id: i64,
    windows: u32,
}

/// Events that can change what a monitor shows.
const EVENTS: &[&str] = &[
    "workspace", "focusedmon", "openwindow", "closewindow", "movewindow", "movetoworkspace",
    "activespecial", "createworkspace", "destroyworkspace", "moveworkspace", "monitoradded",
    "monitorremoved", "changefloatingmode", "minimized",
];

fn hyprctl<T: for<'de> Deserialize<'de>>(what: &str) -> Option<T> {
    let out = Command::new("hyprctl").args(["-j", what]).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

pub fn snapshot() -> Monitors {
    let (Some(monitors), Some(workspaces)) =
        (hyprctl::<Vec<Monitor>>("monitors"), hyprctl::<Vec<Workspace>>("workspaces"))
    else {
        return Monitors::new();
    };
    let windows: HashMap<i64, u32> = workspaces.into_iter().map(|w| (w.id, w.windows)).collect();
    monitors
        .into_iter()
        .map(|m| {
            // an open special workspace (scratchpad) covers the desktop too
            let special = m.special.filter(|s| s.id != 0).map_or(0, |s| windows.get(&s.id).copied().unwrap_or(0));
            let count = windows.get(&m.active.id).copied().unwrap_or(0) + special;
            (m.name, (m.active.id, count))
        })
        .collect()
}

fn socket_dir() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(dirs::runtime_dir()?.join("hypr").join(sig))
}

fn socket_path() -> Option<PathBuf> {
    Some(socket_dir()?.join(".socket2.sock"))
}

/// Sends a fresh snapshot on start and after every relevant event, forever. Reconnects when
/// Hyprland restarts the socket.
pub fn watch(tx: async_channel::Sender<Monitors>) {
    std::thread::spawn(move || {
        loop {
            let _ = tx.send_blocking(snapshot());
            let Some(stream) = socket_path().and_then(|p| UnixStream::connect(p).ok()) else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let event = line.split(">>").next().unwrap_or("");
                if EVENTS.contains(&event) && tx.send_blocking(snapshot()).is_err() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
