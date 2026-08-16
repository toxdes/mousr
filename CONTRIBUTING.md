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
Other compositors are not currently tested or supported. Most runtime work uses
Wayland protocols; Sway IPC is still required to identify the focused output.

## Checks

Run all checks before committing:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
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
