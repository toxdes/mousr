# Mousr

## Contents

- [About](#about)
- [CLI examples](#cli-examples)
- [Default keybindings](#default-keybindings)
- [Configuration](#configuration)
- [Recommended Sway setup](#recommended-sway-setup)
- [Build](#build)

## About

Mousr is low-latency keyboard pointer control for Sway. It supports labelled
multi-output grids, recursive selection, pointer movement, clicks, dragging,
and vertical or horizontal scrolling.

It is an event-driven Rust daemon. Wayland layer-shell draws shared-memory
overlays and captures keys; keyboard-shortcuts-inhibit prevents Sway bindings
from receiving them; native Sway IPC controls the pointer. Only Sway is
currently supported.

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
```

See the [full CLI reference](CLI_REFERENCE.md).

## Default keybindings

| Mode | Keys |
| --- | --- |
| Grid labels | `a-z` select cells |
| Grid actions | `f/d/s` click; `u/e/y/o` scroll; `g` mouse; `Space` move |
| Grid navigation | `Enter` descend; `Backspace` back; `Escape` cancel |
| Mouse movement | `h/j/k/l` left/down/up/right |
| Mouse buttons | Hold `f/d/s` for left/middle/right; combine with movement to drag |
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

## Build

```sh
source ~/env/rust.sh
cargo build --release
install -Dm755 target/release/mousr ~/.local/bin/mousr
```
