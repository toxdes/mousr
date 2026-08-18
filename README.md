# Mousr

## Contents

- [About](#about)
- [Installation](#installation)
- [CLI examples](#cli-examples)
- [Default keybindings](#default-keybindings)
- [Configuration](#configuration)
- [Recommended Sway setup](#recommended-sway-setup)

## About

Mousr is low-latency keyboard pointer control for Sway. It supports labelled
multi-output grids, recursive selection, pointer movement, clicks, dragging,
and vertical or horizontal scrolling.

It is an event-driven Rust daemon. Wayland protocols provide output data, draw
the overlays, capture keys, inhibit shortcuts, and emit pointer events. Sway
IPC is used only to identify the focused output. Only Sway is tested and
supported.

## CLI examples

```sh
mousr grid
mousr grid --scope all
mousr grid --output HDMI-A-1 --action left
mousr grid --action mouse --max-depth 3
mousr mouse
mousr click middle
mousr scroll left --step 30
mousr reload
mousr cancel

# Diagnostic logging
mousr --log-level debug daemon
mousr grid --log-level debug
mousr --log-level debug --log-file ~/.local/state/mousr/logs/debug.json daemon
```

Logs use compact `DBG`, `INF`, `WRN`, `ERR`, and `PNC` prefixes on stderr. The
default level is `info`; logging options apply to the current process, so the
daemon must be started with `--log-level debug` to enable daemon diagnostics.
`--log-file` additionally writes JSON-lines diagnostics to the selected file.

See the [full CLI reference](CLI_REFERENCE.md).

## Installation

Mousr requires a Wayland session and currently supports Sway. Packages install
the required Wayland and XKB libraries, but do not install Sway itself.

### Ubuntu and Debian

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://packages.toxdes.com/apt/pubkey.gpg \
  | sudo gpg --dearmor --yes -o /etc/apt/keyrings/toxdes.gpg

printf '%s\n' \
  'deb [signed-by=/etc/apt/keyrings/toxdes.gpg] https://packages.toxdes.com/apt stable main' \
  | sudo tee /etc/apt/sources.list.d/toxdes.list >/dev/null

sudo apt update
sudo apt install mousr
```

### Fedora

Add the repository:

```sh
tmp_key="$(mktemp)"
curl --fail --silent --show-error \
  https://packages.toxdes.com/rpm/pubkey.gpg \
  -o "$tmp_key"
sudo rpm --import "$tmp_key"
rm -f "$tmp_key"

printf '%s\n' \
  '[toxdes]' \
  'name=Toxdes packages' \
  'baseurl=https://packages.toxdes.com/rpm' \
  'enabled=1' \
  'gpgcheck=1' \
  'repo_gpgcheck=1' \
  'gpgkey=https://packages.toxdes.com/rpm/pubkey.gpg' \
  | sudo tee /etc/yum.repos.d/toxdes.repo >/dev/null

sudo dnf makecache
sudo dnf install mousr
```

### Arch Linux

Mousr is available in the AUR.

```sh
yay -S mousr-bin
```

If you prefer the latest development version:

```sh
yay -S mousr-git
```

### Prebuilt binaries

Download the prebuilt `amd64` or `arm64` archive from the
[releases](https://github.com/toxdes/mousr/releases/latest) and add `mousr` to
your `PATH`.

> [!WARNING]
> Direct archives do not install runtime dependencies. Install
> `libwayland-client0` and `libxkbcommon0` on Debian/Ubuntu,
> `wayland-libs` and `libxkbcommon` on Fedora, or `wayland` and `libxkbcommon`
> on Arch.

### With `cargo install`

```sh
cargo install mousr
```

## Default keybindings

| Mode | Keys |
| --- | --- |
| Grid labels | `a-z` select cells |
| Grid actions | `s/d/f` left/middle/right click; `u/e/y/o` scroll; `g` mouse; `Space` move |
| Grid navigation | `Enter` descend; `Backspace` back; `Escape` cancel |
| Mouse movement | `h/j/k/l` left/down/up/right |
| Mouse buttons | Hold `s/d/f` for left/middle/right; combine with movement to drag |
| Mouse button lock | `v`, then `s/d/f` locks a button; `v` releases it |
| Mouse scrolling | `u/e/y/o` up/down/left/right |
| Mouse exit | `Escape` releases held buttons and exits |

## Configuration

See [configuration reference](CONFIGURATION.md) and the complete
[example configuration](mousr.example.toml).

## Recommended Sway setup

```sway
# Start Mousr with Sway.
exec mousr daemon

# Alt+Space: grid mode
bindsym Mod1+space exec mousr grid

# Alt+Shift+Space: mouse mode
bindsym Mod1+Shift+space exec mousr mouse
```

Building from source or contributing? See [CONTRIBUTING.md](CONTRIBUTING.md).
