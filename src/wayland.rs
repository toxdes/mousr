use std::{io::ErrorKind, os::unix::net::UnixListener, time::Duration};

use calloop::{
    EventLoop, Interest, LoopHandle, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use log::{debug, error, info, warn};
use rustix::time::{ClockId, Timespec, clock_gettime};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer as ShmBuffer, SlotPool},
    },
};
use thiserror::Error;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

use crate::{
    app::App,
    backend::{BackendResult, DisplayBackend, OutputInfo, Timer as AppTimer},
    cli::{DaemonOptions, Direction, MouseButton},
    compositor::{CompositorControl, CompositorError},
    config::{Config, ConfigError},
    ipc::{self, IpcError, Response},
    mode::{KeyInput, KeyState},
    render::{Frame, Screenshot},
};

#[derive(Debug, Error)]
pub enum WaylandError {
    #[error("cannot connect to Wayland: {0}")]
    Connect(#[from] wayland_client::ConnectError),
    #[error("cannot initialize Wayland globals: {0}")]
    Globals(#[from] wayland_client::globals::GlobalError),
    #[error("required Wayland global is unavailable: {0}")]
    MissingGlobal(String),
    #[error("Wayland dispatch failed: {0}")]
    Dispatch(#[from] wayland_client::DispatchError),
    #[error("event loop failed: {0}")]
    EventLoop(#[from] calloop::Error),
    #[error("event source failed: {0}")]
    Source(#[from] calloop::InsertError<WaylandSource<State>>),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("cannot initialize application: {0}")]
    Application(String),
    #[error(transparent)]
    Ipc(#[from] IpcError),
    #[error("shared-memory buffer error: {0}")]
    Shm(#[from] std::io::Error),
    #[error("no keyboard-capable Wayland seat matched the requested seat")]
    NoSeat,
}

struct Overlay {
    name: String,
    width: u32,
    height: u32,
    layer: LayerSurface,
    configured: bool,
}

#[derive(Clone)]
struct Output {
    name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    wl_output: wl_output::WlOutput,
}

struct PendingCapture {
    id: u64,
    frame: ZwlrScreencopyFrameV1,
    output: String,
    logical_width: u32,
    logical_height: u32,
    format: Option<wl_shm::Format>,
    width: u32,
    height: u32,
    stride: u32,
    buffer: Option<ShmBuffer>,
    y_invert: bool,
}

type State = App<WaylandDisplay>;

pub struct WaylandDisplay {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    capture_pool: SlotPool,
    compositor: CompositorState,
    layer_shell: LayerShell,
    overlays: Vec<Overlay>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    seat: Option<wl_seat::WlSeat>,
    seat_name: String,
    shortcut_manager: Option<ZwpKeyboardShortcutsInhibitManagerV1>,
    shortcut_inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
    qh: QueueHandle<State>,
    loop_handle: LoopHandle<'static, State>,
    outputs: Vec<Output>,
    pointer_manager: Option<ZwlrVirtualPointerManagerV1>,
    screencopy_manager: Option<ZwlrScreencopyManagerV1>,
    relative_pointer: Option<ZwlrVirtualPointerV1>,
    desktop: Box<dyn CompositorControl>,
    pending_captures: Vec<PendingCapture>,
}

pub fn run_daemon(
    options: DaemonOptions,
    desktop: Box<dyn CompositorControl>,
) -> Result<(), WaylandError> {
    info!(target: "mousr::wayland", "initializing daemon seat={:?} config={:?}", options.seat, options.config);
    let config = Config::load(options.config.as_deref())?;
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let shm =
        Shm::bind(&globals, &qh).map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let shortcut_manager = globals.bind(&qh, 1..=1, ()).ok();
    let pointer_manager = globals.bind(&qh, 2..=2, ()).ok();
    let screencopy_manager = globals.bind(&qh, 3..=3, ()).ok();
    debug!(target: "mousr::wayland", "bound optional Wayland globals shortcut_inhibit={} virtual_pointer={} screencopy={}", shortcut_manager.is_some(), pointer_manager.is_some(), screencopy_manager.is_some());
    let pool =
        SlotPool::new(4, &shm).map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let capture_pool =
        SlotPool::new(4, &shm).map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let mut event_loop: EventLoop<'static, State> = EventLoop::try_new()?;
    let display = WaylandDisplay {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        capture_pool,
        compositor,
        layer_shell,
        overlays: Vec::new(),
        keyboard: None,
        seat: None,
        seat_name: options.seat.unwrap_or_else(|| "seat0".into()),
        shortcut_manager,
        shortcut_inhibitor: None,
        qh: qh.clone(),
        loop_handle: event_loop.handle(),
        outputs: Vec::new(),
        pointer_manager,
        screencopy_manager,
        relative_pointer: None,
        desktop,
        pending_captures: Vec::new(),
    };
    let mut state = App::new(display, config, options.config)
        .map_err(|error| WaylandError::Application(error.to_string()))?;
    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;
    state.display.select_seat(&qh, event_loop.handle())?;
    let _ = state.display.refresh_outputs()?;
    state.display.create_virtual_pointer(&qh);
    state.display.create_overlays(&qh);
    event_queue.roundtrip(&mut state)?;

    let (listener, socket_guard) = ipc::bind_listener()?;
    WaylandSource::new(conn, event_queue).insert(event_loop.handle())?;
    insert_ipc(&event_loop, listener)
        .map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let _socket_guard = socket_guard;
    info!(target: "mousr::wayland", "daemon started");
    event_loop.run(None, &mut state, |_| {})?;
    Ok(())
}

fn insert_ipc(
    event_loop: &EventLoop<'_, State>,
    listener: UnixListener,
) -> Result<(), calloop::InsertError<Generic<UnixListener>>> {
    event_loop.handle().insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, state| {
            loop {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                };
                let response = match ipc::read_request(&stream) {
                    Ok(command) => {
                        debug!(target: "mousr::wayland", "received IPC command={command}");
                        match state.command(command) {
                            Ok(message) => Response::ok(message),
                            Err(error) => Response::error(error.to_string()),
                        }
                    }
                    Err(error) => Response::error(error.to_string()),
                };
                if let Err(error) = ipc::write_response(&mut stream, &response) {
                    error!(target: "mousr::wayland", "cannot answer IPC request: {error}");
                }
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(())
}

impl State {
    fn reconcile_outputs(&mut self, qh: &QueueHandle<Self>, reason: &str) {
        match self.display.refresh_outputs() {
            Ok(true) => {
                info!(target: "mousr::wayland", "rebuilding output surfaces reason={reason}");
                self.rebuild_output_surfaces(qh);
            }
            Ok(false) => {}
            Err(error) => debug!(
                target: "mousr::wayland",
                "output reconciliation deferred reason={reason}: {error}"
            ),
        }
    }

    fn rebuild_output_surfaces(&mut self, qh: &QueueHandle<Self>) {
        self.cancel();
        self.display.overlays.clear();
        self.display.relative_pointer = None;
        self.display.create_virtual_pointer(qh);
        self.display.create_overlays(qh);
    }

    fn input(&mut self, event: KeyEvent, state: KeyState, repeated: bool) {
        let symbol = input_symbol(event.utf8.as_deref(), event.keysym);
        self.key(KeyInput {
            code: event.raw_code,
            symbol: &symbol,
            state,
            repeated,
        });
    }

    fn finish_capture(&mut self, frame: &ZwlrScreencopyFrameV1, screenshot: Option<Screenshot>) {
        let Some(index) = self
            .display
            .pending_captures
            .iter()
            .position(|capture| &capture.frame == frame)
        else {
            return;
        };
        let capture = self.display.pending_captures.remove(index);
        capture.frame.destroy();
        self.captured(capture.id, &capture.output, screenshot);
    }
}

impl WaylandDisplay {
    fn refresh_outputs(&mut self) -> Result<bool, WaylandError> {
        let mut outputs = Vec::new();
        for wl_output in self.output_state.outputs() {
            let Some(info) = self.output_state.info(&wl_output) else {
                continue;
            };
            let Some(name) = info.name else {
                continue;
            };
            let (x, y) = info.logical_position.unwrap_or(info.location);
            let size = info.logical_size.or_else(|| {
                info.modes.iter().find(|mode| mode.current).map(|mode| {
                    let scale = info.scale_factor.max(1);
                    (mode.dimensions.0 / scale, mode.dimensions.1 / scale)
                })
            });
            let Some((width, height)) = size else {
                continue;
            };
            let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
                continue;
            };
            if width == 0 || height == 0 {
                continue;
            }
            outputs.push(Output {
                name,
                x,
                y,
                width,
                height,
                wl_output,
            });
        }
        outputs.sort_by_key(|output| (output.y, output.x, output.name.clone()));
        if outputs.is_empty() {
            return Err(WaylandError::MissingGlobal(
                "no Wayland output supplied usable geometry".into(),
            ));
        }
        let changed = outputs.len() != self.outputs.len()
            || outputs.iter().zip(&self.outputs).any(|(new, old)| {
                new.name != old.name
                    || new.x != old.x
                    || new.y != old.y
                    || new.width != old.width
                    || new.height != old.height
            });
        if !changed {
            return Ok(false);
        }
        for output in &outputs {
            debug!(
                target: "mousr::wayland",
                "output available name={} position={},{} size={}x{}",
                output.name,
                output.x,
                output.y,
                output.width,
                output.height
            );
        }
        self.outputs = outputs;
        Ok(true)
    }

    fn create_virtual_pointer(&mut self, qh: &QueueHandle<State>) {
        let (Some(manager), Some(seat)) = (&self.pointer_manager, &self.seat) else {
            warn!(target: "mousr::wayland", "virtual pointer unavailable; using compositor cursor commands");
            return;
        };
        self.relative_pointer = Some(manager.create_virtual_pointer(Some(seat), qh, ()));
    }

    fn select_seat(
        &mut self,
        qh: &QueueHandle<State>,
        loop_handle: LoopHandle<'static, State>,
    ) -> Result<(), WaylandError> {
        let seat = self
            .seat_state
            .seats()
            .find(|seat| {
                self.seat_state.info(seat).is_some_and(|info| {
                    info.has_keyboard && info.name.as_deref().unwrap_or("seat0") == self.seat_name
                })
            })
            .ok_or(WaylandError::NoSeat)?;
        self.bind_keyboard(qh, &seat, loop_handle)
            .map_err(WaylandError::MissingGlobal)?;
        Ok(())
    }

    fn bind_keyboard(
        &mut self,
        qh: &QueueHandle<State>,
        seat: &wl_seat::WlSeat,
        loop_handle: LoopHandle<'static, State>,
    ) -> Result<(), String> {
        let keyboard = self
            .seat_state
            .get_keyboard_with_repeat(
                qh,
                seat,
                None,
                loop_handle,
                Box::new(|state, _, event| state.input(event, KeyState::Pressed, true)),
            )
            .map_err(|error| error.to_string())?;
        self.keyboard = Some(keyboard);
        self.seat = Some(seat.clone());
        Ok(())
    }

    fn create_overlays(&mut self, qh: &QueueHandle<State>) {
        for output in &self.outputs {
            debug!(target: "mousr::wayland", "creating overlay output={}", output.name);
            let surface = self.compositor.create_surface(qh);
            let layer = self.layer_shell.create_layer_surface(
                qh,
                surface,
                Layer::Overlay,
                Some("mousr"),
                Some(&output.wl_output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_size(output.width, output.height);
            let input_region = self.compositor.wl_compositor().create_region(qh, ());
            layer.set_input_region(Some(&input_region));
            input_region.destroy();
            layer.commit();
            self.overlays.push(Overlay {
                name: output.name.clone(),
                width: output.width,
                height: output.height,
                layer,
                configured: false,
            });
        }
    }

    fn park_overlays(&mut self) -> Result<(), String> {
        let mut pool = SlotPool::new(self.overlays.len().max(1) * 64, &self.shm)
            .map_err(|error| error.to_string())?;
        for overlay in &self.overlays {
            if !overlay.configured {
                continue;
            }
            let (buffer, canvas) = pool
                .create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
                .map_err(|error| error.to_string())?;
            canvas.fill(0);
            overlay
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            overlay.layer.set_size(1, 1);
            buffer
                .attach_to(overlay.layer.wl_surface())
                .map_err(|error| error.to_string())?;
            overlay.layer.wl_surface().damage_buffer(0, 0, 1, 1);
            overlay.layer.commit();
        }
        self.pool = pool;
        Ok(())
    }

    fn present_pixels(
        &mut self,
        index: usize,
        bytes: Option<&[u8]>,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let required = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        self.pool
            .resize(required.max(4))
            .map_err(|error| error.to_string())?;
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
            )
            .map_err(|error| error.to_string())?;
        if let Some(bytes) = bytes {
            canvas.copy_from_slice(bytes);
        } else {
            canvas.fill(0);
        }
        let surface = self.overlays[index].layer.wl_surface();
        buffer
            .attach_to(surface)
            .map_err(|error| error.to_string())?;
        surface.damage_buffer(0, 0, width as i32, height as i32);
        self.overlays[index].layer.commit();
        Ok(())
    }

    fn active_pointer(&self) -> Option<&ZwlrVirtualPointerV1> {
        self.relative_pointer.as_ref()
    }
}

impl DisplayBackend for WaylandDisplay {
    fn outputs(&self) -> impl Iterator<Item = OutputInfo<'_>> {
        self.outputs.iter().map(|output| OutputInfo {
            name: &output.name,
            x: output.x,
            y: output.y,
            width: output.width,
            height: output.height,
            ready: self
                .overlays
                .iter()
                .any(|overlay| overlay.name == output.name && overlay.configured),
        })
    }

    fn focused_output(&self) -> BackendResult<String> {
        Ok(self.desktop.focused_output()?)
    }
    fn focus(&mut self, output: &str) -> BackendResult {
        let index = self
            .overlays
            .iter()
            .position(|overlay| overlay.configured && overlay.name == output)
            .ok_or_else(|| format!("overlay for output {output:?} is unavailable"))?;
        // Commit the keyboard target last so another output cannot win focus.
        for (i, overlay) in self.overlays.iter().enumerate() {
            if !overlay.configured || i == index {
                continue;
            }
            overlay
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            overlay.layer.set_size(overlay.width, overlay.height);
            overlay.layer.commit();
        }
        let target = &self.overlays[index];
        target
            .layer
            .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        target.layer.set_size(target.width, target.height);
        target.layer.commit();
        if let (Some(manager), Some(seat)) = (&self.shortcut_manager, &self.seat) {
            self.shortcut_inhibitor =
                Some(manager.inhibit_shortcuts(target.layer.wl_surface(), seat, &self.qh, ()));
        }
        Ok(())
    }
    fn park(&mut self) -> BackendResult {
        if let Some(inhibitor) = self.shortcut_inhibitor.take() {
            inhibitor.destroy();
        }
        self.park_overlays()?;
        if self.capture_pool.len() > 4 {
            self.capture_pool = SlotPool::new(4, &self.shm)?;
        }
        Ok(())
    }
    fn present(&mut self, output: &str, frame: &Frame) -> BackendResult {
        let index = self
            .overlays
            .iter()
            .position(|overlay| overlay.name == output && overlay.configured)
            .ok_or_else(|| format!("overlay for output {output:?} is unavailable"))?;
        self.present_pixels(index, Some(&frame.argb8888), frame.width, frame.height)
            .map_err(Into::into)
    }
    fn can_capture(&self) -> bool {
        self.screencopy_manager.is_some()
    }

    fn can_inhibit_shortcuts(&self) -> bool {
        self.shortcut_manager.is_some()
    }

    fn capture(&mut self, id: u64, name: &str) -> BackendResult {
        let manager = self
            .screencopy_manager
            .clone()
            .ok_or("screen capture is unavailable")?;
        let output = self
            .outputs
            .iter()
            .find(|output| output.name == name)
            .ok_or_else(|| format!("output {name:?} disappeared before capture"))?;
        let (width, height, wl_output) = (output.width, output.height, output.wl_output.clone());
        let index = self
            .overlays
            .iter()
            .position(|overlay| overlay.name == name && overlay.configured)
            .ok_or_else(|| format!("overlay for output {name:?} is unavailable"))?;
        self.present_pixels(index, None, width, height)?;
        let frame = manager.capture_output(0, &wl_output, &self.qh, ());
        self.pending_captures.push(PendingCapture {
            id,
            frame,
            output: name.to_owned(),
            logical_width: width,
            logical_height: height,
            format: None,
            width: 0,
            height: 0,
            stride: 0,
            buffer: None,
            y_invert: false,
        });
        Ok(())
    }

    fn cancel_captures(&mut self) {
        for capture in self.pending_captures.drain(..) {
            capture.frame.destroy();
        }
    }
    fn schedule(&mut self, timer: AppTimer, delay: Duration) -> BackendResult {
        self.loop_handle
            .insert_source(Timer::from_duration(delay), move |_, _, state| {
                state
                    .timer(timer)
                    .map_or(TimeoutAction::Drop, TimeoutAction::ToDuration)
            })?;
        Ok(())
    }
    fn warp(&mut self, output_name: &str, x: u32, y: u32) -> BackendResult {
        let output = self
            .outputs
            .iter()
            .find(|output| output.name == output_name)
            .ok_or_else(|| CompositorError::MissingOutput(output_name.to_owned()))?;
        // Output-bound virtual pointers cannot reliably warp across outputs;
        // compositor coordinates also allow outputs left of or above the origin.
        self.desktop.warp_cursor(
            &self.seat_name,
            i64::from(output.x) + i64::from(x),
            i64::from(output.y) + i64::from(y),
        )?;
        Ok(())
    }
    fn move_cursor(&mut self, x: f64, y: f64) -> BackendResult {
        if let Some(pointer) = self.active_pointer() {
            pointer.motion(monotonic_time_ms(), x, y);
            pointer.frame();
            Ok(())
        } else {
            Ok(self.desktop.move_cursor(&self.seat_name, x, y)?)
        }
    }
    fn click(&mut self, button: MouseButton) -> BackendResult {
        if let Some(pointer) = self.active_pointer() {
            let time = monotonic_time_ms();
            let button = button_code(button);
            pointer.button(time, button, wl_pointer::ButtonState::Pressed);
            pointer.button(time, button, wl_pointer::ButtonState::Released);
            pointer.frame();
            Ok(())
        } else {
            Ok(self.desktop.click(&self.seat_name, button)?)
        }
    }
    fn button(&mut self, button: MouseButton, state: KeyState) -> BackendResult {
        if let Some(pointer) = self.active_pointer() {
            let state = match state {
                KeyState::Pressed => wl_pointer::ButtonState::Pressed,
                KeyState::Released => wl_pointer::ButtonState::Released,
            };
            pointer.button(monotonic_time_ms(), button_code(button), state);
            pointer.frame();
            return Ok(());
        }
        Ok(self.desktop.button(&self.seat_name, button, state)?)
    }
    fn scroll(&mut self, direction: Direction, amount: f64) -> BackendResult {
        if let Some(pointer) = self.active_pointer() {
            let (axis, sign) = match direction {
                Direction::Up => (wl_pointer::Axis::VerticalScroll, -1.0),
                Direction::Down => (wl_pointer::Axis::VerticalScroll, 1.0),
                Direction::Left => (wl_pointer::Axis::HorizontalScroll, -1.0),
                Direction::Right => (wl_pointer::Axis::HorizontalScroll, 1.0),
            };
            let discrete = (amount / 15.0).round().max(1.0) as i32;
            pointer.axis_source(wl_pointer::AxisSource::Wheel);
            pointer.axis_discrete(
                monotonic_time_ms(),
                axis,
                sign * amount,
                sign as i32 * discrete,
            );
            pointer.frame();
            return Ok(());
        }
        Ok(self.desktop.scroll(&self.seat_name, direction, amount)?)
    }
}

fn screenshot_from_capture(pool: &mut SlotPool, capture: &PendingCapture) -> Option<Screenshot> {
    let buffer = capture.buffer.as_ref()?;
    let bytes = buffer.canvas(pool)?;
    let rgba = rgba_from_shm(
        bytes,
        capture.width,
        capture.height,
        capture.stride,
        capture.format?,
        capture.y_invert,
    )?;
    Some(Screenshot {
        width: capture.width,
        height: capture.height,
        logical_width: capture.logical_width,
        logical_height: capture.logical_height,
        rgba,
    })
}

fn rgba_from_shm(
    bytes: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    format: wl_shm::Format,
    y_invert: bool,
) -> Option<Vec<u8>> {
    let row_bytes = usize::try_from(width).ok()?.checked_mul(4)?;
    let stride = usize::try_from(stride).ok()?;
    let height = usize::try_from(height).ok()?;
    if stride < row_bytes || bytes.len() < stride.checked_mul(height)? {
        return None;
    }
    let mut rgba = vec![0_u8; row_bytes.checked_mul(height)?];
    for destination_y in 0..height {
        let source_y = if y_invert {
            height - destination_y - 1
        } else {
            destination_y
        };
        let source = &bytes[source_y * stride..source_y * stride + row_bytes];
        let destination = &mut rgba[destination_y * row_bytes..(destination_y + 1) * row_bytes];
        let (source_pixels, _) = source.as_chunks::<4>();
        let (destination_pixels, _) = destination.as_chunks_mut::<4>();
        for (source, destination) in source_pixels.iter().zip(destination_pixels) {
            let (red, green, blue) = match format {
                wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 => {
                    (source[2], source[1], source[0])
                }
                wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => {
                    (source[0], source[1], source[2])
                }
                _ => return None,
            };
            destination[0] = red;
            destination[1] = green;
            destination[2] = blue;
            destination[3] = match format {
                wl_shm::Format::Argb8888 | wl_shm::Format::Abgr8888 => source[3],
                _ => 255,
            };
        }
    }
    Some(rgba)
}

fn monotonic_time_ms() -> u32 {
    timespec_to_millis(&clock_gettime(ClockId::Monotonic))
}

fn timespec_to_millis(timespec: &Timespec) -> u32 {
    let milliseconds = (timespec.tv_sec as u128 * 1_000) + (timespec.tv_nsec as u128 / 1_000_000);
    milliseconds as u32
}

fn button_code(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0x110,
        MouseButton::Right => 0x111,
        MouseButton::Middle => 0x112,
    }
}

fn keysym_name(keysym: Keysym) -> String {
    let name = keysym.name().unwrap_or_default();
    name.strip_prefix("XK_").unwrap_or(name).to_owned()
}

fn input_symbol(utf8: Option<&str>, keysym: Keysym) -> String {
    let printable = utf8.filter(|value| {
        let mut characters = value.chars();
        characters
            .next()
            .is_some_and(|character| !character.is_control() && !character.is_whitespace())
            && characters.next().is_none()
    });
    printable
        .map(str::to_owned)
        .unwrap_or_else(|| keysym_name(keysym))
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.display.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.display.overlays.is_empty() {
            self.reconcile_outputs(qh, "output added");
        }
    }
    fn update_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.display.overlays.is_empty() {
            self.reconcile_outputs(qh, "output changed");
        }
    }
    fn output_destroyed(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.display.overlays.is_empty() {
            self.reconcile_outputs(qh, "output removed");
        }
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let known = self
            .display
            .overlays
            .iter()
            .any(|overlay| overlay.layer.wl_surface() == layer.wl_surface());
        if known {
            warn!(target: "mousr::wayland", "overlay closed; rebuilding output surfaces");
            self.rebuild_output_surfaces(qh);
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(overlay) = self
            .display
            .overlays
            .iter_mut()
            .find(|overlay| overlay.layer.wl_surface() == layer.wl_surface())
        {
            overlay.configured = true;
            debug!(target: "mousr::wayland", "overlay configured output={}", overlay.name);
        }
        if let Err(error) = self.redraw() {
            error!(target: "mousr::wayland", "cannot redraw configured output: {error}");
            self.cancel();
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.display.seat_state
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        let matches = self.display.seat_state.info(&seat).is_some_and(|info| {
            info.has_keyboard && info.name.as_deref().unwrap_or("seat0") == self.display.seat_name
        });
        if matches
            && self.display.seat.is_none()
            && !self.display.overlays.is_empty()
            && let Err(error) =
                self.display
                    .bind_keyboard(qh, &seat, self.display.loop_handle.clone())
        {
            warn!(target: "mousr::wayland", "cannot rebind keyboard after seat appeared: {error}");
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Keyboard
            || !self.display.seat_state.info(&seat).is_some_and(|info| {
                info.name.as_deref().unwrap_or("seat0") == self.display.seat_name
            })
        {
            return;
        }
        let selected_seat = self
            .display
            .seat
            .as_ref()
            .is_some_and(|selected| selected == &seat);
        if (selected_seat || (self.display.keyboard.is_none() && !self.display.overlays.is_empty()))
            && let Err(error) =
                self.display
                    .bind_keyboard(qh, &seat, self.display.loop_handle.clone())
        {
            warn!(target: "mousr::wayland", "cannot rebind keyboard after capability returned: {error}");
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            self.display.keyboard = None;
            self.cancel();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.display.keyboard = None;
        self.display.seat = None;
        self.cancel();
    }
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_entered();
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.keyboard_left();
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.input(event, KeyState::Pressed, false);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.input(event, KeyState::Pressed, true);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.input(event, KeyState::Released, false);
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.display.shm
    }
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_keyboard!(State);
delegate_layer!(State);
delegate_registry!(State);
delegate_noop!(State: ignore wl_region::WlRegion);
delegate_noop!(State: ignore ZwpKeyboardShortcutsInhibitManagerV1);
delegate_noop!(State: ignore ZwlrScreencopyManagerV1);
delegate_noop!(State: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ignore ZwlrVirtualPointerV1);

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        frame: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let Some(capture) = state
                    .display
                    .pending_captures
                    .iter_mut()
                    .find(|capture| &capture.frame == frame)
                else {
                    return;
                };
                if let WEnum::Value(
                    format @ (wl_shm::Format::Argb8888
                    | wl_shm::Format::Xrgb8888
                    | wl_shm::Format::Abgr8888
                    | wl_shm::Format::Xbgr8888),
                ) = format
                {
                    capture.format = Some(format);
                    capture.width = width;
                    capture.height = height;
                    capture.stride = stride;
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                let Some(index) = state
                    .display
                    .pending_captures
                    .iter()
                    .position(|capture| &capture.frame == frame)
                else {
                    return;
                };
                let (format, width, height, stride) = {
                    let capture = &state.display.pending_captures[index];
                    (
                        capture.format,
                        capture.width,
                        capture.height,
                        capture.stride,
                    )
                };
                let Some(format) = format else {
                    state.finish_capture(frame, None);
                    return;
                };
                let buffer = state.display.capture_pool.create_buffer(
                    width as i32,
                    height as i32,
                    stride as i32,
                    format,
                );
                match buffer {
                    Ok((buffer, canvas)) => {
                        canvas.fill(0);
                        if let Err(error) = buffer.activate() {
                            eprintln!("mousr: cannot activate screencopy buffer: {error}");
                            state.finish_capture(frame, None);
                            return;
                        }
                        frame.copy(buffer.wl_buffer());
                        state.display.pending_captures[index].buffer = Some(buffer);
                    }
                    Err(error) => {
                        eprintln!("mousr: cannot allocate screencopy buffer: {error}");
                        state.finish_capture(frame, None);
                    }
                }
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                if let Some(capture) = state
                    .display
                    .pending_captures
                    .iter_mut()
                    .find(|capture| &capture.frame == frame)
                    && let WEnum::Value(flags) = flags
                {
                    capture.y_invert = flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let screenshot = state
                    .display
                    .pending_captures
                    .iter()
                    .position(|capture| &capture.frame == frame)
                    .and_then(|index| {
                        screenshot_from_capture(
                            &mut state.display.capture_pool,
                            &state.display.pending_captures[index],
                        )
                    });
                state.finish_capture(frame, screenshot);
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                warn!(target: "mousr::wayland", "Wayland screencopy failed; refinement will continue without a captured background");
                state.finish_capture(frame, None);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => {}
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                state.shortcuts_lost();
            }
            _ => {}
        }
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.display.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_whitespace_input_uses_keysym_names() {
        assert_eq!(input_symbol(Some("\u{1b}"), Keysym::Escape), "Escape");
        assert_eq!(input_symbol(Some("\r"), Keysym::Return), "Return");
        assert_eq!(input_symbol(Some(" "), Keysym::space), "space");
    }

    #[test]
    fn printable_input_uses_layout_text() {
        assert_eq!(input_symbol(Some("a"), Keysym::A), "a");
    }

    #[test]
    fn converts_and_flips_screencopy_pixels() {
        let pixels = [
            0, 0, 255, 0, // red in XRGB8888
            255, 0, 0, 0, // blue in XRGB8888
        ];
        let rgba = rgba_from_shm(&pixels, 1, 2, 4, wl_shm::Format::Xrgb8888, true).unwrap();
        assert_eq!(rgba, [0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn monotonic_timestamp_converts_to_milliseconds() {
        let timespec = Timespec {
            tv_sec: 12,
            tv_nsec: 345_678_901,
        };

        assert_eq!(timespec_to_millis(&timespec), 12_345);
    }

    #[test]
    fn monotonic_timestamp_uses_wayland_u32_representation() {
        let timespec = Timespec {
            tv_sec: (u64::from(u32::MAX) + 1_001) as i64,
            tv_nsec: 0,
        };

        assert_eq!(timespec_to_millis(&timespec), 1_000_000);
    }
}
