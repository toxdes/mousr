# Mousr

## Contents

- [About](#about)
- [CLI examples](#cli-examples)
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
