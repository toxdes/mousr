use super::*;
use crate::{backend::OutputInfo, cli::MouseButton, render::Frame};

struct TestDisplay {
    captures: Vec<(u64, String)>,
    timers: Vec<Timer>,
    frames: usize,
    focused: String,
    buttons: Vec<(MouseButton, KeyState)>,
    warps: Vec<(String, u32, u32)>,
    movement: Vec<(f64, f64)>,
    capture_available: bool,
    shortcut_available: bool,
    fail_capture: bool,
    fail_present: bool,
    scrolls: Vec<(Direction, f64)>,
    focused_scrolls: Vec<(Direction, f64)>,
    fail_focused_scroll: bool,
    input_policies: Vec<InputPolicy>,
    fail_input: bool,
}

impl DisplayBackend for TestDisplay {
    fn resolve_key(&self, name: &str) -> BackendResult<Vec<u32>> {
        match name {
            "F24" => Ok(vec![29, 194]),
            "F23" => Ok(Vec::new()),
            _ => Err("unknown key name".into()),
        }
    }
    fn outputs(&self) -> impl Iterator<Item = OutputInfo<'_>> {
        ["primary", "secondary"].into_iter().map(|name| OutputInfo {
            name,
            x: 0,
            y: 0,
            width: 640,
            height: 480,
            ready: true,
        })
    }
    fn focused_output(&self) -> BackendResult<String> {
        Ok("primary".into())
    }
    fn activate_input(&mut self, output: &str, policy: InputPolicy) -> BackendResult {
        self.input_policies.push(policy);
        if self.fail_input {
            return Err("input method unavailable".into());
        }
        self.focused = output.into();
        Ok(())
    }
    fn park(&mut self) -> BackendResult {
        self.focused.clear();
        Ok(())
    }
    fn present(&mut self, _: &str, frame: &Frame) -> BackendResult {
        if self.fail_present {
            return Err("presentation failed".into());
        }
        assert_eq!(
            frame.argb8888.len(),
            frame.width as usize * frame.height as usize * 4
        );
        self.frames += 1;
        Ok(())
    }
    fn can_capture(&self) -> bool {
        self.capture_available
    }
    fn can_inhibit_shortcuts(&self) -> bool {
        self.shortcut_available
    }
    fn capture(&mut self, id: u64, output: &str) -> BackendResult {
        if self.fail_capture {
            return Err("capture failed".into());
        }
        self.captures.push((id, output.into()));
        Ok(())
    }
    fn cancel_captures(&mut self) {}
    fn schedule(&mut self, timer: Timer, _: Duration) -> BackendResult {
        self.timers.push(timer);
        Ok(())
    }
    fn warp(&mut self, output: &str, x: u32, y: u32) -> BackendResult {
        self.warps.push((output.into(), x, y));
        Ok(())
    }
    fn move_cursor(&mut self, x: f64, y: f64) -> BackendResult {
        self.movement.push((x, y));
        Ok(())
    }
    fn button(&mut self, button: MouseButton, state: KeyState) -> BackendResult {
        self.buttons.push((button, state));
        Ok(())
    }
    fn scroll(&mut self, direction: Direction, amount: f64) -> BackendResult {
        self.scrolls.push((direction, amount));
        Ok(())
    }
    fn scroll_focused_window(&mut self, direction: Direction, amount: f64) -> BackendResult {
        assert!(
            self.focused.is_empty(),
            "session must be cancelled before targeting a window"
        );
        self.focused_scrolls.push((direction, amount));
        if self.fail_focused_scroll {
            return Err("no focused window".into());
        }
        Ok(())
    }
}

fn app() -> App<TestDisplay> {
    let mut config = Config::default();
    config.grid.refinement_zoom = 2.0;
    config.ui.lens_animation_ms = 0;
    App::new(
        TestDisplay {
            captures: Vec::new(),
            timers: Vec::new(),
            frames: 0,
            focused: String::new(),
            buttons: Vec::new(),
            warps: Vec::new(),
            movement: Vec::new(),
            capture_available: true,
            shortcut_available: true,
            fail_capture: false,
            fail_present: false,
            scrolls: Vec::new(),
            focused_scrolls: Vec::new(),
            fail_focused_scroll: false,
            input_policies: Vec::new(),
            fail_input: false,
        },
        config,
        None,
    )
    .unwrap()
}

fn press(app: &mut App<TestDisplay>, symbol: &str) {
    app.key(KeyInput {
        code: 1,
        symbol,
        state: KeyState::Pressed,
        repeated: false,
    });
}

fn select_first(app: &mut App<TestDisplay>) {
    let Session::Grid(grid) = &app.session else {
        panic!("grid is not active")
    };
    let label = grid.layout().tiles[0].label.clone();
    for character in label.chars() {
        press(app, &character.to_string());
    }
}

fn refine(app: &mut App<TestDisplay>) -> (u64, String) {
    select_first(app);
    press(app, "Return");
    app.display.captures.last().unwrap().clone()
}

fn screenshot(color: u8) -> Screenshot {
    Screenshot {
        width: 1,
        height: 1,
        logical_width: 640,
        logical_height: 480,
        rgba: vec![color, 0, 0, 255],
    }
}

#[test]
fn root_grid_does_not_capture_and_can_focus_a_nonfocused_output() {
    let mut app = app();
    app.command(Command::Grid(GridOptions {
        output: Some("secondary".into()),
        ..GridOptions::default()
    }))
    .unwrap();
    assert_eq!(app.display.focused, "secondary");
    assert_eq!(app.display.frames, 1);
    assert!(app.display.captures.is_empty());
}

#[test]
fn every_refinement_waits_for_a_fresh_frame() {
    let mut app = app();
    app.command(Command::Grid(GridOptions::default())).unwrap();
    let (id, output) = refine(&mut app);
    let frames = app.display.frames;
    app.redraw().unwrap();
    assert_eq!(app.display.frames, frames);
    app.captured(id, &output, Some(screenshot(42)));
    assert_eq!(app.display.frames, frames + 1);
    assert_eq!(app.screenshots[&output].rgba[0], 42);
    let (next_id, next_output) = refine(&mut app);
    assert_ne!(next_id, id);
    app.captured(next_id, &next_output, Some(screenshot(84)));
    assert_eq!(app.screenshots[&next_output].rgba[0], 84);
}

#[test]
fn canceled_capture_cannot_overwrite_a_new_session() {
    let mut app = app();
    app.command(Command::Grid(GridOptions::default())).unwrap();
    let (old_id, output) = refine(&mut app);
    press(&mut app, "Escape");
    assert!(matches!(app.session, Session::Idle));
    app.command(Command::Grid(GridOptions::default())).unwrap();
    let (new_id, _) = refine(&mut app);
    app.captured(old_id, &output, Some(screenshot(42)));
    app.timer(Timer::Capture(old_id));
    assert_eq!(app.capture.as_ref().unwrap().id, new_id);
    assert!(app.screenshots.is_empty());
}

#[test]
fn capture_failure_and_timeout_remove_stale_magnification() {
    for timeout in [false, true] {
        let mut app = app();
        app.command(Command::Grid(GridOptions::default())).unwrap();
        let (id, output) = refine(&mut app);
        app.captured(id, &output, Some(screenshot(42)));
        let (id, output) = refine(&mut app);
        if timeout {
            app.timer(Timer::Capture(id));
        } else {
            app.captured(id, &output, None);
        }
        assert!(app.capture.is_none());
        assert!(app.screenshots.is_empty());
        let Session::Grid(grid) = &app.session else {
            panic!("grid is not active")
        };
        assert!(grid.view().is_none());
        let expected = grid.layout().tiles[0].bounds.center();
        select_first(&mut app);
        press(&mut app, "s");
        assert_eq!(
            app.display.warps.last(),
            Some(&(output, expected.0, expected.1))
        );
    }
}

#[test]
fn synchronous_capture_errors_restore_the_grid() {
    let mut app = app();
    app.display.fail_capture = true;
    app.command(Command::Grid(GridOptions::default())).unwrap();
    select_first(&mut app);
    press(&mut app, "Return");
    assert!(app.capture.is_none());
    assert!(matches!(&app.session, Session::Grid(grid) if grid.view().is_none()));
}

#[test]
fn capture_capability_controls_magnification() {
    let mut app = app();
    app.display.capture_available = false;
    app.command(Command::Grid(GridOptions::default())).unwrap();
    select_first(&mut app);
    press(&mut app, "Return");
    assert!(app.display.captures.is_empty());
    assert!(
        matches!(&app.session, Session::Grid(grid) if grid.depth() == 2 && grid.view().is_none())
    );
}

#[test]
fn cancel_releases_a_locked_drag_and_stops_motion() {
    let mut app = app();
    app.command(Command::Mouse(Default::default())).unwrap();
    press(&mut app, "h");
    assert!(app.display.movement[0].0 < 0.0);
    press(&mut app, "v");
    press(&mut app, "s");
    app.cancel();
    assert_eq!(
        app.display.buttons,
        [
            (MouseButton::Left, KeyState::Pressed),
            (MouseButton::Left, KeyState::Released),
        ]
    );
    assert_eq!(app.timer(Timer::Motion), None);
    assert!(matches!(app.session, Session::Idle));
}

fn release_controlled_mouse() -> Command {
    Command::Mouse(crate::cli::MouseOptions {
        until_release: Some("F24".into()),
    })
}

#[test]
fn lifetime_release_after_movement_releases_drag_and_stops_motion() {
    let mut app = app();
    app.command(release_controlled_mouse()).unwrap();
    assert_eq!(
        app.display.input_policies.last(),
        Some(&InputPolicy {
            preserve_focus: Config::default().general.preserve_focus,
            capture_unpaired_releases: true,
        })
    );
    press(&mut app, "h");
    press(&mut app, "v");
    press(&mut app, "s");
    // The release symbol can differ from the activation symbol after a
    // layout/modifier change. The resolved physical key is authoritative.
    app.key(KeyInput {
        code: 29,
        symbol: "DifferentSymbol",
        state: KeyState::Released,
        repeated: false,
    });
    assert!(matches!(app.session, Session::Idle));
    assert!(app.display.focused.is_empty());
    assert_eq!(
        app.display.buttons.last(),
        Some(&(MouseButton::Left, KeyState::Released))
    );
    let movements = app.display.movement.len();
    assert_eq!(app.timer(Timer::Motion), None);
    assert_eq!(app.display.movement.len(), movements);
}

#[test]
fn unrelated_release_does_not_end_session_and_reactivation_preserves_drag() {
    let mut app = app();
    app.command(release_controlled_mouse()).unwrap();
    press(&mut app, "s");
    let activations = app.display.input_policies.len();
    app.command(release_controlled_mouse()).unwrap();
    assert_eq!(app.display.input_policies.len(), activations);
    assert_eq!(
        app.display.buttons,
        [(MouseButton::Left, KeyState::Pressed)]
    );
    app.key(KeyInput {
        code: 77,
        symbol: "F24",
        state: KeyState::Released,
        repeated: false,
    });
    assert!(matches!(app.session, Session::Mouse(_)));
    app.key(KeyInput {
        code: 29,
        symbol: "Escape",
        state: KeyState::Pressed,
        repeated: true,
    });
    assert!(matches!(app.session, Session::Mouse(_)));
    press(&mut app, "Escape");
    assert!(matches!(app.session, Session::Idle));
}

#[test]
fn replacement_session_does_not_inherit_release_condition() {
    let mut app = app();
    app.command(release_controlled_mouse()).unwrap();
    app.command(Command::Mouse(Default::default())).unwrap();
    app.key(KeyInput {
        code: 29,
        symbol: "F24",
        state: KeyState::Released,
        repeated: false,
    });
    assert!(matches!(app.session, Session::Mouse(_)));
    assert!(matches!(app.lifetime, SessionLifetime::UntilCancelled));
}

#[test]
fn invalid_or_unmapped_lifetime_key_never_acquires_input() {
    for key in ["Unknown", "F23"] {
        let mut app = app();
        assert!(
            app.command(Command::Mouse(crate::cli::MouseOptions {
                until_release: Some(key.into()),
            }))
            .is_err()
        );
        assert!(matches!(app.session, Session::Idle));
        assert!(app.display.input_policies.is_empty());
    }
}

#[test]
fn keymap_changes_end_only_release_controlled_sessions() {
    let mut app = app();
    app.command(release_controlled_mouse()).unwrap();
    press(&mut app, "s");
    app.keyboard_map_changed();
    assert!(matches!(app.session, Session::Idle));
    assert_eq!(
        app.display.buttons.last(),
        Some(&(MouseButton::Left, KeyState::Released))
    );
    app.command(Command::Mouse(Default::default())).unwrap();
    app.keyboard_map_changed();
    assert!(matches!(app.session, Session::Mouse(_)));
}

#[test]
fn returning_focus_invalidates_the_old_focus_loss_timer() {
    let mut app = app();
    app.command(Command::Mouse(Default::default())).unwrap();
    app.keyboard_entered();
    app.keyboard_left();
    let timer = *app.display.timers.last().unwrap();
    app.keyboard_entered();
    app.timer(timer);
    assert!(matches!(app.session, Session::Mouse(_)));
    app.keyboard_left();
    app.timer(*app.display.timers.last().unwrap());
    assert!(matches!(app.session, Session::Idle));
}

#[test]
fn presentation_failure_cancels_activation() {
    let mut app = app();
    app.display.fail_present = true;
    assert!(app.command(Command::Grid(GridOptions::default())).is_err());
    assert!(matches!(app.session, Session::Idle));
    assert!(app.display.focused.is_empty());
}

#[test]
fn a_previous_sessions_focus_timer_cannot_cancel_new_activation() {
    let mut app = app();
    app.command(Command::Mouse(Default::default())).unwrap();
    app.keyboard_left();
    let timer = *app.display.timers.last().unwrap();
    app.command(Command::Grid(GridOptions::default())).unwrap();
    app.timer(timer);
    assert!(matches!(app.session, Session::Grid(_)));
}

#[test]
fn required_shortcut_inhibition_is_checked_without_a_native_adapter() {
    let mut display = app().display;
    display.shortcut_available = false;
    assert!(App::new(display, Config::default(), None).is_err());
}

#[test]
fn losing_shortcut_inhibition_releases_drag_and_obeys_configuration() {
    for required in [false, true] {
        let mut app = app();
        app.config.general =
            toml::from_str(&format!("require_shortcut_inhibit = {required}")).unwrap();
        app.command(Command::Mouse(Default::default())).unwrap();
        press(&mut app, "v");
        press(&mut app, "s");
        app.shortcuts_lost();
        assert_eq!(
            app.display.buttons.last(),
            Some(&(MouseButton::Left, KeyState::Released))
        );
        assert_eq!(matches!(app.session, Session::Idle), required);
    }
}

#[test]
fn direct_scroll_defaults_to_the_pointer_and_respects_step_overrides() {
    let mut app = app();
    app.config.scroll.horizontal_step = 30.0;
    app.command(Command::Scroll {
        direction: Direction::Left,
        step: None,
    })
    .unwrap();
    app.command(Command::Scroll {
        direction: Direction::Down,
        step: Some(45.0),
    })
    .unwrap();
    assert_eq!(
        app.display.scrolls,
        [(Direction::Left, 30.0), (Direction::Down, 45.0)]
    );
    assert!(app.display.focused_scrolls.is_empty());
    assert!(app.display.warps.is_empty());
}

#[test]
fn focused_scroll_is_opt_in_and_cancels_input_before_targeting_the_window() {
    let mut app = app();
    app.config.scroll.target = ScrollTarget::FocusedWindow;
    app.config.scroll.horizontal_step = 30.0;
    app.command(Command::Mouse(Default::default())).unwrap();
    press(&mut app, "v");
    press(&mut app, "s");
    app.command(Command::Scroll {
        direction: Direction::Right,
        step: None,
    })
    .unwrap();
    app.command(Command::Scroll {
        direction: Direction::Up,
        step: Some(22.5),
    })
    .unwrap();
    assert_eq!(
        app.display.focused_scrolls,
        [(Direction::Right, 30.0), (Direction::Up, 22.5)]
    );
    assert!(app.display.scrolls.is_empty());
    assert!(matches!(app.session, Session::Idle));
    assert_eq!(
        app.display.buttons.last(),
        Some(&(MouseButton::Left, KeyState::Released))
    );
}

#[test]
fn focus_preservation_is_opt_in_and_applies_to_grid_and_mouse() {
    for preserve_focus in [false, true] {
        let mut app = app();
        app.config.general.preserve_focus = preserve_focus;
        app.command(Command::Grid(GridOptions::default())).unwrap();
        app.command(Command::Mouse(Default::default())).unwrap();
        assert_eq!(
            app.display.input_policies,
            [InputPolicy {
                preserve_focus,
                capture_unpaired_releases: false
            }; 2]
        );
    }
}

#[test]
fn unavailable_input_method_fails_activation_without_falling_back_to_focus() {
    let mut app = app();
    app.config.general.preserve_focus = true;
    app.display.fail_input = true;
    let error = app
        .command(Command::Grid(GridOptions::default()))
        .unwrap_err();
    assert!(error.to_string().contains("input method unavailable"));
    assert!(matches!(app.session, Session::Idle));
    assert_eq!(app.display.frames, 0);
    assert_eq!(
        app.display.input_policies,
        [InputPolicy {
            preserve_focus: true,
            capture_unpaired_releases: false,
        }]
    );
}

#[test]
fn input_method_mode_does_not_require_overlay_shortcuts_or_keyboard_focus() {
    let display = app().display;
    let mut config = Config::default();
    config.general.preserve_focus = true;
    let mut display = display;
    display.shortcut_available = false;
    let mut app = App::new(display, config, None).unwrap();
    app.command(Command::Mouse(Default::default())).unwrap();
    press(&mut app, "h");
    press(&mut app, "v");
    press(&mut app, "s");
    app.keyboard_left();
    app.shortcuts_lost();
    assert!(matches!(app.session, Session::Mouse(_)));
    assert!(
        !app.display
            .timers
            .iter()
            .any(|timer| matches!(timer, Timer::Focus(_)))
    );
    assert!(app.mouse_is_moving());
    app.cancel();
    assert_eq!(
        app.display.buttons.last(),
        Some(&(MouseButton::Left, KeyState::Released))
    );
}

#[test]
fn focused_scroll_failure_does_not_fall_back_to_the_pointer() {
    let mut app = app();
    app.config.scroll.target = ScrollTarget::FocusedWindow;
    app.display.fail_focused_scroll = true;
    assert!(
        app.command(Command::Scroll {
            direction: Direction::Down,
            step: None
        })
        .is_err()
    );
    assert!(app.display.scrolls.is_empty());
}

#[test]
fn focused_window_setting_does_not_retarget_grid_or_mouse_scrolling() {
    let mut app = app();
    app.config.scroll.target = ScrollTarget::FocusedWindow;
    app.command(Command::Grid(GridOptions::default())).unwrap();
    select_first(&mut app);
    press(&mut app, "u");
    assert_eq!(app.display.warps.len(), 1);
    app.command(Command::Mouse(Default::default())).unwrap();
    press(&mut app, "e");
    assert_eq!(
        app.display.scrolls,
        [(Direction::Up, 15.0), (Direction::Down, 15.0)]
    );
    assert!(app.display.focused_scrolls.is_empty());
}

#[test]
fn reloading_input_policy_cancels_the_old_session_and_releases_drag() {
    let mut app = app();
    app.command(Command::Mouse(Default::default())).unwrap();
    press(&mut app, "v");
    press(&mut app, "s");
    let path = std::env::temp_dir().join(format!(
        "mousr-policy-{}-{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, "[general]\npreserve_focus = true\n").unwrap();
    app.config_path = Some(path.clone());
    let result = app.command(Command::Reload);
    std::fs::remove_file(path).unwrap();
    result.unwrap();
    assert!(matches!(app.session, Session::Idle));
    assert_eq!(
        app.display.buttons.last(),
        Some(&(MouseButton::Left, KeyState::Released))
    );
    app.command(Command::Mouse(Default::default())).unwrap();
    assert_eq!(
        app.display.input_policies.last(),
        Some(&InputPolicy {
            preserve_focus: true,
            capture_unpaired_releases: false,
        })
    );
}
