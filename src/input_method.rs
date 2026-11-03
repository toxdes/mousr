//! Session-scoped Wayland input-method grabs, independent of overlay focus.

use std::{
    cell::{Cell, RefCell},
    fs::File,
    os::unix::fs::FileExt,
    rc::Rc,
    time::Duration,
};

use calloop::{
    LoopHandle, RegistrationToken,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_keyboard, wl_registry, wl_seat},
};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2::{self, ZwpInputMethodKeyboardGrabV2},
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use xkbcommon::xkb;

use crate::{backend::BackendResult, mode::KeyState, wayland::input_symbol};

pub(crate) enum InputEvent {
    Key {
        code: u32,
        symbol: String,
        state: KeyState,
        repeated: bool,
    },
    Lost(String),
}

/// Owns the connection as well as the grab: dropping a session frees the IME slot.
pub(crate) struct InputMethod<D: 'static> {
    connection: Connection,
    method: ZwpInputMethodV2,
    grab: Option<ZwpInputMethodKeyboardGrabV2>,
    handle: LoopHandle<'static, D>,
    source: Option<RegistrationToken>,
    repeat_source: Rc<Cell<Option<RegistrationToken>>>,
}

impl<D: 'static> InputMethod<D> {
    pub(crate) fn acquire(
        handle: LoopHandle<'static, D>,
        seat_name: &str,
        callback: impl FnMut(InputEvent, &mut D) + 'static,
    ) -> BackendResult<Self> {
        // A separate connection lets us confirm availability before reporting
        // activation success, without re-entering the overlay's event dispatch.
        let connection = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init::<GrabState>(&connection)?;
        let qh = queue.handle();
        let manager: ZwpInputMethodManagerV2 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "preserve_focus requires zwp_input_method_manager_v2")?;
        let mut state = GrabState::default();
        globals.contents().with_list(|list| {
            for global in list.iter().filter(|global| global.interface == "wl_seat") {
                let seat = globals
                    .registry()
                    .bind(global.name, global.version.min(7), &qh, ());
                state.seats.push((seat, String::new(), false));
            }
        });
        queue.roundtrip(&mut state)?;
        let seat = state
            .seats
            .iter()
            .find(|(_, name, keyboard)| name == seat_name && *keyboard)
            .map(|(seat, _, _)| seat.clone())
            .ok_or_else(|| format!("no keyboard-capable Wayland seat named {seat_name:?}"))?;
        state.selected_seat = Some(seat.clone());
        let method = manager.get_input_method(&seat, &qh, ());
        let mut input = Self {
            connection,
            method,
            grab: None,
            handle: handle.clone(),
            source: None,
            repeat_source: Rc::new(Cell::new(None)),
        };
        // An occupied seat returns `unavailable`. Never fall back to overlay focus.
        queue.roundtrip(&mut state)?;
        if let Some(error) = state.failure.take() {
            return Err(error.into());
        }
        input.grab = Some(input.method.grab_keyboard(&qh, ()));
        manager.destroy();
        let state = Rc::new(RefCell::new(state));
        let callback = Rc::new(RefCell::new(callback));
        let repeat_source = input.repeat_source.clone();
        input.source = Some(handle.clone().insert_source(
            WaylandSource::new(input.connection.clone(), queue),
            move |_, queue, data| {
                let (count, events, failure, revision, repeat) = {
                    let mut state = state.borrow_mut();
                    let previous = state.repeat_revision;
                    let count = queue.dispatch_pending(&mut *state)?;
                    let changed = previous != state.repeat_revision;
                    (
                        count,
                        std::mem::take(&mut state.events),
                        state.failure.take(),
                        state.repeat_revision,
                        changed,
                    )
                };
                // Invalidate repeats before a callback can end the session.
                if repeat && let Some(token) = repeat_source.take() {
                    handle.remove(token);
                }
                if let Some(error) = failure {
                    callback.borrow_mut()(InputEvent::Lost(error), data);
                    return Ok(count);
                }
                if repeat {
                    let delay = state.borrow().repeat_delay();
                    if let Some(delay) = delay {
                        let state = state.clone();
                        let repeat_callback = callback.clone();
                        let token =
                            handle.insert_source(Timer::from_duration(delay), move |_, _, data| {
                                let next = {
                                    let state = state.borrow();
                                    if state.repeat_revision != revision {
                                        None
                                    } else {
                                        state.repeat_event()
                                    }
                                };
                                if let Some((event, interval)) = next {
                                    repeat_callback.borrow_mut()(event, data);
                                    TimeoutAction::ToDuration(interval)
                                } else {
                                    TimeoutAction::Drop
                                }
                            });
                        match token {
                            Ok(token) => repeat_source.set(Some(token)),
                            Err(error) => {
                                callback.borrow_mut()(
                                    InputEvent::Lost(format!(
                                        "cannot schedule keyboard repeat: {error}"
                                    )),
                                    data,
                                );
                                return Ok(count);
                            }
                        }
                    }
                }
                for event in events {
                    callback.borrow_mut()(event, data);
                }
                Ok(count)
            },
        )?);
        input.connection.flush()?;
        Ok(input)
    }
}

impl<D: 'static> Drop for InputMethod<D> {
    fn drop(&mut self) {
        if let Some(token) = self.repeat_source.take() {
            self.handle.remove(token);
        }
        if let Some(token) = self.source.take() {
            self.handle.remove(token);
        }
        if let Some(grab) = self.grab.take() {
            grab.release();
        }
        self.method.destroy();
        let _ = self.connection.flush();
        // Ensure the seat slot is free before an immediate grid -> mouse handoff
        // acquires a new input method on a different connection.
        let _ = self.connection.roundtrip();
    }
}

#[derive(Default)]
struct GrabState {
    seats: Vec<(wl_seat::WlSeat, String, bool)>,
    selected_seat: Option<wl_seat::WlSeat>,
    keyboard: Option<xkb::State>,
    events: Vec<InputEvent>,
    failure: Option<String>,
    repeat_key: Option<u32>,
    repeat_revision: u64,
    rate: i32,
    delay: i32,
}

impl GrabState {
    fn handle_key(&mut self, key: u32, key_state: KeyState) {
        if let Some(event) = self.key_event(key, key_state, false) {
            self.events.push(event);
        } else {
            self.failure = Some("input-method key arrived without a usable keymap".into());
        }
        if key_state == KeyState::Released && self.repeat_key == Some(key) {
            self.stop_repeat();
        } else if key_state == KeyState::Pressed
            && let Some(keyboard) = &self.keyboard
            && let Some(code) = key.checked_add(8)
            && keyboard.get_keymap().key_repeats(xkb::Keycode::new(code))
        {
            self.repeat_key = Some(key);
            self.repeat_revision = self.repeat_revision.wrapping_add(1);
        }
    }

    fn stop_repeat(&mut self) {
        self.repeat_key = None;
        self.repeat_revision = self.repeat_revision.wrapping_add(1);
    }

    fn key_event(&self, code: u32, state: KeyState, repeated: bool) -> Option<InputEvent> {
        let keyboard = self.keyboard.as_ref()?;
        let keycode = xkb::Keycode::new(code.checked_add(8)?);
        let text = keyboard.key_get_utf8(keycode);
        Some(InputEvent::Key {
            code,
            symbol: input_symbol(Some(&text), keyboard.key_get_one_sym(keycode)),
            state,
            repeated,
        })
    }

    fn repeat_delay(&self) -> Option<Duration> {
        (self.repeat_key.is_some() && self.rate > 0)
            .then(|| Duration::from_millis(self.delay.max(0) as u64))
    }

    fn repeat_event(&self) -> Option<(InputEvent, Duration)> {
        if self.rate <= 0 {
            return None;
        }
        let event = self.key_event(self.repeat_key?, KeyState::Pressed, true)?;
        let interval =
            Duration::from_secs_f64(1.0 / f64::from(self.rate)).max(Duration::from_millis(1));
        Some((event, interval))
    }

    fn set_keymap(&mut self, fd: std::os::fd::OwnedFd, size: u32) -> BackendResult {
        const MAX_KEYMAP_BYTES: u32 = 16 * 1024 * 1024;
        if size == 0 || size > MAX_KEYMAP_BYTES {
            return Err("input-method keymap has an invalid size".into());
        }
        let mut bytes = vec![0; size as usize];
        // Wayland passes shared file descriptions: the position may be at EOF
        // or changed by another recipient. Do not read or modify that position.
        File::from(fd).read_exact_at(&mut bytes, 0)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("input-method keymap is not a terminated XKB string")?;
        bytes.truncate(end);
        let text = String::from_utf8(bytes)?;
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_string(
            &context,
            text,
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("cannot compile input-method keyboard map")?;
        self.keyboard = Some(xkb::State::new(&keymap));
        self.stop_repeat();
        Ok(())
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for GrabState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for GrabState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, name, keyboard)) = state
            .seats
            .iter_mut()
            .find(|(candidate, _, _)| candidate == seat)
        else {
            return;
        };
        match event {
            wl_seat::Event::Name { name: value } => *name = value,
            wl_seat::Event::Capabilities {
                capabilities: WEnum::Value(capabilities),
            } => {
                *keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
                if !*keyboard && state.selected_seat.as_ref() == Some(seat) {
                    state.failure = Some("input-method seat lost its keyboard".into());
                    state.stop_repeat();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for GrabState {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_input_method_v2::Event::Unavailable = event {
            state.failure = Some("input-method capture unavailable: another input method may own the seat, or the seat was removed; overlay focus will not be taken".into());
            state.stop_repeat();
        }
    }
}

impl Dispatch<ZwpInputMethodKeyboardGrabV2, ()> for GrabState {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodKeyboardGrabV2,
        event: zwp_input_method_keyboard_grab_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_keyboard_grab_v2::Event::Keymap { format, fd, size } => {
                let result = if format == WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
                    state.set_keymap(fd, size)
                } else {
                    Err("input-method keyboard requires an XKB keymap".into())
                };
                if let Err(error) = result {
                    state.failure = Some(error.to_string());
                    state.stop_repeat();
                }
            }
            zwp_input_method_keyboard_grab_v2::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(keyboard) = &mut state.keyboard {
                    keyboard.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            zwp_input_method_keyboard_grab_v2::Event::RepeatInfo { rate, delay } => {
                state.rate = rate.max(0);
                state.delay = delay.max(0);
                state.repeat_revision = state.repeat_revision.wrapping_add(1);
            }
            zwp_input_method_keyboard_grab_v2::Event::Key {
                key, state: value, ..
            } => {
                let key_state = match value {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => KeyState::Pressed,
                    WEnum::Value(wl_keyboard::KeyState::Released) => KeyState::Released,
                    _ => return,
                };
                state.handle_key(key, key_state);
            }
            _ => {}
        }
    }
}

delegate_noop!(GrabState: ignore ZwpInputMethodManagerV2);

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard() -> GrabState {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "evdev",
            "pc105",
            "us",
            "",
            Some(String::new()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        GrabState {
            keyboard: Some(xkb::State::new(&keymap)),
            rate: 25,
            delay: 400,
            ..GrabState::default()
        }
    }

    #[test]
    fn translates_labels_and_control_keys_from_evdev_codes() {
        let state = keyboard();
        for (code, expected) in [(30, "a"), (28, "Return"), (1, "Escape"), (14, "BackSpace")] {
            assert!(matches!(state.key_event(code, KeyState::Pressed, false),
                Some(InputEvent::Key { code: actual, symbol, repeated: false, .. })
                    if actual == code && symbol == expected));
        }
    }

    #[test]
    fn modifiers_change_symbols_but_release_keeps_the_physical_code() {
        let mut state = keyboard();
        state.handle_key(30, KeyState::Pressed);
        let keyboard = state.keyboard.as_mut().unwrap();
        let shift = keyboard.get_keymap().mod_get_index(xkb::MOD_NAME_SHIFT);
        keyboard.update_mask(1 << shift, 0, 0, 0, 0, 0);
        state.handle_key(30, KeyState::Released);
        assert!(
            matches!(&state.events[0], InputEvent::Key { code: 30, symbol, state: KeyState::Pressed, .. } if symbol == "a")
        );
        assert!(
            matches!(&state.events[1], InputEvent::Key { code: 30, symbol, state: KeyState::Released, .. } if symbol == "A")
        );
    }

    #[test]
    fn repeat_tracks_the_latest_key_and_ignores_other_key_releases() {
        let mut state = keyboard();
        state.handle_key(35, KeyState::Pressed); // h
        state.handle_key(36, KeyState::Pressed); // j
        state.handle_key(35, KeyState::Released);
        assert_eq!(state.repeat_delay(), Some(Duration::from_millis(400)));
        let (event, interval) = state.repeat_event().unwrap();
        assert!(matches!(
            event,
            InputEvent::Key {
                code: 36,
                repeated: true,
                ..
            }
        ));
        assert_eq!(interval, Duration::from_millis(40));
        state.handle_key(36, KeyState::Released);
        assert!(state.repeat_event().is_none());
        state.handle_key(35, KeyState::Pressed);
        state.rate = 0;
        assert!(state.repeat_delay().is_none());
        assert!(state.repeat_event().is_none());
    }

    #[test]
    fn unusable_keymaps_fail_instead_of_accepting_input() {
        let mut state = GrabState::default();
        state.handle_key(30, KeyState::Pressed);
        assert!(state.failure.is_some());
        assert!(state.events.is_empty());
        for size in [0, 1, 32 * 1024 * 1024] {
            let fd = File::open("/dev/null").unwrap().into();
            assert!(state.set_keymap(fd, size).is_err());
        }
    }

    #[test]
    fn shared_keymap_descriptors_are_read_without_using_or_changing_their_offset() {
        use std::io::{Seek, Write};
        let keyboard = keyboard();
        let text = keyboard
            .keyboard
            .as_ref()
            .unwrap()
            .get_keymap()
            .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let path = std::env::temp_dir().join(format!(
            "mousr-keymap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::fs::remove_file(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.write_all(&[0, 0]).unwrap();
        let end = file.stream_position().unwrap();
        let mut state = GrabState::default();
        for _ in 0..2 {
            state
                .set_keymap(file.try_clone().unwrap().into(), end as u32)
                .unwrap();
            assert_eq!(file.stream_position().unwrap(), end);
            assert!(matches!(state.key_event(30, KeyState::Pressed, false),
                Some(InputEvent::Key { symbol, .. }) if symbol == "a"));
        }
    }

    #[test]
    #[ignore = "requires isolated headless Wayland compositor and wtype"]
    fn compositor_grab_handles_occupied_seats_repeats_and_reacquisition() {
        assert_eq!(std::env::var("MOUSR_HEADLESS_TEST").as_deref(), Ok("1"));
        let mut event_loop = calloop::EventLoop::<Vec<InputEvent>>::try_new().unwrap();
        let first = InputMethod::acquire(event_loop.handle(), "seat0", |event, events| {
            events.push(event)
        })
        .unwrap();
        let error = InputMethod::acquire(event_loop.handle(), "seat0", |event, events| {
            events.push(event)
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains("another input method"));
        drop(first);
        let second = InputMethod::acquire(event_loop.handle(), "seat0", |event, events| {
            events.push(event)
        })
        .unwrap();
        let mut keys = std::process::Command::new("wtype")
            .args([
                "-k", "a", "-P", "h", "-s", "1000", "-p", "h", "-k", "Escape",
            ])
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut events = Vec::new();
        loop {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut events)
                .unwrap();
            if let Some(status) = keys.try_wait().unwrap() {
                assert!(status.success());
                event_loop
                    .dispatch(Some(Duration::from_millis(50)), &mut events)
                    .unwrap();
                break;
            }
            if std::time::Instant::now() > deadline {
                keys.kill().unwrap();
                keys.wait().unwrap();
                panic!("test key injection timed out");
            }
        }
        for symbol in ["a", "h", "Escape"] {
            assert!(events.iter().any(|event| matches!(event,
                InputEvent::Key { symbol: actual, state: KeyState::Pressed, .. } if actual == symbol)));
            assert!(events.iter().any(|event| matches!(event,
                InputEvent::Key { symbol: actual, state: KeyState::Released, .. } if actual == symbol)));
        }
        assert!(
            events
                .iter()
                .any(|event| matches!(event, InputEvent::Key { repeated: true, .. }))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, InputEvent::Lost(_)))
        );
        drop(second);
        let _third = InputMethod::acquire(event_loop.handle(), "seat0", |event, events| {
            events.push(event)
        })
        .unwrap();
    }
}
