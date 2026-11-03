<p align="center">
  <img src="assets/logo-256.png" width="128" alt="hyprwidgets logo">
</p>

<h1 align="center">hyprwidgets</h1>

<p align="center">Interactive desktop widgets for Hyprland. They sit on your wallpaper and show up when a workspace is empty.</p>

<p align="center"><b>Install:</b> <code>curl -fsSL https://raw.githubusercontent.com/elyesghazel/hyprwidgets/main/get.sh | sh</code></p>

<p align="center">
  <img src="assets/screenshot.png" width="380" alt="Clock, weather, calendar and reminders widgets">
</p>

- **Clock**: time, date and week number
- **Weather**: current conditions, high and low from Open-Meteo (no API key)
- **Calendar**: a month you can page through, with a dot on every day that has something. Click a day to see its events, double-click it (or the month name) to open your calendar. Medium shows what's next, Small today's date and the next event. Reads any iCal feed (Google, iCloud, Outlook, Proton, Fastmail), recurring events included. (`agenda` is the same widget, starting at Medium.)
- **Reminders**: Apple Reminders. Tick them off, or add new ones right on the desktop.
- **Now playing**: any MPRIS player (Spotify, Firefox, Chrome, mpv), with art, progress and controls
- **Pulse**: recovery, sleep and strain rings from a [Pulse](https://github.com/adityaongit/pulse) health dashboard, or anything that prints the same JSON

Each monitor gets its own set. They fade out as soon as a window opens on that workspace and fade back in when it's empty again, so they never get in the way.

**A grid, like iPad widgets.** The desktop is a grid of cells and every widget comes in fixed sizes: Mini (1×1), Small (2×2), Medium (4×2) and Large (4×4). Any 4×4 area holds one Large widget, two Medium or four Small ones. Each size has its own layout: the weather adds an hourly forecast at Medium and five days at Large, Pulse goes from one ring to three rings and the numbers behind them.

- **Move** a card by dragging it. It follows the pointer and lands on the nearest free cells (a dashed outline shows where).
- **Resize** it by dragging its bottom-right corner, or right-click it and pick a size. It steps between the sizes that widget has.
- **Collapse** it with the arrow in its corner (or the right-click menu). It shrinks to a one-row strip showing the time, the next event or "3 open". Click the strip to open it again.

All of that is remembered in `~/.local/state/hyprwidgets/layout.json`.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/elyesghazel/hyprwidgets/main/get.sh | sh
```

It builds hyprwidgets into `~/.local/bin` and runs `hyprwidgets setup`, which:

- starts it with Hyprland and blurs behind the cards (in `~/.config/hypr/hyprwidgets-setup.lua` or `.conf`, loaded from your main config; anything you already have is left alone),
- adds **Hyprwidgets** and **Hyprwidgets Settings** to your app launcher,
- writes a starter config and starts the widgets.

On Arch it installs GTK 4, `gtk4-layer-shell` and Rust first. Elsewhere it tells you the package command. `playerctl` is optional (for the media widget). Run the same line again to update; `… | sh -s -- --uninstall` removes it.

<details>
<summary>By hand</summary>

```sh
sudo pacman -S --needed gtk4 gtk4-layer-shell playerctl rust   # Arch
git clone https://github.com/elyesghazel/hyprwidgets
cd hyprwidgets
cargo build --release
install -Dm755 target/release/hyprwidgets ~/.local/bin/hyprwidgets
hyprwidgets setup
```
</details>

## Settings

Open **Hyprwidgets Settings** from your launcher, run `hyprwidgets settings`, right-click a card and pick **Settings…**, or start `hyprwidgets` again while it's running.

- **Look**: themes (Midnight, Smoke, Clear, Graphite, Daylight, Paper) previewed on your own wallpaper, or set dark/light, glass/frosted/solid, opacity and an accent color yourself. The desktop follows as you change it.
- **Widgets**: which cards are on the desktop, when they show, clock, weather place.
- **Calendars**: add your iCal links.
- **Setup**: checks autostart, blur, the launcher entries and Reminders, and fixes what's missing.

Everything is saved to `~/.config/hyprwidgets/config.toml` (your comments stay), and the widgets pick up a saved file right away, whether the settings or your editor saved it.

| Command | |
|---|---|
| `hyprwidgets` | run the widgets; already running: open the settings |
| `hyprwidgets settings [look\|widgets\|calendars\|setup]` | the settings window |
| `hyprwidgets setup` | autostart, blur, launcher entries, start |
| `hyprwidgets restart` / `quit` | restart or stop the running widgets |

## Setup in detail

The settings cover the usual things. The config file and the sections below cover the rest.

### 1. Write a config

```sh
hyprwidgets init      # writes ~/.config/hyprwidgets/config.toml
```

Every key is optional and explained in [`config.example.toml`](config.example.toml). The ones you'll most likely change:

```toml
widgets = ["clock", "weather", "calendar", "reminders", "media"]
position = "top-left"     # the corner new widgets fill from; drag cards to move them
style = "glass"           # frosted, solid
opacity = 0.7             # higher reads better on bright wallpapers
show = "empty"            # "always" keeps them up behind your windows

[weather]
location = "Zurich"
```

### 2. Hyprland: autostart and blur

`hyprwidgets setup` does this for you. By hand: start it with Hyprland, and let Hyprland blur behind the cards for the frosted-glass look.

**Lua config** (`hyprland.lua`):

```lua
hl.on("hyprland.start", function()
    hl.exec_cmd("hyprwidgets")
end)

hl.layer_rule({
    name         = "hyprwidgets",
    match        = { namespace = "^hyprwidgets$" },
    blur         = true,
    ignore_alpha = 0.2,
    no_anim      = true,
})
```

**Classic config** (`hyprland.conf`):

```ini
exec-once = hyprwidgets

layerrule = blur, hyprwidgets
layerrule = ignorealpha 0.2, hyprwidgets
layerrule = noanim, hyprwidgets
```

`ignore_alpha` keeps the blur to the cards instead of the whole rectangle around them. `no_anim` is there because the widgets fade themselves in and out.

### 3. Google Calendar

hyprwidgets reads calendars through their private iCal address. No sign-in and no OAuth app needed.

1. Open [Google Calendar](https://calendar.google.com) on a computer.
2. Click the gear, then **Settings**.
3. On the left, under **Settings for my calendars**, click the calendar.
4. Scroll to **Integrate calendar** and copy **Secret address in iCal format** (ends in `basic.ics`).
5. Add it to the config:

```toml
[[calendar.sources]]
name = "Personal"
url = "https://calendar.google.com/calendar/ical/you%40gmail.com/private-xxxxxxxx/basic.ics"
color = "#0a84ff"
```

Repeat for every calendar you want, each with its own `[[calendar.sources]]` block. Keep the address private: anyone who has it can read the calendar.

Work or school Google accounts sometimes have the secret address turned off by the admin. In that case use **Settings → Import & export → Export** and point `url` at the downloaded `.ics` file (it won't update by itself).

Google refreshes these feeds every few hours, so a new event can take a while to show up. A double click on a day opens it in Google Calendar; change `open` under `[calendar]` to use something else, or set `open_command` to run a program instead (your own calendar popup, for example).

### 4. iCloud, Outlook and other calendars

Anything that gives you an `.ics` or `webcal://` link works the same way:

- **iCloud**: in the Calendar app on iPhone, tap ⓘ next to a calendar, turn on **Public Calendar** and share the link. On a Mac: right-click the calendar → **Share Calendar** → **Public Calendar**. The `webcal://` link can go straight into the config. Note that a public link is readable by anyone who has it.
- **Outlook / Microsoft 365**: Settings → Calendar → Shared calendars → **Publish a calendar**, then copy the ICS link.
- **Proton, Fastmail, Nextcloud**: each has a "share via link" or "subscribe" URL ending in `.ics`.
- **Local file**: `url = "~/calendars/uni.ics"` works too.

### 5. Apple Reminders

Apple doesn't offer a public Reminders API, so hyprwidgets goes through [pyicloud](https://github.com/timlaing/pyicloud), the same way the iCloud website does.

1. Install the CLI (with [uv](https://docs.astral.sh/uv/)):

   ```sh
   uv tool install 'pyicloud[cli]'
   ```

2. Sign in once. pyicloud keeps its session under `$TMPDIR`, so point that somewhere that survives a reboot (the same folder hyprwidgets uses, `session_dir`):

   ```sh
   TMPDIR=~/.local/state icloud auth login --username you@icloud.com
   ```

   Enter your Apple ID password and the 2FA code from your iPhone. The password goes into your system keyring.

3. Check it works:

   ```sh
   TMPDIR=~/.local/state icloud reminders list
   ```

4. That's it: the `[reminders]` defaults already point at `~/.local/bin/icloud` and `~/.local/state`. To use one list only, take its id from `icloud reminders lists` and set `list_id`.

Tick the circle to complete a reminder, type in **New Reminder** and press Enter to add one (it goes to `list_id`, or your first list). Click the header to refresh. A listing takes a few seconds, so the last one is cached and shown right away on start.

If the card says **iCloud timed out (keyring locked?)**: pyicloud is waiting for the keyring to unlock. Unlock it (GNOME Keyring or KWallet usually unlock with your login, but not with auto-login), or install a keyring that unlocks at login. Apple also ends the session every couple of months; run the login command again when that happens.

### 6. Now playing

Install `playerctl`. Spotify, Firefox, Chromium-based browsers and most music players work out of the box. mpv needs the [mpv-mpris](https://github.com/hoyon/mpv-mpris) plugin. The card hides itself while nothing is playing (`hide_when_idle`).

### 7. Pulse

The Pulse card runs a command every 15 minutes and reads one line of JSON from it. Every key is optional:

```json
{"recovery": 72, "band": "green", "hrv": 59, "rhr": 54,
 "sleep_min": 454, "sleep_perf": 82, "debt_min": 45,
 "strain": 6.7, "strain_low": 8, "strain_high": 12, "energy": 64,
 "help": {"label": "HEART_RATE_VARIABILITY", "points": 12},
 "drag": {"label": "SLEEP_QUALITY", "points": -3}}
```

Recovery and sleep performance are 0-100, strain is 0-21. If Pulse runs on a server, a script there can read its `daily_scores` table and print this, and the command is just `ssh`:

```toml
widgets = ["clock", "weather", "calendar", "pulse"]

[pulse]
command = "ssh myserver pulse-widget"
open = "https://pulse.example.com"
```

## Styling

Themes, opacity and the accent color are in the settings (`appearance`, `style`, `opacity`, `accent` in the config; `appearance = "auto"` follows the GNOME color scheme). To change anything else, put CSS in `~/.config/hyprwidgets/style.css`. It loads on top of the [built-in stylesheet](src/style.css), so you can use the same class names:

```css
/* bigger, rounder cards */
.card { border-radius: 28px; padding: 20px; }

/* a different weather sky */
.card.weather { background-image: linear-gradient(170deg, #ff9a62, #e4547a); }
```

Classes: `.card` plus `.clock`, `.weather`, `.calendar`, `.agenda`, `.reminders`, `.media`, `.pulse`, and `.mini` / `.small` / `.medium` / `.large`. A collapsed card is `.card.pill`. The window has `.dark` or `.light` and `.glass`, `.frosted` or `.solid`.

## Writing a widget

Every widget implements one trait and is listed once in [`src/widgets/mod.rs`](src/widgets/mod.rs):

```rust
pub trait Widget {
    fn title(&self) -> &'static str;                       // shown when collapsed
    fn icon(&self) -> &'static str;                        // symbolic icon name
    fn sizes(&self) -> &'static [Size];                    // the first is the default
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget; // one card, per monitor and size
    fn summary(&self, ctx: &Ctx, label: &gtk::Label) {}    // the collapsed line
}

pub const REGISTRY: &[(&str, Constructor)] = &[
    ("clock", clock::new),
    // ("yours", yours::new),
];
```

`new` starts the widget's data source (a thread that sends into a `Shared<T>`) and returns the widget; `build` lays out a card for the given size (`card(name, size)` gives you an empty one of exactly that size) and binds labels to that `Shared<T>`. [`clock.rs`](src/widgets/clock.rs) is the smallest example, [`pulse.rs`](src/widgets/pulse.rs) one with a background source.

## Troubleshooting

- **A card is stuck somewhere odd**: Settings → Setup → Card positions → Reset.
- **Hard to read on a bright wallpaper**: raise the opacity in Settings → Look, or pick Smoke or Graphite.
- **Nothing shows**: the widgets only appear on workspaces without windows. Switch to an empty one, or set `show = "always"`. Errors go to `~/.local/state/hyprwidgets/log` when it was started by setup or the settings; or run `hyprwidgets quit; hyprwidgets` in a terminal.
- **Wrong monitor**: `monitors = ["DP-1"]` limits them to the listed outputs (`hyprctl monitors` shows the names).
- **Calendar is empty or says it can't load**: open the `.ics` address in a browser. It should download a file starting with `BEGIN:VCALENDAR`.
- **Weather can't find a place**: use the English name, or set `latitude` and `longitude`.

## License

MIT
