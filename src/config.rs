//! `~/.config/hyprwidgets/config.toml`. Every key is optional; a missing file means the defaults.

use std::path::PathBuf;

use serde::Deserialize;

pub const DEFAULT: &str = include_str!("../config.example.toml");

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Widgets top to bottom: clock, weather, calendar, reminders, media.
    pub widgets: Vec<String>,
    /// top-left, top-right, bottom-left, bottom-right, left, right, center.
    pub position: String,
    pub margin: i32,
    /// dark, light, or auto (follows the GNOME color-scheme setting).
    pub appearance: String,
    /// glass (a pane of glass with a sheen), frosted (flat), or solid.
    pub style: String,
    /// How opaque the glass and frosted cards are, 0 (clear) to 1 (solid).
    pub opacity: f64,
    /// A color like "#0a84ff" for highlights and ticked reminders. Empty keeps the defaults.
    pub accent: String,
    /// "empty" shows the widgets only on workspaces without windows, "always" keeps them up.
    pub show: String,
    /// Monitor connector names (DP-1, HDMI-A-2). Empty means every monitor.
    pub monitors: Vec<String>,
    /// Workspace ids the widgets appear on. Empty means all of them.
    pub workspaces: Vec<i64>,
    pub clock: Clock,
    pub weather: Weather,
    pub calendar: Calendar,
    pub reminders: Reminders,
    pub media: Media,
    pub pulse: Pulse,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            widgets: ["clock", "weather", "calendar", "reminders", "media"].map(String::from).to_vec(),
            position: "top-left".into(),
            margin: 40,
            appearance: "auto".into(),
            style: "glass".into(),
            opacity: 0.7,
            accent: String::new(),
            show: "empty".into(),
            monitors: Vec::new(),
            workspaces: Vec::new(),
            clock: Clock::default(),
            weather: Weather::default(),
            calendar: Calendar::default(),
            reminders: Reminders::default(),
            media: Media::default(),
            pulse: Pulse::default(),
        }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Clock {
    pub seconds: bool,
    pub twelve_hour: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Clock { seconds: false, twelve_hour: false }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Weather {
    /// A place name, looked up once with Open-Meteo's geocoder.
    pub location: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub fahrenheit: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Weather { location: "Zurich".into(), latitude: None, longitude: None, fahrenheit: false }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Calendar {
    pub sources: Vec<CalendarSource>,
    /// How many days ahead the agenda looks.
    pub days: i64,
    /// Most events listed at once.
    pub max_events: usize,
    /// Minutes between refreshes.
    pub refresh: u64,
    /// What a click on an event or the header opens.
    pub open: String,
    /// A command to run instead of opening `open`, e.g. your own calendar popup.
    pub open_command: String,
}

impl Default for Calendar {
    fn default() -> Self {
        Calendar {
            sources: Vec::new(),
            days: 7,
            max_events: 5,
            refresh: 10,
            open: "https://calendar.google.com/calendar/r/day/{y}/{m}/{d}".into(),
            open_command: String::new(),
        }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct CalendarSource {
    pub name: String,
    /// An iCal (.ics) address: Google's "secret address in iCal format", an iCloud public
    /// calendar link, Outlook's ICS link, or a local file path.
    pub url: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reminders {
    pub enabled: bool,
    /// The pyicloud CLI. Installed with `uv tool install 'pyicloud[cli]'`.
    pub icloud: String,
    /// pyicloud keeps its session under $TMPDIR; point it at a directory that survives reboots.
    pub session_dir: String,
    /// Restrict to one list. Empty means every list, new reminders go to the first one.
    pub list_id: String,
    pub max_items: usize,
    pub refresh: u64,
}

impl Default for Reminders {
    fn default() -> Self {
        Reminders {
            enabled: true,
            icloud: "~/.local/bin/icloud".into(),
            session_dir: "~/.local/state".into(),
            list_id: String::new(),
            max_items: 6,
            refresh: 5,
        }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Media {
    /// Hide the card while nothing is playing or paused.
    pub hide_when_idle: bool,
}

impl Default for Media {
    fn default() -> Self {
        Media { hide_when_idle: true }
    }
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Pulse {
    /// Prints the scores as one JSON line (see the README).
    pub command: String,
    /// Minutes between refreshes.
    pub refresh: u64,
    /// Opened on click.
    pub open: String,
}

impl Default for Pulse {
    fn default() -> Self {
        Pulse { command: String::new(), refresh: 15, open: String::new() }
    }
}

pub fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config")).join("hyprwidgets")
}

pub fn path() -> PathBuf {
    dir().join("config.toml")
}

impl Config {
    /// Equal apart from the look, which can change while running.
    pub fn same_but_look(&self, other: &Config) -> bool {
        let mut o = other.clone();
        o.appearance.clone_from(&self.appearance);
        o.style.clone_from(&self.style);
        o.opacity = self.opacity;
        o.accent.clone_from(&self.accent);
        *self == o
    }
}

pub fn load() -> Config {
    try_load().unwrap_or_else(|e| {
        eprintln!("hyprwidgets: {e}");
        Config::default()
    })
}

/// The config, or why it can't be read. A missing file is the defaults.
pub fn try_load() -> Result<Config, String> {
    match std::fs::read_to_string(path()) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path().display())),
        Err(_) => Ok(Config::default()),
    }
}

/// `~/x` to an absolute path.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

pub fn cache_dir() -> PathBuf {
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("hyprwidgets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
