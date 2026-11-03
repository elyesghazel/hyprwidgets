//! Colors. The palettes are zunder's, with the same names, so the widgets and the launcher look
//! like one thing; `theme = "zunder"` follows whatever zunder is set to. Turned into
//! `@define-color` lines that sit above style.css and can change while running.

use std::path::PathBuf;

use crate::config::Config;

/// The color names every theme defines, in this order (zunder's).
pub const COLORS: [&str; 9] = ["bg", "fg", "muted", "faint", "ghost", "accent", "green", "sel", "hover"];

pub struct Builtin {
    pub name: &'static str,
    /// Same order as `COLORS`.
    pub colors: [&'static str; 9],
}

#[rustfmt::skip]
pub const BUILTIN: &[Builtin] = &[
    //                           bg         fg         muted      faint      ghost      accent     green      sel        hover
    Builtin { name: "graphite",         colors: ["#0e0e14", "#e6e6ee", "#9a9aac", "#5a5a6a", "#34343f", "#b4befe", "#a6e3a1", "#262636", "#1b1b27"] },
    Builtin { name: "catppuccin-mocha", colors: ["#1e1e2e", "#cdd6f4", "#a6adc8", "#7f849c", "#45475a", "#cba6f7", "#a6e3a1", "#313244", "#28283a"] },
    Builtin { name: "catppuccin-latte", colors: ["#eff1f5", "#4c4f69", "#6c6f85", "#8c8fa1", "#bcc0cc", "#8839ef", "#40a02b", "#ccd0da", "#e6e9ef"] },
    Builtin { name: "rose-pine",        colors: ["#191724", "#e0def4", "#908caa", "#6e6a86", "#403d52", "#c4a7e7", "#9ccfd8", "#26233a", "#1f1d2e"] },
    Builtin { name: "rose-pine-dawn",   colors: ["#faf4ed", "#575279", "#797593", "#9893a5", "#cecacd", "#907aa9", "#286983", "#dfdad9", "#f2e9e1"] },
    Builtin { name: "gruvbox-dark",     colors: ["#282828", "#ebdbb2", "#bdae93", "#928374", "#504945", "#fabd2f", "#b8bb26", "#3c3836", "#32302f"] },
    Builtin { name: "tokyo-night",      colors: ["#1a1b26", "#c0caf5", "#a9b1d6", "#565f89", "#3b4261", "#7aa2f7", "#9ece6a", "#292e42", "#1f2335"] },
    Builtin { name: "nord",             colors: ["#2e3440", "#eceff4", "#d8dee9", "#7b88a1", "#4c566a", "#88c0d0", "#a3be8c", "#3b4252", "#353c4a"] },
    Builtin { name: "everforest-dark",  colors: ["#2d353b", "#d3c6aa", "#9da9a0", "#7a8478", "#4f585e", "#a7c080", "#a7c080", "#3d484d", "#343f44"] },
    Builtin { name: "dracula",          colors: ["#282a36", "#f8f8f2", "#bfc4de", "#6272a4", "#44475a", "#bd93f9", "#50fa7b", "#44475a", "#343746"] },
    Builtin { name: "kanagawa",         colors: ["#1f1f28", "#dcd7ba", "#c8c093", "#727169", "#54546d", "#7e9cd8", "#98bb6c", "#2d4f67", "#2a2a37"] },
    Builtin { name: "one-dark",         colors: ["#282c34", "#abb2bf", "#828997", "#5c6370", "#4b5263", "#61afef", "#98c379", "#3e4451", "#2c313c"] },
];

/// A theme's nine colors, by `COLORS` index.
pub type Palette = [String; 9];

fn zunder_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder")
}

/// zunder's `theme` and `accent`, if zunder is set up.
pub fn zunder_choice() -> Option<(String, String)> {
    let text = std::fs::read_to_string(zunder_dir().join("config.toml")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    let get = |k: &str| table.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
    Some((get("theme"), get("accent")))
}

/// A user theme from zunder's themes folder: `@define-color name #hex;` lines.
fn user_theme(name: &str) -> Option<Palette> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    let text = std::fs::read_to_string(zunder_dir().join("themes").join(format!("{name}.css"))).ok()?;
    let mut p: Palette = BUILTIN[0].colors.map(String::from);
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("@define-color") else { continue };
        let Some((key, value)) = rest.trim().split_once(char::is_whitespace) else { continue };
        if let Some(i) = COLORS.iter().position(|c| *c == key) {
            p[i] = value.trim().trim_end_matches(';').trim().to_owned();
        }
    }
    Some(p)
}

/// The palette for a theme name, with "zunder" resolved; and the accent zunder overrides, if any.
pub fn palette(name: &str) -> (Palette, String) {
    let (name, accent) = if name == "zunder" { zunder_choice().unwrap_or_default() } else { (name.to_owned(), String::new()) };
    let p = user_theme(&name)
        .unwrap_or_else(|| BUILTIN.iter().find(|t| t.name == name).unwrap_or(&BUILTIN[0]).colors.map(String::from));
    (p, accent)
}

/// The name "zunder" stands for right now, for the settings.
pub fn zunder_theme_name() -> Option<String> {
    zunder_choice().map(|(t, _)| if t.is_empty() { BUILTIN[0].name.to_owned() } else { t })
}

/// Accepts "#rgb" and "#rrggbb".
pub fn valid_color(c: &str) -> bool {
    let Some(hex) = c.strip_prefix('#') else { return false };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether a background color is light, by luminance.
pub fn is_light(bg: &str) -> bool {
    let hex = bg.trim_start_matches('#');
    let channel = |i: usize| hex.get(i..i + 2).and_then(|c| u8::from_str_radix(c, 16).ok()).unwrap_or(0) as f64;
    hex.len() >= 6 && 0.299 * channel(0) + 0.587 * channel(2) + 0.114 * channel(4) > 140.0
}

/// The colors for a config, accent applied: config accent, else zunder's, else the theme's.
pub fn resolved(cfg: &Config) -> Palette {
    let (mut p, zunder_accent) = palette(&cfg.theme);
    for a in [&zunder_accent, &cfg.accent] {
        if valid_color(a) {
            p[5] = a.clone();
        }
    }
    p
}

/// The window classes for a config: "dark" or "light".
pub fn classes(cfg: &Config) -> [&'static str; 1] {
    [if is_light(&resolved(cfg)[0]) { "light" } else { "dark" }]
}

/// `@define-color` lines for a palette, plus `card`: the background at `opacity`.
pub fn define_colors(p: &Palette, opacity: f64) -> String {
    let mut s: String = COLORS.iter().zip(p).map(|(n, v)| format!("@define-color {n} {v};\n")).collect();
    s += &format!("@define-color card alpha({}, {:.2});\n", p[0], opacity.clamp(0.0, 1.0));
    s
}

/// The runtime stylesheet for a config.
pub fn css(cfg: &Config) -> String {
    define_colors(&resolved(cfg), cfg.opacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert!(valid_color("#0a84ff"));
        assert!(valid_color("#fff"));
        assert!(!valid_color("0a84ff"));
        assert!(!valid_color("#0a84fg"));
        assert!(is_light("#eff1f5"));
        assert!(!is_light("#0e0e14"));
    }

    #[test]
    fn unknown_theme_is_graphite() {
        let (p, _) = palette("no-such-theme");
        assert_eq!(p[0], "#0e0e14");
        let css = define_colors(&p, 0.5);
        assert!(css.contains("@define-color accent #b4befe;"));
        assert!(css.contains("@define-color card alpha(#0e0e14, 0.50);"));
    }
}
