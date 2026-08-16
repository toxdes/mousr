# Mousr

Mousr is a small, daemon-backed keyboard pointer controller for Sway. It draws a
labelled grid with Wayland layer-shell, captures the keyboard while a mode is
active, and asks Sway to move, click, drag, or scroll the pointer.

Only Sway is supported. The compositor boundary is isolated in one module so a
Hyprland adapter can be added later, but no untestable Hyprland behavior is
shipped.

## Build and run

```sh
source ~/env/rust.sh
cargo build --release
install -Dm755 target/release/mousr ~/.local/bin/mousr
```

Start the daemon once from the Sway config and bind whichever entry points you
want:

```sway
exec_always --no-startup-id mousr daemon
bindsym Mod1+space exec mousr grid
bindsym Mod1+m exec mousr mouse
```

The daemon keeps the Wayland connection, output metadata, font, renderer, and
overlay surfaces ready. CLI invocations are short-lived clients communicating
through a mode-0600 socket in `XDG_RUNTIME_DIR`.

## Grid mode

Typing a label selects its cell. A selected cell accepts these default keys:

| Key | Action |
| --- | --- |
| `f`, `d`, `s` | left, middle, right click |
| `u`, `e` | scroll up, down |
| `y`, `o` | scroll left, right |
| `g` | enter mouse mode at the cell centre |
| `Space` | move only and exit |
| `Enter` | descend into the cell |
| `Backspace` | erase a label character or go up one level |
| `Escape` | cancel |

Labels are fixed-width base-26 strings. Mousr chooses the shortest configured
label length that addresses the derived cells; for example, one character gives
`a` through `z`, while two give `aa` onward. Cell count is derived from output
dimensions, minimum tile dimensions, label capacity, and `max_cells`.

Examples:

```sh
mousr grid --scope all
mousr grid --output HDMI-A-1 --action left
mousr grid --action mouse --max-depth 3
mousr click right
mousr scroll left --step 30
mousr reload
mousr cancel
```

`--scope focused` means the output containing Sway's focused workspace.
`--scope all` gives every active output unique labels. `--output NAME` targets an
exact Sway output and is mutually exclusive with `--scope`.

## Mouse mode

`h`, `j`, `k`, and `l` move the pointer. `f`, `d`, and `s` mirror button state:
pressing a key presses its mouse button and releasing it releases the button.
Holding one while moving therefore performs drag-and-drop. `u`/`e` scroll
vertically and `y`/`o` horizontally. `Escape` releases every held button before
exiting.

Wheel thresholds are expressed in units where 15 is one wheel notch. Fractional
values below one notch are rounded up. Motion begins immediately, follows the
keyboard's repeat stream, accelerates from `initial_speed`, and is capped by
`max_speed`. `tick_hz` determines the initial movement quantum.

## Configuration

The default file is `$XDG_CONFIG_HOME/mousr/config.toml`, or
`~/.config/mousr/config.toml`. See [mousr.example.toml](mousr.example.toml) for
every option. Unknown keys and invalid values are rejected. `mousr reload`
atomically replaces the active configuration only after parsing and validation
succeed.

Mousr requests Sway's keyboard-shortcuts-inhibit protocol and defaults to
fail-closed startup if it is unavailable. While active, its layer surface has
exclusive keyboard focus and an empty pointer input region, so pointer events
still reach the application below it.

## Resource model

Idle operation is event-driven and has no polling timer. Full-screen pixel
buffers exist only while an overlay is rendered; cancelling unmaps every surface
and released shared-memory slots are reused. Rendering is CPU/software based and
bounded by `max_cells`.

Run the project checks with:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```
