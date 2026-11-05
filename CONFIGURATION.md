# Configuration

Mousr reads `$XDG_CONFIG_HOME/mousr/config.toml`, falling back to
`~/.config/mousr/config.toml`. Pass `mousr daemon --config PATH` to use another
file. Unknown keys and invalid values are errors. `mousr reload` applies a valid
replacement without restarting the daemon.

## General

| Option | Default | Meaning |
| --- | --- | --- |
| `general.scope` | `"focused"` | Initial grid scope: `focused` or `all`. |
| `general.preserve_focus` | `false` | Keep application keyboard focus while Mousr is active, using Wayland input-method v2. |

`focused` means the output containing Sway's focused workspace. CLI
`--scope` overrides the default; `--output NAME` targets one exact output.

With `preserve_focus = false` (the default), Mousr focuses its overlay and requires
compositor shortcut inhibition. With `preserve_focus = true`, grid, mouse, and
interactive scroll sessions keep the overlays unfocusable. This can preserve menus and popovers that close
when their application loses keyboard focus. Mousr acquires the input-method
slot only during a session and releases it on exit. The compositor must support
`zwp_input_method_manager_v2`, and the selected seat must not already have an input
method such as Fcitx5 using that protocol. Activation fails if capture is
unavailable; Mousr never falls back to taking focus.

With focus preservation enabled, compositor shortcuts
remain active and can intercept keys before Mousr receives them. Normal keyboard
input is captured only while a session is active. Pointer movement, clicks, and
`focus_follows_mouse` can still change the focused window, and scrolling still
targets the surface under the pointer. Focus preservation does not remove the
existing Sway dependency for output selection, cursor warps, or pointer fallbacks.

Changing `preserve_focus` with `mousr reload` cancels the current session
and applies the new input policy to the next activation.

### Existing configurations

`general.require_shortcut_inhibit` is deprecated but remains accepted with a
warning. Existing `true` values retain the default requirement for inhibition;
existing `false` values retain permissive operation, which requests inhibition
but allows the overlay to take focus even if inhibition is unavailable.

Remove a legacy `true` entry to keep the same behavior. A legacy `false` entry
has no exact replacement: remove it to require inhibition, or replace it with
`preserve_focus = true` to select a different input path that preserves focus.
Configurations explicitly containing both keys are rejected, including when
either value is `false`; remove `require_shortcut_inhibit` before setting
`preserve_focus`. Mousr does not rewrite existing configuration files.

## Grid

| Option | Default | Meaning |
| --- | --- | --- |
| `grid.root_min_tile_width` | `96` | Minimum logical cell width for the first grid level. |
| `grid.root_min_tile_height` | `54` | Minimum logical cell height for the first grid level. |
| `grid.min_tile_width` | `16` | Minimum displayed recursive cell width; magnified source cells may be smaller. |
| `grid.min_tile_height` | `16` | Minimum displayed recursive cell height; magnified source cells may be smaller. |
| `grid.refinement_zoom` | `1.0` | Magnification applied to the selected rectangle at each recursive level; `1.0` disables the lens. |
| `grid.max_label_length` | `3` | Maximum base-26 label length, from 1 through 4. |
| `grid.max_depth` | `4` | Maximum recursive selection depth. |
| `grid.max_cells` | `4096` | Hard cell-count and rendering-work limit. |
| `grid.auto_descend` | `false` | Descend immediately after selecting a cell when possible. |
| `grid.exit_on_scroll` | `false` | Exit grid mode after a directional scroll action. |
| `grid.unmatched` | `"dim"` | Render unmatched cells as `keep`, `dim`, or `hide`. |
| `grid.unmatched_opacity` | `0.18` | Opacity used by `dim`, from 0 through 1. |

Mousr derives the first grid level from output size and the root minimum tile
size. Each refinement halves the width and height targets independently until
the finest `min_tile_*` values are reached, then coarsens the level if needed
to fit label capacity and `max_cells`. With `refinement_zoom` greater than
`1.0`, Mousr opens the root grid without a screenshot, then captures a fresh
output image for each magnified descent. The minimum tile dimensions then apply to
the magnified display space, so smaller source-coordinate cells remain easy to
read and select. Values from `1.0` through `32.0` are accepted. Labels are
fixed-width: `a`–`z`, then `aa`–`zz`, and so on.

## Motion and scrolling

| Option | Default | Meaning |
| --- | --- | --- |
| `motion.initial_speed` | `60.0` | Initial movement speed in logical pixels per second. |
| `motion.acceleration` | `1400.0` | Controls how quickly movement reaches maximum speed; `0` disables the ramp. |
| `motion.max_speed` | `1800.0` | Movement speed cap. |
| `motion.tick_hz` | `120` | Pointer update frequency while moving; accepted range is 30–1000. |
| `motion.curve` | `"ease-in-out"` | Velocity ramp: `linear`, `ease-in`, `ease-out`, or `ease-in-out`. |
| `scroll.vertical_step` | `15.0` | Vertical wheel amount; 15 is one notch. |
| `scroll.horizontal_step` | `15.0` | Horizontal wheel amount; 15 is one notch. |
| `scroll.target` | `"pointer"` | Target for direct `mousr scroll` commands: `pointer` or `focused-window`. |

CLI `mousr scroll DIRECTION --step AMOUNT` overrides the configured scroll step.
The Wayland virtual-pointer path sends the requested continuous amount together
with a discrete notch count rounded to the nearest whole notch, with a minimum
of one. The Sway IPC fallback emits only whole notches using that same rounding.

By default, direct scroll commands leave the cursor in place and scroll the
surface under it. Set `target = "focused-window"` under `[scroll]` to move the
cursor to the center of the configured seat's focused window content before
scrolling. The cursor stays there afterward; it is not restored. On Sway, the
window is found through IPC. If no usable focused window is available, the
command fails without scrolling at the old pointer location.

This setting does not change scrolling inside grid, mouse, or interactive scroll
modes: those use the selected location or current pointer. A window's center
may land in a different scrollable pane than intended; use grid selection for
precise targeting. Moving the pointer can affect hover state and interacts with
`focus_follows_mouse` as normal.

Relative mouse movement uses the compositor's global logical coordinate space,
so it crosses outputs according to the layout configured in Sway or `wlr-randr`.
Fractional motion is preserved, allowing precise movement below one logical
pixel per update.

## Bindings

All values are XKB keysym names or single UTF-8 characters.

| Grid option | Default | Grid option | Default |
| --- | --- | --- | --- |
| `left_click` | `"s"` | `middle_click` | `"d"` |
| `right_click` | `"f"` | `enter_mouse` | `"g"` |
| `double_click` | `"c"` | `scroll_up` | `"u"` |
| `scroll_down` | `"e"` | `scroll_left` | `"y"` |
| `scroll_right` | `"o"` | `move_only` | `"space"` |
| `descend` | `"Return"` | `back` | `"BackSpace"` |
| `cancel` | `"Escape"` | | |

These keys live under `[bindings.grid]`.

| Mouse option | Default | Mouse option | Default |
| --- | --- | --- | --- |
| `left` | `"h"` | `down` | `"j"` |
| `up` | `"k"` | `right` | `"l"` |
| `left_button` | `"s"` | `middle_button` | `"d"` |
| `right_button` | `"f"` | `button_lock` | `"v"` |
| `double_click` | `"c"` | `scroll_up` | `"u"` |
| `scroll_down` | `"e"` | `scroll_left` | `"y"` |
| `scroll_right` | `"o"` | `cancel` | `"Escape"` |

These keys live under `[bindings.mouse]`. Button keys mirror physical state, so
holding one while moving performs drag-and-drop. Cancelling releases all held
buttons. Press `c` (or the configured `double_click` key) for an explicit left
double-click; pressing the left button key twice also works. Press
`button_lock`, then a button key, to lock that button without a chord; press
`button_lock` again to release it. Mouse bindings must be non-empty and unique.

## UI

Colors accept `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA`. Short digits expand,
so `#fff` is opaque white and `#fff0` is transparent white. The alpha component
controls opacity. Overlay, cell, label, matched-prefix, selection, badge,
border, and target colors are independently configurable.

When magnified refinement is active, Mousr temporarily hides the overlays and
captures a fresh output image on each descent, draws that screenshot inside the
lens, and dims the area outside it
with `lens_scrim_opacity`. The grid's tile fill is painted over the screenshot;
`lens_cell_opacity` multiplies the tile fill's own alpha, so lowering it makes
the preview clearer without changing labels or grid lines. If screencopy is not
available or capture fails, refinement continues without the screenshot lens.

| Option | Meaning |
| --- | --- |
| `font_path`, `font_size` | Optional TTF/OTF path and label size. An embedded font is the fallback. |
| `overlay_background`, `cell_background` | Full overlay and cell fill colors. |
| `lens_scrim_opacity` | Opacity used to dim everything outside a magnified refinement lens. |
| `lens_border`, `lens_border_width` | Magnified refinement lens outline. |
| `lens_animation_ms` | Grid-level transition duration in milliseconds; `0` disables animation. |
| `lens_cell_opacity` | Multiplier applied to `cell_background` opacity inside the lens. |
| `grid_border`, `grid_border_width` | Grid line color and width. |
| `label_background`, `label_foreground` | Normal label colors. |
| `matched_background`, `matched_foreground` | Colors for the matched label prefix. |
| `selected_background`, `selected_border`, `selected_border_width` | Selected-cell styling. |
| `badge_background`, `badge_foreground`, `badge_border`, `badge_border_width` | Mouse/scroll mode badge styling. |
| `target_ring`, `target_ring_width`, `target_ring_radius` | Selected pointer-target ring styling. |
| `show_badge`, `show_target_ring` | Toggle mode feedback elements. |
| `show_action_hints` | Show configured actions after grid selection and in mouse mode. |

Both action tables use the badge background, foreground, and border colors.

Descending into or backing out of a magnified level uses a short ease-out
transition controlled by `lens_animation_ms`. Keyboard input interrupts the
transition immediately; the renderer also limits the number of animation
frames on very large outputs. Set the duration to `0` to disable it.

If the Wayland keyboard or output disappears during suspend/resume or display
reconfiguration, Mousr cancels the active mode, releases held buttons, parks
its overlays, and rebinds the keyboard when the selected seat returns.

The [example configuration](mousr.example.toml) shows all settings with their
default values except `grid.refinement_zoom`, which is set to `4.0` to demonstrate
magnification; its built-in default is `1.0`.
