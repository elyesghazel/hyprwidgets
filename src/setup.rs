//! `hyprwidgets setup` and the Setup page: start with Hyprland, blur behind the cards, the
//! launcher entries. Everything here can run again safely.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use gtk4::glib;
use gtk4::glib::prelude::*;
use gtk4::gio;

use crate::config;

pub const ICON: &str = include_str!("../assets/logo.svg");
const OWN: &str = "hyprwidgets-setup";

/// Whether the widgets are running in this session.
pub fn running() -> bool {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return false };
    conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(crate::APP_ID,).to_variant()),
        Some(glib::VariantTy::new("(b)").unwrap()),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(b,)| b)
}

/// Starts the widgets in the background, logging to ~/.local/state/hyprwidgets/log.
pub fn start() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let dir = dirs::state_dir().unwrap_or_else(std::env::temp_dir).join("hyprwidgets");
    let _ = fs::create_dir_all(&dir);
    let log = fs::File::create(dir.join("log")).map_err(|e| e.to_string())?;
    crate::self_cmd()
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// What to put in an Exec line or exec_cmd: the bare name when it's on PATH.
fn exec_name() -> String {
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("hyprwidgets").is_file()));
    if on_path {
        return "hyprwidgets".into();
    }
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "hyprwidgets".into())
}

// ── launcher entries ─────────────────────────────────────

fn applications() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("applications")
}

pub fn launcher_installed() -> bool {
    let name = format!("{}.desktop", crate::APP_ID);
    applications().join(&name).exists() || Path::new("/usr/share/applications").join(&name).exists()
}

/// Two entries, so typing "widgets" or "settings" in a launcher finds the right one.
pub fn install_launcher() -> Result<Vec<PathBuf>, String> {
    let exe = exec_name();
    let apps = applications();
    let icon = dirs::data_dir().unwrap_or_default().join("icons/hicolor/scalable/apps/hyprwidgets.svg");
    let main = format!(
        "[Desktop Entry]\nType=Application\nName=Hyprwidgets\nGenericName=Desktop widgets\n\
         Comment=Widgets on empty Hyprland workspaces; opens the settings when already running\n\
         Exec={exe}\nIcon=hyprwidgets\nTerminal=false\nCategories=Utility;\n\
         Keywords=widgets;desktop;clock;weather;calendar;reminders;\nActions=settings;restart;\n\n\
         [Desktop Action settings]\nName=Settings\nExec={exe} settings\n\n\
         [Desktop Action restart]\nName=Restart\nExec={exe} restart\n"
    );
    let settings = format!(
        "[Desktop Entry]\nType=Application\nName=Hyprwidgets Settings\nComment=Look, widgets and calendars of hyprwidgets\n\
         Exec={exe} settings\nIcon=hyprwidgets\nTerminal=false\nCategories=Settings;Utility;\n\
         Keywords=widgets;theme;hyprwidgets;\nStartupWMClass={}.settings\n",
        crate::APP_ID
    );
    let files = [
        (apps.join(format!("{}.desktop", crate::APP_ID)), main),
        (apps.join(format!("{}.settings.desktop", crate::APP_ID)), settings),
        (icon, ICON.to_owned()),
    ];
    for (path, text) in &files {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(files.into_iter().map(|(p, _)| p).collect())
}

// ── Hyprland ─────────────────────────────────────────────

pub struct Hypr {
    pub dir: PathBuf,
    pub lua: bool,
    main: PathBuf,
}

impl Hypr {
    /// None when there's no Hyprland config.
    pub fn detect() -> Option<Hypr> {
        let dir = dirs::config_dir()?.join("hypr");
        let lua = dir.join("hyprland.lua").exists();
        let main = dir.join(if lua { "hyprland.lua" } else { "hyprland.conf" });
        main.exists().then_some(Hypr { dir, lua, main })
    }

    pub fn own(&self) -> PathBuf {
        self.dir.join(format!("{OWN}.{}", if self.lua { "lua" } else { "conf" }))
    }

    /// Lines of every config file but ours, comments dropped.
    fn others(&self) -> Vec<String> {
        let ext = if self.lua { "lua" } else { "conf" };
        let comment = if self.lua { "--" } else { "#" };
        let own = self.own();
        let mut out = Vec::new();
        let mut stack = vec![self.dir.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            for path in entries.flatten().map(|e| e.path()) {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == ext) && path != own {
                    let text = fs::read_to_string(&path).unwrap_or_default();
                    out.extend(text.lines().map(|l| l.split(comment).next().unwrap_or("").trim().to_owned()));
                }
            }
        }
        out
    }

    fn has_autostart(lines: &[String], lua: bool) -> bool {
        lines.iter().any(|l| {
            if lua {
                l.contains("exec_cmd") && (l.contains("\"hyprwidgets\"") || l.contains("/hyprwidgets\""))
            } else {
                l.starts_with("exec-once") && l.trim_end().ends_with("hyprwidgets")
            }
        })
    }

    fn has_blur(lines: &[String], lua: bool) -> bool {
        if lua {
            // a hl.layer_rule block for our namespace with blur in it
            let joined = lines.join("\n");
            joined.split("layer_rule").skip(1).any(|block| {
                let block = block.split("})").next().unwrap_or("");
                block.contains("hyprwidgets") && block.contains("blur")
            })
        } else {
            lines.iter().any(|l| l.starts_with("layerrule") && l.contains("blur") && l.contains("hyprwidgets"))
        }
    }

    fn own_lines(&self) -> Vec<String> {
        fs::read_to_string(self.own()).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    pub fn autostart(&self) -> bool {
        Self::has_autostart(&self.others(), self.lua) || Self::has_autostart(&self.own_lines(), self.lua) && self.linked()
    }

    pub fn blur(&self) -> bool {
        Self::has_blur(&self.others(), self.lua) || Self::has_blur(&self.own_lines(), self.lua) && self.linked()
    }

    fn link_line(&self) -> String {
        if self.lua { format!("pcall(require, \"{OWN}\")") } else { format!("source = {}", self.own().display()) }
    }

    fn linked(&self) -> bool {
        fs::read_to_string(&self.main).is_ok_and(|t| t.contains(&self.link_line()))
    }

    /// Writes our file with whatever the rest of the config lacks, and loads it from the main
    /// config. Returns what it added.
    pub fn apply(&self) -> Result<Vec<&'static str>, String> {
        let others = self.others();
        let exe = exec_name();
        let mut added = Vec::new();
        let mut s = if self.lua {
            String::from("-- written by `hyprwidgets setup`; run it again to change this, or edit it freely\n\n")
        } else {
            String::from("# written by `hyprwidgets setup`; run it again to change this, or edit it freely\n\n")
        };
        if !Self::has_autostart(&others, self.lua) {
            added.push("start with Hyprland");
            s += &if self.lua {
                format!("hl.on(\"hyprland.start\", function()\n    hl.exec_cmd(\"{exe}\")\nend)\n\n")
            } else {
                format!("exec-once = {exe}\n\n")
            };
        }
        if !Self::has_blur(&others, self.lua) {
            added.push("blur behind the cards");
            // ignore_alpha keeps the blur to the cards; the widgets fade themselves
            s += if self.lua {
                "hl.layer_rule({\n    name         = \"hyprwidgets-blur\",\n    match        = { namespace = \"^hyprwidgets$\" },\n    blur         = true,\n    ignore_alpha = 0.2,\n    no_anim      = true,\n})\n\n"
            } else {
                "layerrule = blur, hyprwidgets\nlayerrule = ignorealpha 0.2, hyprwidgets\nlayerrule = noanim, hyprwidgets\n\n"
            };
        }
        if self.lua && !others.iter().any(|l| l.contains("hyprwidgets\\\\.settings")) {
            added.push("float the settings window");
            s += "hl.window_rule({\n    name   = \"hyprwidgets-settings\",\n    match  = { class = \"^ch\\\\.elyesghazel\\\\.hyprwidgets\\\\.settings$\" },\n    float  = true,\n    size   = \"920 660\",\n    center = true,\n})\n";
        }
        fs::write(self.own(), s).map_err(|e| format!("{}: {e}", self.own().display()))?;
        if !self.linked() {
            let mut text = fs::read_to_string(&self.main).map_err(|e| e.to_string())?;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text += &self.link_line();
            text.push('\n');
            fs::write(&self.main, text).map_err(|e| format!("{}: {e}", self.main.display()))?;
        }
        Ok(added)
    }
}

/// The pyicloud CLI the Reminders card needs, if it's there.
pub fn pyicloud(cfg: &config::Config) -> bool {
    config::expand(&cfg.reminders.icloud).is_file()
}

// ── `hyprwidgets setup` ──────────────────────────────────

pub fn run() -> glib::ExitCode {
    let ok = |s: &str| println!("\x1b[32m✓\x1b[0m {s}");
    let warn = |s: &str| println!("\x1b[33m!\x1b[0m {s}");
    let mut failed = false;

    let path = config::path();
    if path.exists() {
        ok(&format!("config: {}", path.display()));
    } else {
        let _ = fs::create_dir_all(config::dir());
        match fs::write(&path, config::DEFAULT) {
            Ok(()) => ok(&format!("wrote a starter config: {}", path.display())),
            Err(e) => {
                warn(&format!("{}: {e}", path.display()));
                failed = true;
            }
        }
    }

    match install_launcher() {
        Ok(_) => ok("launcher entries: Hyprwidgets, Hyprwidgets Settings"),
        Err(e) => {
            warn(&format!("launcher entries: {e}"));
            failed = true;
        }
    }

    match Hypr::detect() {
        None => warn("no Hyprland config found (~/.config/hypr); add autostart and blur by hand, see the README"),
        Some(h) => match h.apply() {
            Ok(added) if added.is_empty() => ok("Hyprland already starts and blurs the widgets"),
            Ok(added) => ok(&format!("Hyprland ({}): {}", h.own().display(), added.join(", "))),
            Err(e) => {
                warn(&format!("Hyprland: {e}"));
                failed = true;
            }
        },
    }

    if !pyicloud(&config::load()) {
        warn("Reminders need pyicloud: uv tool install 'pyicloud[cli]' (see the README)");
    }

    if running() {
        ok("running");
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match start() {
            Ok(()) => ok("started"),
            Err(e) => warn(&format!("couldn't start: {e}")),
        }
    }
    println!("\nChange the look, widgets and calendars in Hyprwidgets Settings (`hyprwidgets settings`).");
    if failed { glib::ExitCode::FAILURE } else { glib::ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.trim().to_owned()).collect()
    }

    #[test]
    fn finds_existing_rules() {
        let lua = lines("hl.on(\"hyprland.start\", function()\n hl.exec_cmd(\"hyprwidgets\")\nend)\nhl.layer_rule({\n name = \"x\",\n match = { namespace = \"^hyprwidgets$\" },\n blur = true,\n})");
        assert!(Hypr::has_autostart(&lua, true));
        assert!(Hypr::has_blur(&lua, true));
        let other = lines("hl.exec_cmd(\"hyprwidgets settings\")\nhl.layer_rule({\n match = { namespace = \"^waybar$\" },\n blur = true,\n})\nhl.layer_rule({ match = { namespace = \"^hyprwidgets$\" }, no_anim = true })");
        assert!(!Hypr::has_autostart(&other, true));
        assert!(!Hypr::has_blur(&other, true));
        let conf = lines("exec-once = hyprwidgets\nlayerrule = blur, hyprwidgets");
        assert!(Hypr::has_autostart(&conf, false));
        assert!(Hypr::has_blur(&conf, false));
    }
}
