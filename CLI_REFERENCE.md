# CLI reference

All commands except `daemon` are short-lived clients of the running daemon.
The local `target/debug/mousr` build identifies as `mousr-dev` and uses a
separate IPC socket, so it can be tested alongside an installed release daemon.

```text
mousr [GLOBAL OPTIONS] daemon [--config PATH] [--seat NAME]
mousr [GLOBAL OPTIONS] reload
mousr [GLOBAL OPTIONS] cancel
mousr [GLOBAL OPTIONS] grid [OPTIONS]
mousr [GLOBAL OPTIONS] mouse
mousr [GLOBAL OPTIONS] click left|middle|right
mousr [GLOBAL OPTIONS] scroll up|down|left|right [--step AMOUNT]
```

Global options may appear before or after the subcommand:

| Option | Meaning |
| --- | --- |
| `--log-level LEVEL` | `error`, `warn`, `info`, `debug`, or `trace`; default: `info`. |
| `--log-file PATH` | Also append JSON-lines logs to `PATH`, creating parent directories. |

Stderr uses compact `DBG`, `INF`, `WRN`, `ERR`, and `PNC` prefixes with UTC
timestamps. Logging is process-local; use `mousr --log-level debug daemon` to
debug the daemon.

## `daemon`

Starts the long-running Wayland backend and Sway focus resolver.

| Option | Meaning |
| --- | --- |
| `--config PATH` | Use `PATH` instead of the default configuration file. |
| `--seat NAME` | Use a named Wayland/Sway seat; default: `seat0`. |

Only one daemon may run per `WAYLAND_DISPLAY`. Its mode-0600 IPC socket is
created under `XDG_RUNTIME_DIR`. If the Wayland keyboard or an output
disappears during suspend/resume or display reconfiguration, the daemon cancels
the active mode and releases held buttons; it rebinds the selected keyboard
when the seat becomes available again.

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
| `double-click` | Move, double-click the left button, and exit. |
| `scroll` | Move and enter persistent scroll mode. |
| `scroll-up`, `scroll-down` | Move and scroll vertically; exit only if `grid.exit_on_scroll` is enabled. |
| `scroll-left`, `scroll-right` | Move and scroll horizontally; exit only if `grid.exit_on_scroll` is enabled. |

Persistent scroll mode uses the configured mouse scroll keys (`u`, `e`, `y`,
and `o` by default). Exit with the mouse cancel key (`Escape` by default).

Default grid keys:

| Keys | Action |
| --- | --- |
| `a`–`z` | Enter a label. |
| `s`, `d`, `f` | Left, middle, or right click. |
| `c` | Double-click the left button. |
| `u`, `e`, `y`, `o` | Scroll up, down, left, or right. |
| `g` | Enter mouse mode. |
| `Space` | Move to the selected cell and exit. |
| `Enter` | Descend into the selected cell. |
| `Backspace` | Erase a character or return one level. |
| `Escape` | Cancel. |

## `mouse`

Starts continuous keyboard pointer control.

```text
mousr mouse [--until-release KEY]
```

`--until-release KEY` ends the session when that key is released, even while
movement or button keys are held. Exit stops movement and releases all held or
locked mouse buttons. The named key is reserved for session lifetime; its
presses and repeats do not trigger mouse actions. `Escape` and `mousr cancel`
still exit normally. Without this option, behavior is unchanged.

On Wayland, use a case-sensitive XKB keysym such as `F24`, `Control_L`, or `Alt_L`.
The daemon resolves it against the current seat's keymap and tracks the physical
keycodes, so changing modifiers while holding the key does not affect release
matching. If several physical keys produce the same keysym, releasing any of
them ends the session; use a uniquely mapped key for a dedicated activation key.
Unknown names and unmapped keys are rejected. A keymap change cancels a
release-controlled session. Repeating the same release-controlled mouse command
while it is active preserves movement and held buttons.

For a dedicated key mapped to `F24`, use one Sway press binding:

```sway
bindsym --no-repeat F24 exec mousr mouse --until-release F24
```

Remove the corresponding Sway `--release` binding: Mousr handles release during
the session. This command uses the existing daemon; it does not register a
global shortcut or synthesize a keypress.

Release-controlled Wayland sessions require an input-method v2 keyboard grab,
even with `general.preserve_focus = false`: Sway's ordinary focused-keyboard
path can omit releases whose presses were consumed by a shortcut. These
sessions preserve application focus and fail activation if the grab is
unavailable or already occupied by another input method.

Input handoff limitation: a release that happens before Mousr acquires keyboard
input cannot be recovered reliably from Wayland's input-method protocol. Sway's
focus snapshot also excludes shortcut-consumed keys. An extremely quick tap
can therefore leave mouse mode active; use `Escape` or `mousr cancel` to exit.
This option observes release events; it does not query global held-key state.

| Default keys | Action |
| --- | --- |
| `h`, `j`, `k`, `l` | Move left, down, up, or right. |
| `s`, `d`, `f` | Hold/release left, middle, or right button. |
| `c` | Double-click the left button. |
| `v` then `s`, `d`, or `f` | Lock a button; press `v` again to release it. |
| `u`, `e`, `y`, `o` | Scroll up, down, left, or right. |
| `Escape` | Release held buttons and exit. |

Button keys mirror key state. Button lock provides dragging without a keyboard
chord. Pressing `s` twice also performs two left clicks.

## One-shot commands

```text
mousr click left|middle|right
mousr scroll up|down|left|right [--step AMOUNT]
```

`click` presses and releases the chosen button. `scroll` emits the chosen wheel
direction. `--step` overrides its configured amount; 15 units equal one notch.
Both commands cancel the active session and release held mouse buttons before
performing their action. `click` uses the current pointer location.

Direct scrolling targets the current pointer by default. With
`[scroll] target = "focused-window"`, it moves the cursor to the focused window's
content center, scrolls there, and leaves the cursor there. Grid and mouse mode
scrolling are unaffected.

## Lifecycle commands

- `mousr reload` validates and applies the daemon configuration. A rejected
  configuration leaves the previous configuration in place but cancels the
  active session. Changing `general.preserve_focus` also cancels the session.
  If redrawing fails after application, the new configuration remains applied
  and the session is cancelled.
- `mousr cancel` exits the active mode and releases held mouse buttons.
- `mousr --help` prints command syntax.
- `mousr --version` prints the version.

Version output is `<name> v<version>(<short-commit>)`, for example
`mousr v0.1.1(1a2b3c4d)` or `mousr-dev v0.1.1(1a2b3c4d)`.
