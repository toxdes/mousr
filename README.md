# Mousr

Fast keyboard pointer control for Sway: labelled grids, `hjkl` movement, clicks,
dragging, and two-axis scrolling. A small daemon keeps Wayland state warm.

## Install

```sh
source ~/env/rust.sh
cargo build --release
install -Dm755 target/release/mousr ~/.local/bin/mousr
```

## Recommended Sway setup

```sway
# Start once with Sway.
exec mousr daemon

# Common workflows.
bindsym Mod1+space       exec mousr grid
bindsym Mod1+Shift+space exec mousr grid --scope all
bindsym Mod1+m           exec mousr mouse
bindsym Mod1+Shift+c     exec mousr cancel

# Select a grid cell and immediately perform an action.
bindsym Mod1+l exec mousr grid --action left
bindsym Mod1+r exec mousr grid --action right
bindsym Mod1+s exec mousr grid --action scroll
```

Grid defaults: type a label, then use `f/d/s` for left/middle/right click,
`u/e/y/o` to scroll, `g` for mouse mode, `Space` to move only, `Enter` to
descend, or `Escape` to cancel.

Mouse defaults: `hjkl` moves, `f/d/s` holds the three buttons, `u/e/y/o`
scrolls, and `Escape` safely releases held buttons.

Useful commands:

```sh
mousr grid --output HDMI-A-1
mousr grid --action mouse --max-depth 3
mousr click middle
mousr scroll left --step 30
mousr reload
```

See [CONFIGURATION.md](CONFIGURATION.md) for every option and
[mousr.example.toml](mousr.example.toml) for a complete file.

Only Sway is currently supported. Run checks with `cargo test --all-targets` and
`cargo clippy --all-targets -- -D warnings`.
