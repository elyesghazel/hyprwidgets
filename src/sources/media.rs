//! Now playing from any MPRIS player (Spotify, browsers, mpv), through `playerctl --follow`.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub playing: bool,
    pub player: String,
    pub title: String,
    pub artist: String,
    pub art: String,
    /// Seconds.
    pub length: f64,
    pub position: f64,
    /// When `position` was read, so the widget can count on from it.
    pub at: Option<Instant>,
}

impl Track {
    pub fn active(&self) -> bool {
        !self.title.is_empty()
    }

    pub fn position_now(&self) -> f64 {
        let extra = match (self.playing, self.at) {
            (true, Some(at)) => at.elapsed().as_secs_f64(),
            _ => 0.0,
        };
        (self.position + extra).min(self.length.max(self.position))
    }
}

const FORMAT: &str = "{{status}}\t{{playerName}}\t{{title}}\t{{artist}}\t{{mpris:artUrl}}\t{{mpris:length}}";

pub fn watch(tx: async_channel::Sender<Track>) {
    std::thread::spawn(move || {
        loop {
            let child = Command::new("playerctl")
                .args(["metadata", "--follow", "--format", FORMAT])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let Ok(mut child) = child else {
                // no playerctl: say so once and stop
                let _ = tx.send_blocking(Track::default());
                return;
            };
            let stdout = child.stdout.take().unwrap();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send_blocking(parse(&line)).is_err() {
                    let _ = child.kill();
                    return;
                }
            }
            let _ = child.wait();
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn parse(line: &str) -> Track {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 6 || f[2].is_empty() {
        return Track::default();
    }
    let position = Command::new("playerctl")
        .args(["-p", f[1], "position"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0.0);
    Track {
        playing: f[0] == "Playing",
        player: f[1].into(),
        title: f[2].into(),
        artist: f[3].into(),
        art: f[4].into(),
        length: f[5].trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0,
        position,
        at: Some(Instant::now()),
    }
}

pub fn control(player: &str, action: &str) {
    let mut cmd = Command::new("playerctl");
    if !player.is_empty() {
        cmd.args(["-p", player]);
    }
    let _ = cmd.arg(action).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
