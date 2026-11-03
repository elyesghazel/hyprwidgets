#!/bin/sh
# Install or update hyprwidgets:
#
#   curl -fsSL https://raw.githubusercontent.com/elyesghazel/hyprwidgets/main/get.sh | sh
#
# Builds it with cargo into ~/.local/bin (installs GTK 4, gtk4-layer-shell and Rust first on Arch,
# tells you the command elsewhere), then runs `hyprwidgets setup`: start with Hyprland, blur,
# launcher entries. Run it again to update.
#
#   … | sh -s -- --uninstall      remove hyprwidgets (your config stays)
#   … | sh -s -- --no-setup       install only
set -eu

REPO="elyesghazel/hyprwidgets"
ROOT="${HYPRWIDGETS_ROOT:-$HOME/.local}"

say() { printf '\033[1;38;2;10;132;255m❯\033[0m %s\n' "$*"; }
die() { printf '\033[38;2;255;69;58m✗\033[0m %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

setup=yes
action=install
for arg in "$@"; do
    case "$arg" in
        --uninstall) action=uninstall ;;
        --no-setup) setup=no ;;
        *) die "unknown option $arg" ;;
    esac
done

if [ "$action" = uninstall ]; then
    has hyprwidgets && hyprwidgets quit 2>/dev/null || true
    rm -f "$ROOT/bin/hyprwidgets"
    apps="${XDG_DATA_HOME:-$HOME/.local/share}"
    rm -f "$apps/applications/ch.elyesghazel.hyprwidgets.desktop" \
          "$apps/applications/ch.elyesghazel.hyprwidgets.settings.desktop" \
          "$apps/icons/hicolor/scalable/apps/hyprwidgets.svg"
    hypr="${XDG_CONFIG_HOME:-$HOME/.config}/hypr"
    for f in "$hypr/hyprland.lua" "$hypr/hyprland.conf"; do
        [ -f "$f" ] && sed -i '/^pcall(require, "hyprwidgets-setup")$/d; /^source = .*hyprwidgets-setup\.conf$/d' "$f"
    done
    rm -f "$hypr/hyprwidgets-setup.lua" "$hypr/hyprwidgets-setup.conf"
    say "hyprwidgets removed. Your settings are still in ~/.config/hyprwidgets"
    exit 0
fi

[ "$(uname -s)" = Linux ] || die "hyprwidgets is for Hyprland on Linux"

SUDO=""
[ "$(id -u)" = 0 ] || SUDO="sudo"

libs_ok() {
    has pkg-config && pkg-config --atleast-version=4.14 gtk4 && pkg-config --exists gtk4-layer-shell-0
}

if ! libs_ok || ! has cargo; then
    if has pacman; then
        pkgs="gtk4 gtk4-layer-shell pkgconf base-devel"
        has cargo || pkgs="$pkgs rust"
        has playerctl || pkgs="$pkgs playerctl"
        say "installing $pkgs"
        $SUDO pacman -S --needed --noconfirm $pkgs
    elif ! libs_ok; then
        if has dnf; then cmd="sudo dnf install gtk4-devel gtk4-layer-shell-devel playerctl"
        elif has apt-get; then cmd="sudo apt install libgtk-4-dev libgtk4-layer-shell-dev pkg-config playerctl"
        elif has zypper; then cmd="sudo zypper install gtk4-devel gtk4-layer-shell-devel playerctl"
        else cmd="your package manager: GTK 4.14+ and gtk4-layer-shell, with their development files"
        fi
        die "hyprwidgets needs GTK 4.14+ and gtk4-layer-shell. Install them with:
    $cmd
then run this again."
    fi
    has cargo || die "hyprwidgets is built with Rust. Install it from https://rustup.rs, then run this again."
fi

say "building hyprwidgets (a minute or two the first time)"
cargo install --locked --force --quiet --git "https://github.com/$REPO" --root "$ROOT" hyprwidgets

bin="$ROOT/bin/hyprwidgets"
case ":$PATH:" in
    *":$ROOT/bin:"*) ;;
    *) say "note: $ROOT/bin isn't on your PATH; setup uses the full path" ;;
esac

if "$bin" restart 2>/dev/null; then
    say "updated, the running widgets restarted"
fi
if [ "$setup" = yes ]; then
    "$bin" setup
fi
say "done. Open Hyprwidgets Settings from your launcher, or run: hyprwidgets settings"
