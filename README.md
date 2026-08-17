# Mousr

## Contents

- [About](#about)
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
