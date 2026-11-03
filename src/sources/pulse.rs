//! Health scores from Pulse (github.com/adityaongit/pulse) or anything else that prints the same
//! JSON: a command run every few minutes, e.g. over ssh to the server Pulse runs on.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::config::{self, Pulse as Cfg};

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Scores {
    pub day: String,
    /// 0-100.
    pub recovery: Option<f64>,
    /// green, yellow or red.
    pub band: Option<String>,
    pub hrv: Option<f64>,
    pub rhr: Option<f64>,
    pub sleep_min: Option<f64>,
    /// Sleep performance, 0-100.
    pub sleep_perf: Option<f64>,
    pub sleep_need_h: Option<f64>,
    pub debt_min: Option<f64>,
    /// Day strain, 0-21.
    pub strain: Option<f64>,
    pub strain_low: Option<f64>,
    pub strain_high: Option<f64>,
    /// Energy bank, 0-100.
    pub energy: Option<f64>,
    pub drag: Option<Driver>,
    pub help: Option<Driver>,
    pub error: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
pub struct Driver {
    pub label: String,
    pub points: f64,
}

pub fn watch(cfg: Cfg, tx: async_channel::Sender<Scores>) {
    if cfg.command.trim().is_empty() {
        let _ = tx.send_blocking(Scores { error: Some("Set [pulse] command in config.toml".into()), ..Default::default() });
        return;
    }
    let cache = config::cache_dir().join("pulse.json");
    // the last numbers right away; ssh can take a moment
    if let Some(s) = std::fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        let _ = tx.send_blocking(s);
    }
    std::thread::spawn(move || {
        loop {
            let scores = match fetch(&cfg.command) {
                Ok(text) => match serde_json::from_str::<Scores>(&text) {
                    Ok(s) => {
                        if s.error.is_none() {
                            let _ = std::fs::write(&cache, &text);
                        }
                        s
                    }
                    Err(e) => Scores { error: Some(format!("Bad JSON: {e}")), ..Default::default() },
                },
                Err(e) => Scores { error: Some(e), ..Default::default() },
            };
            if tx.send_blocking(scores).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(cfg.refresh.max(1) * 60));
        }
    });
}

fn fetch(command: &str) -> Result<String, String> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > Duration::from_secs(30) => {
                let _ = child.kill();
                return Err("Pulse timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).map_err(|e| e.to_string())?;
    let line = out.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or("").to_string();
    if line.is_empty() { Err("Pulse sent nothing".into()) } else { Ok(line) }
}
