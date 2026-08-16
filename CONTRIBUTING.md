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

Install the debug variant alongside it:

```sh
cargo install --path . --bin mousr-dev --features dev-bin --debug
```

The commands install as `mousr` and `mousr-dev` and use independent IPC
sockets, so both daemons can exist during development.

## Test under Sway

For an installed debug build:

```sway
exec mousr-dev daemon
bindsym Mod1+space exec mousr-dev grid
bindsym Mod1+Shift+space exec mousr-dev mouse
```

Use `mousr-dev cancel` before replacing or restarting the development daemon.
Other compositors are not currently tested or supported. Most runtime work uses
Wayland protocols; Sway IPC is still required to identify the focused output.

## Checks

Run all checks before committing:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
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
