# CLI reference

All commands except `daemon` are short-lived clients of the running daemon.
The local `target/debug/mousr` build identifies as `mousr-dev` and uses a
separate IPC socket, so it can be tested alongside an installed release daemon.

```text
mousr daemon [--config PATH] [--seat NAME] [--log-level LEVEL]
mousr reload
mousr cancel
mousr grid [OPTIONS]
mousr mouse
mousr click left|middle|right
mousr scroll up|down|left|right [--step AMOUNT]
```

## `daemon`

Starts the long-running Wayland backend and Sway focus resolver.

| Option | Meaning |
| --- | --- |
| `--config PATH` | Use `PATH` instead of the default configuration file. |
| `--seat NAME` | Use a named Wayland/Sway seat; default: `seat0`. |
| `--log-level LEVEL` | Set the requested diagnostic level; default: `warn`. |

Only one daemon may run per `WAYLAND_DISPLAY`. Its mode-0600 IPC socket is
created under `XDG_RUNTIME_DIR`.

## `grid`

Displays a labelled grid and captures keyboard input.

| Option | Values | Meaning |
| --- | --- | --- |
| `--scope` | `focused`, `all` | Override the configured output scope. |
| `--output` | output name | Target one active Wayland output. |
| `--action` | action below | Run an action when a label is selected. |
| `--auto-descend` | — | Descend automatically when another level fits. |
| `--no-auto-descend` | — | Disable automatic descent. |
| `--max-depth` | `u8` integer | Override the configured recursion limit. |

`--scope` and `--output` are mutually exclusive. `focused` means the output
containing Sway's focused workspace. With `all`, labels remain unique across
outputs.

Actions:

| Action | Result after selection |
| --- | --- |
| `choose` | Wait for a grid-mode action key. This is the default. |
| `move` | Move to the cell centre and exit. |
| `mouse` | Move to the cell centre and enter mouse mode. |
| `left`, `middle`, `right` | Move, click that button, and exit. |
| `scroll` | Move and enter persistent scroll mode. |
| `scroll-up`, `scroll-down` | Move, scroll vertically, and exit. |
| `scroll-left`, `scroll-right` | Move, scroll horizontally, and exit. |

Default grid keys:

| Keys | Action |
| --- | --- |
| `a`–`z` | Enter a label. |
| `s`, `d`, `f` | Left, middle, or right click. |
| `u`, `e`, `y`, `o` | Scroll up, down, left, or right. |
| `g` | Enter mouse mode. |
| `Space` | Move to the selected cell and exit. |
| `Enter` | Descend into the selected cell. |
| `Backspace` | Erase a character or return one level. |
| `Escape` | Cancel. |

## `mouse`

Starts continuous keyboard pointer control.

| Default keys | Action |
| --- | --- |
| `h`, `j`, `k`, `l` | Move left, down, up, or right. |
| `s`, `d`, `f` | Hold/release left, middle, or right button. |
| `u`, `e`, `y`, `o` | Scroll up, down, left, or right. |
| `Escape` | Release held buttons and exit. |

Button keys mirror key state, so holding a button key while moving performs a
drag.

## One-shot commands

```text
mousr click left|middle|right
mousr scroll up|down|left|right [--step AMOUNT]
```

`click` presses and releases the chosen button. `scroll` emits the chosen wheel
direction. `--step` overrides its configured amount; 15 units equal one notch.

## Lifecycle commands

- `mousr reload` validates and atomically applies the daemon configuration.
- `mousr cancel` exits the active mode and releases held mouse buttons.
- `mousr --help` prints command syntax.
- `mousr --version` prints the version.

Version output is `<name> v<version>(<short-commit>)`, for example
`mousr v0.1.1(1a2b3c4d)` or `mousr-dev v0.1.1(1a2b3c4d)`.
