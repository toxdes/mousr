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
