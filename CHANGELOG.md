# Changelog

## 0.2.1 - 2026-11-23

### Added

- `mousr mouse --until-release KEY` ends mouse mode when `KEY` is released, for "hold key to activate". Movement stops and held buttons are released.
- `KEY` is an XKB keysym name, such as `F24`, resolved to a physical key on the active keymap. Changing modifiers does not affect it. Sessions using `--until-release` end if the keymap changes.

### Changed

- The IPC protocol version is now 2. Restart the daemon with the new binary after upgrading; mismatched versions report an error asking for this.

## 0.2.0 - 2026-11-05

### Added

- Opt-in `general.preserve_focus` for `grid`, `mouse`, and interactive `scroll` modes.
- `scroll.target = "focused-window"` moves the cursor into the focused window before direct scrolling. Pointer targeting remains the default.

### Changed

- A `Backend` input policy replaces `general.require_shortcut_inhibit`, which is deprecated. Existing configurations keep their behavior and log migration warnings.

### Fixed

- Duplicate wheel events in the Sway IPC fallback.

## 0.1.9 - 2026-10-31

### Added

- Stable AUR source package built from the release tag, alongside the binary and development packages.

### Changed

- A shared application runtime handles sessions, rendering, capture, animation, and motion.
- Display and compositor contracts have separate Wayland and Sway adapters.

### Fixed

- Screenshots refresh before magnified refinement. Capture failures, timeouts, and stale callbacks no longer leave stale magnification.
- Grids targeting a nonfocused output get keyboard focus.
- Command or rendering failures cancel the session and clean up input and overlays.
- AUR packages declare alternatives, and isolated AUR metadata generation is correct.
- Release tooling updates Yesb and pins Docker Hub helpers for AUR metadata and containerized APT/RPM publishing.

## 0.1.8 - 2026-09-10

Changelog was not maintained for this and earlier releases.
