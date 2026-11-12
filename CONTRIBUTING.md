# Building and contributing

## Requirements

- A current stable Rust toolchain
- Sway and a Wayland session for integration testing
- System development files required by `libxkbcommon`

## Build from source

Debug builds identify as `mousr-dev` and use a separate daemon socket. Release
builds identify as `mousr`.

```sh
cargo build
cargo build --release
```

The binaries are written to `target/debug/mousr` and
`target/release/mousr` respectively.

## Install from source

Install the optimized release:

```sh
cargo install --path . --bin mousr
```

Debug builds are run from `target/debug` rather than installed. They use a
separate IPC socket, so a development daemon can coexist with an installed
release daemon.

## Test under Sway

Build first, then replace the path below with the absolute path to the checkout:

```sh
cargo build
```

```sway
set $mousr_dev /absolute/path/to/mousr/target/debug/mousr
exec $mousr_dev daemon
bindsym Mod1+space exec $mousr_dev grid
bindsym Mod1+Shift+space exec $mousr_dev mouse
```

Use `./target/debug/mousr cancel` before replacing or restarting the development
daemon.
Other compositors are not currently tested or supported. Wayland handles
surfaces, input, capture, and usually pointer events; Sway IPC handles focused
output, cross-output warps, and pointer fallback commands.

`App<D>` in `src/app.rs` owns commands, sessions, rendering, capture policy,
animation, and motion. `DisplayBackend` in `src/backend.rs` supplies output
geometry, focus, presentation, asynchronous capture, timers, and pointer events.
`activate_input` takes a backend-neutral focus-preservation policy. Adapters
must fail when that policy cannot be honored, rather than silently taking focus.
The Wayland adapter implements the opt-in policy through a session-scoped
input-method v2 grab in `src/input_method.rs`; default sessions use layer-shell
keyboard focus. Protocol types stay inside the Wayland implementation.
Adapters translate native input into `KeyInput`, deliver capture results with
their request IDs, and dispatch scheduled timers back to `App`. Cancel the
session before rebuilding output surfaces. The renderer produces packed,
premultiplied BGRA frames; pointer targets use output-local logical pixels.

`CompositorControl` in `src/compositor.rs` isolates compositor commands;
`src/sway.rs` implements it. Selection and composition live in `src/backend.rs`,
which injects compositor control into the display adapter. New adapters reuse
the application runtime and do not modify existing adapters.

Default Cargo features enable the current `wayland` and `sway` adapters.
`cargo test --no-default-features` exercises the shared runtime without either
adapter or their native dependencies. Other platforms remain unimplemented;
the local IPC transport currently requires Unix sockets.

## Release-controlled mouse sessions

`MouseOptions::until_release` is carried over IPC and becomes shared session
lifetime state in `App`, separate from mouse movement and button bindings.
`DisplayBackend::resolve_key` translates a backend's named key into stable
identifiers in the same namespace as `KeyInput::code`. Unsupported backends
return an error. Wayland resolves XKB keysyms across the seat keymap's layouts
and levels and cancels release-controlled sessions when that map changes.
Release is processed before mode actions and uses the normal cancellation path.
`InputPolicy::capture_unpaired_releases` requires release events even for keys
whose presses were consumed by the activating shortcut. The Wayland backend
uses its input-method v2 grab to satisfy this independently of `preserve_focus`.
Other backends must honor this policy or fail activation.

`Command::Mouse` now takes `MouseOptions`; Rust callers should use
`Command::Mouse(MouseOptions::default())` for ordinary mouse mode. The changed
command payload uses IPC protocol version 2. Restart the daemon with the same
binary as the CLI when testing or upgrading; version mismatches are rejected
before command decoding.

## Rust API migration for v0.2.0

Downstream users of the library need these changes from v0.1.9:

- Replace `DisplayBackend::focus(output)` implementations and calls with
  `activate_input(output, InputPolicy)`. Honor `preserve_focus` or return an
  error when it cannot be supported. `park` must release session input as well
  as hide overlays.
- Implement the new required `CompositorControl::warp_to_focused_window(seat)`
  method. `DisplayBackend::scroll_focused_window` has a default unsupported
  error; override it to support focused-window scrolling. It must target the
  window before scrolling and leave the cursor there.
- Replace Rust access to `General::require_shortcut_inhibit` with
  `requires_shortcut_inhibit()` and use `General::preserve_focus` to select the
  input policy. `General` now contains private compatibility state; construct it
  through `General::default()` and update public fields, or deserialize TOML.
  Legacy TOML keys remain supported as described in [CONFIGURATION.md](CONFIGURATION.md).
- Include `Scroll::target` in struct initializers, using `ScrollTarget::Pointer`
  for the previous behavior or `ScrollTarget::FocusedWindow` for cursor
  relocation. `Scroll::default()` retains pointer targeting.

## Checks

Run all checks before committing:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --no-default-features
cargo build --release
```

## Contribution notes

Keep changes focused and easy to review. Separate unrelated behavior into
different commits, run the checks above, and include tests when changing
parsing, geometry, rendering, or mode behavior.

Prefer straightforward, idiomatic Rust over new abstractions that are only used
once. Comments are useful when they explain a surprising Wayland or Sway
constraint; ordinary control flow should speak for itself.

Input and Wayland changes deserve extra care. Every grabbed key, held mouse
button, surface, and shared-memory buffer needs a clear cleanup path, including
cancel and error cases. Changes should not introduce idle polling or keep
full-screen buffers alive after a mode exits.
