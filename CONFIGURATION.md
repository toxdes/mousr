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
| `grid.min_tile_width` | `24` | Minimum logical cell width. |
| `grid.min_tile_height` | `24` | Minimum logical cell height. |
| `grid.max_label_length` | `2` | Maximum base-26 label length, from 1 through 4. |
| `grid.max_depth` | `2` | Maximum recursive selection depth. |
| `grid.max_cells` | `4096` | Hard cell-count and rendering-work limit. |
| `grid.auto_descend` | `false` | Descend immediately after selecting a cell when possible. |
| `grid.unmatched` | `"dim"` | Render unmatched cells as `keep`, `dim`, or `hide`. |
| `grid.unmatched_opacity` | `0.18` | Opacity used by `dim`, from 0 through 1. |

Mousr derives the grid from output size and minimum tile size, then coarsens it
to fit label capacity and `max_cells`. Labels are fixed-width: `a`–`z`, then
`aa`–`zz`, and so on.

## Motion and scrolling

| Option | Default | Meaning |
| --- | --- | --- |
| `motion.initial_speed` | `240.0` | Initial movement speed in logical pixels per second. |
| `motion.acceleration` | `1400.0` | Speed increase per second while moving. |
| `motion.max_speed` | `1800.0` | Movement speed cap. |
| `motion.tick_hz` | `120` | Pointer update frequency while moving; accepted range is 30–1000. |
| `scroll.vertical_step` | `15.0` | Vertical wheel amount; 15 is one notch. |
| `scroll.horizontal_step` | `15.0` | Horizontal wheel amount; 15 is one notch. |

CLI `mousr scroll DIRECTION --step AMOUNT` overrides the configured scroll step.
Amounts are converted to whole wheel notches, with a minimum of one.

Relative mouse movement uses the compositor's global logical coordinate space,
so it crosses outputs according to the layout configured in Sway or `wlr-randr`.

## Bindings

All values are XKB keysym names or single UTF-8 characters.

| Grid option | Default | Grid option | Default |
| --- | --- | --- | --- |
| `left_click` | `"f"` | `middle_click` | `"d"` |
| `right_click` | `"s"` | `enter_mouse` | `"g"` |
| `scroll_up` | `"u"` | `scroll_down` | `"e"` |
| `scroll_left` | `"y"` | `scroll_right` | `"o"` |
| `move_only` | `"space"` | `descend` | `"Return"` |
| `back` | `"BackSpace"` | `cancel` | `"Escape"` |

These keys live under `[bindings.grid]`.

| Mouse option | Default | Mouse option | Default |
| --- | --- | --- | --- |
| `left` | `"h"` | `down` | `"j"` |
| `up` | `"k"` | `right` | `"l"` |
| `left_button` | `"f"` | `middle_button` | `"d"` |
| `right_button` | `"s"` | `cancel` | `"Escape"` |
| `scroll_up` | `"u"` | `scroll_down` | `"e"` |
| `scroll_left` | `"y"` | `scroll_right` | `"o"` |

These keys live under `[bindings.mouse]`. Button keys mirror physical state, so
holding one while moving performs drag-and-drop. Cancelling releases all held
buttons.

## UI

Colors accept `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA`. Short digits expand,
so `#fff` is opaque white and `#fff0` is transparent white. The alpha component
controls opacity. Overlay, cell, label, matched-prefix, selection, badge,
border, and target colors are independently configurable.

| Option | Meaning |
| --- | --- |
| `font_path`, `font_size` | Optional TTF/OTF path and label size. An embedded font is the fallback. |
| `overlay_background`, `cell_background` | Full overlay and cell fill colors. |
| `grid_border`, `grid_border_width` | Grid line color and width. |
| `label_background`, `label_foreground` | Normal label colors. |
| `matched_background`, `matched_foreground` | Colors for the matched label prefix. |
| `selected_background`, `selected_border`, `selected_border_width` | Selected-cell styling. |
| `badge_background`, `badge_foreground`, `badge_border`, `badge_border_width` | Mouse/scroll mode badge styling. |
| `target_ring`, `target_ring_width`, `target_ring_radius` | Selected pointer-target ring styling. |
| `show_badge`, `show_target_ring` | Toggle mode feedback elements. |
| `show_action_hints` | Show configured actions after grid selection and in mouse mode. |

Exact default values are in [mousr.example.toml](mousr.example.toml).
