//! The look that can change while running: dark or light, the card style, its opacity and the
//! accent color. Turned into a stylesheet that sits above style.css.

use crate::config::Config;

pub const STYLES: &[&str] = &["glass", "frosted", "solid"];

/// A ready-made look, offered in the settings.
pub struct Preset {
    pub name: &'static str,
    pub appearance: &'static str,
    pub style: &'static str,
    pub opacity: f64,
}

pub const PRESETS: &[Preset] = &[
    Preset { name: "Midnight", appearance: "dark", style: "glass", opacity: 0.7 },
    Preset { name: "Smoke", appearance: "dark", style: "frosted", opacity: 0.8 },
    Preset { name: "Clear", appearance: "dark", style: "glass", opacity: 0.35 },
    Preset { name: "Graphite", appearance: "dark", style: "solid", opacity: 1.0 },
    Preset { name: "Daylight", appearance: "light", style: "glass", opacity: 0.7 },
    Preset { name: "Paper", appearance: "light", style: "solid", opacity: 1.0 },
];

pub const ACCENTS: &[&str] = &["", "#0a84ff", "#30d158", "#ff9f0a", "#ff453a", "#bf5af2", "#ff375f", "#64d2ff"];

/// Whether `appearance` means dark right now.
pub fn is_dark(appearance: &str) -> bool {
    match appearance {
        "light" => false,
        "dark" => true,
        _ => !std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("light")),
    }
}

pub fn style_class(style: &str) -> &'static str {
    STYLES.iter().find(|s| **s == style).copied().unwrap_or("glass")
}

/// The window classes for a config: ["dark" | "light", style].
pub fn classes(cfg: &Config) -> [&'static str; 2] {
    [if is_dark(&cfg.appearance) { "dark" } else { "light" }, style_class(&cfg.style)]
}

/// Accepts "#rgb" and "#rrggbb".
pub fn valid_color(c: &str) -> bool {
    let Some(hex) = c.strip_prefix('#') else { return false };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// The card background for one look; `card` selects the cards it applies to.
pub fn card_css(card: &str, dark: bool, style: &str, opacity: f64) -> String {
    let a = if style == "solid" { 1.0 } else { opacity.clamp(0.0, 1.0) };
    let (base, edge) = if dark { ("20, 20, 24", "255, 255, 255") } else { ("250, 250, 252", "255, 255, 255") };
    match style_class(style) {
        // a pane of glass: a light top edge and a soft sheen over the tint
        "glass" => {
            let (sheen, top, ring, shadow) =
                if dark { (0.12, 0.26, 0.09, 0.22) } else { (0.40, 0.85, 0.35, 0.14) };
            format!(
                "{card} {{ background-color: rgba({base}, {a:.2}); \
                 background-image: linear-gradient(165deg, rgba({edge}, {sheen}), rgba({edge}, {s2:.3}) 42%, rgba({edge}, 0) 70%); \
                 box-shadow: inset 0 1px 0 rgba({edge}, {top}), inset 0 0 0 1px rgba({edge}, {ring}), 0 10px 28px rgba(0, 0, 0, {shadow}); }}\n",
                s2 = sheen / 4.0
            )
        }
        "frosted" => {
            let ring = if dark { "rgba(255, 255, 255, 0.06)" } else { "rgba(255, 255, 255, 0.5)" };
            format!("{card} {{ background-color: rgba({base}, {a:.2}); background-image: none; box-shadow: inset 0 0 0 1px {ring}, 0 6px 18px rgba(0, 0, 0, 0.2); }}\n")
        }
        _ => {
            let (bg, ring) = if dark { ("#1c1c1e", "rgba(255, 255, 255, 0.06)") } else { ("#ffffff", "rgba(0, 0, 0, 0.05)") };
            format!("{card} {{ background-color: {bg}; background-image: none; box-shadow: inset 0 0 0 1px {ring}, 0 6px 18px rgba(0, 0, 0, 0.25); }}\n")
        }
    }
}

/// The whole runtime stylesheet for a config.
pub fn css(cfg: &Config) -> String {
    let dark = is_dark(&cfg.appearance);
    let mut s = card_css("window.hyprwidgets .card", dark, &cfg.style, cfg.opacity);
    // see-through cards on a bright wallpaper: a shadow keeps light text readable
    if dark && style_class(&cfg.style) != "solid" && cfg.opacity < 0.75 {
        s += ".hyprwidgets .card label { text-shadow: 0 1px 2px rgba(0, 0, 0, 0.35); }\n";
    }
    if valid_color(&cfg.accent) {
        let c = &cfg.accent;
        s += &format!(
            ".hyprwidgets .accent, .hyprwidgets.dark .accent, .hyprwidgets.light .accent {{ color: {c}; }}\n\
             .hyprwidgets button.rem-tick:hover {{ border-color: {c}; }}\n\
             .hyprwidgets button.rem-tick.done {{ background-color: {c}; border-color: {c}; }}\n\
             .hyprwidgets entry.rem-entry:focus-within {{ box-shadow: 0 0 0 2px alpha({c}, 0.55); }}\n"
        );
    }
    s
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
    }

    #[test]
    fn solid_ignores_opacity() {
        assert!(card_css(".x", true, "solid", 0.2).contains("#1c1c1e"));
        assert!(card_css(".x", true, "glass", 0.45).contains("rgba(20, 20, 24, 0.45)"));
        assert!(card_css(".x", false, "nonsense", 2.0).contains("rgba(250, 250, 252, 1.00)"));
    }
}
