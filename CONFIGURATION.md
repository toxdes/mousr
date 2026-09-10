# Configuration

Mousr reads `$XDG_CONFIG_HOME/mousr/config.toml`, falling back to
`~/.config/mousr/config.toml`. Pass `mousr daemon --config PATH` to use another
file. Unknown keys and invalid values are errors. `mousr reload` applies a valid
replacement without restarting the daemon.

## General

| Option | Default | Meaning |
| --- | --- | --- |
| `general.scope` | `"focused"` | Initial grid scope: `focused` or `all`. |
| `general.require_shortcut_inhibit` | `true` | Refuse unsafe operation when Sway cannot inhibit compositor shortcuts. |

`focused` means the output containing Sway's focused workspace. CLI
`--scope` overrides the default; `--output NAME` targets one exact output.

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
`1.0`, Mousr captures the output before opening the grid and magnifies the
selected rectangle when descending. The minimum tile dimensions then apply to
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

CLI `mousr scroll DIRECTION --step AMOUNT` overrides the configured scroll step.
Amounts are converted to whole wheel notches, with a minimum of one.

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

When magnified refinement is active, Mousr captures the output before showing
the grid, draws that screenshot inside the lens, and dims the area outside it
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

Exact default values are in [mousr.example.toml](mousr.example.toml).
