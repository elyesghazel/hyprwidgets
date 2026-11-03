//! Apple Reminders through the pyicloud CLI. Apple has no public API, so this drives
//! `icloud reminders ...` and caches the last listing so the widget fills instantly on start.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use serde::Deserialize;

use crate::config::{self, Reminders as Cfg};

#[derive(Clone, Debug)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub due: Option<DateTime<Local>>,
}

#[derive(Clone, Default)]
pub struct State {
    pub items: Vec<Reminder>,
    pub lists: Vec<(String, String)>,
    pub error: Option<String>,
    pub loading: bool,
}

pub enum Cmd {
    Refresh,
    Complete(String),
    Create(String),
}

#[derive(Deserialize)]
struct Row {
    id: String,
    title: String,
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    deleted: bool,
    due_date: Option<String>,
    #[serde(default)]
    list_id: String,
}

#[derive(Deserialize)]
struct List {
    id: String,
    title: String,
}

/// Starts the worker. Returns the sender widgets use to ask for changes.
pub fn watch(cfg: Cfg, tx: async_channel::Sender<State>) -> async_channel::Sender<Cmd> {
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<Cmd>();
    let cache = config::cache_dir().join("reminders.json");
    std::thread::spawn(move || {
        let mut state = State { loading: true, ..Default::default() };
        if let Some(rows) = std::fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str::<Vec<Row>>(&t).ok()) {
            state.items = open(rows, &cfg);
        }
        let _ = tx.send_blocking(state.clone());
        let mut next = Some(Cmd::Refresh);
        loop {
            let cmd = match next.take() {
                Some(c) => c,
                None => match recv_timeout(&cmd_rx, Duration::from_secs(cfg.refresh.max(1) * 60)) {
                    Ok(Some(c)) => c,
                    Ok(None) => Cmd::Refresh,
                    Err(()) => return,
                },
            };
            match cmd {
                Cmd::Refresh => {}
                Cmd::Complete(id) => {
                    // drop it right away, the network round trip takes seconds
                    state.items.retain(|r| r.id != id);
                    let _ = tx.send_blocking(state.clone());
                    if let Err(e) = run(&cfg, &["reminders", "set-status", &id, "--completed"]) {
                        state.error = Some(e);
                    }
                }
                Cmd::Create(title) => {
                    if state.lists.is_empty() {
                        state.lists = lists(&cfg).unwrap_or_default();
                    }
                    let list = if cfg.list_id.is_empty() { state.lists.first().map(|l| l.0.clone()) } else { Some(cfg.list_id.clone()) };
                    match list {
                        Some(list) => {
                            if let Err(e) = run(&cfg, &["reminders", "create", "--list-id", &list, "--title", &title]) {
                                state.error = Some(e);
                            }
                        }
                        None => state.error = Some("No reminder list found".into()),
                    }
                }
            }
            state.loading = true;
            let _ = tx.send_blocking(state.clone());
            match run(&cfg, &["reminders", "list"]).and_then(|out| serde_json::from_str::<Vec<Row>>(&out).map_err(|e| e.to_string())) {
                Ok(rows) => {
                    let _ = std::fs::write(&cache, serde_json::to_string(&rows_json(&rows)).unwrap_or_default());
                    state.items = open(rows, &cfg);
                    state.error = None;
                }
                Err(e) => state.error = Some(e),
            }
            state.loading = false;
            if tx.send_blocking(state.clone()).is_err() {
                return;
            }
            // several clicks in a row: run them all before listing again
            if let Ok(c) = cmd_rx.try_recv() {
                next = Some(c);
            }
        }
    });
    cmd_tx
}

fn recv_timeout(rx: &async_channel::Receiver<Cmd>, wait: Duration) -> Result<Option<Cmd>, ()> {
    let end = Instant::now() + wait;
    loop {
        match rx.try_recv() {
            Ok(c) => return Ok(Some(c)),
            Err(async_channel::TryRecvError::Closed) => return Err(()),
            Err(async_channel::TryRecvError::Empty) => {}
        }
        if Instant::now() >= end {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn open(rows: Vec<Row>, cfg: &Cfg) -> Vec<Reminder> {
    let mut items: Vec<Reminder> = rows
        .into_iter()
        .filter(|r| !r.completed && !r.deleted)
        .filter(|r| cfg.list_id.is_empty() || r.list_id.ends_with(cfg.list_id.trim_start_matches("List/")))
        .map(|r| Reminder {
            due: r.due_date.and_then(|d| DateTime::parse_from_rfc3339(&d.replace('Z', "+00:00")).ok()).map(|d| d.with_timezone(&Local)),
            id: r.id,
            title: r.title,
        })
        .collect();
    // soonest first, undated at the end
    items.sort_by_key(|r| (r.due.is_none(), r.due));
    items
}

fn rows_json(rows: &[Row]) -> serde_json::Value {
    rows.iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id, "title": r.title, "completed": r.completed, "deleted": r.deleted,
                "due_date": r.due_date, "list_id": r.list_id,
            })
        })
        .collect()
}

fn lists(cfg: &Cfg) -> Result<Vec<(String, String)>, String> {
    let out = run(cfg, &["reminders", "lists"])?;
    let lists: Vec<List> = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(lists.into_iter().map(|l| (l.id, l.title)).collect())
}

/// One pyicloud call with JSON output and a hard timeout: when the keyring is locked, pyicloud
/// waits for an unlock forever.
fn run(cfg: &Cfg, args: &[&str]) -> Result<String, String> {
    let bin = config::expand(&cfg.icloud);
    if !bin.exists() {
        return Err("pyicloud not installed, see the setup guide".into());
    }
    let session = config::expand(&cfg.session_dir);
    let _ = std::fs::create_dir_all(session.join("pyicloud"));
    let out_path = config::cache_dir().join(format!("icloud-{}.out", std::process::id()));
    let err_path = config::cache_dir().join(format!("icloud-{}.err", std::process::id()));
    let mut child = Command::new(bin)
        .args(args)
        .args(["--format", "json", "--log-level", "error"])
        .env("TMPDIR", &session)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&out_path).map_err(|e| e.to_string())?)
        .stderr(std::fs::File::create(&err_path).map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| e.to_string())?;
    let end = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err("iCloud timed out (keyring locked?)".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let out = std::fs::read_to_string(&out_path).unwrap_or_default();
    let err = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);
    if status.success() && out.trim_start().starts_with(['[', '{']) {
        Ok(out)
    } else {
        let msg = err.lines().chain(out.lines()).filter(|l| !l.trim().is_empty()).last().unwrap_or("iCloud failed");
        Err(msg.chars().take(80).collect())
    }
}
