use std::{
    collections::HashMap,
    io::ErrorKind,
    os::unix::net::UnixListener,
    path::PathBuf,
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, Interest, LoopHandle, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use log::{debug, error, info, warn};
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
    cli::{Command, DaemonOptions, DaemonOptionsWire, Direction, GridOptions, MouseButton, Scope},
    compositor::{CompositorError, Sway},
    config::{Config, ConfigError, Motion, MotionCurve},
    grid::{self, Rect, Region, Settings, View},
    ipc::{self, IpcError, Response},
    mode::{Effect, GridSession, KeyState, MouseSession},
    render::{ActionHint, GridRender, RenderError, Renderer, Screenshot},
};

const FOCUS_LOSS_GRACE: Duration = Duration::from_millis(50);

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
    #[error(transparent)]
    Compositor(#[from] CompositorError),
    #[error(transparent)]
    Ipc(#[from] IpcError),
    #[error(transparent)]
    Render(#[from] RenderError),
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
    pointer: Option<ZwlrVirtualPointerV1>,
}

struct PendingCapture {
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

struct LensAnimation {
    from: Rect,
    target: View,
    started: Instant,
    duration: Duration,
    frame_interval: Duration,
}

impl LensAnimation {
    fn frame(&self, now: Instant) -> (View, f32, bool) {
        let linear = (now.duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32().max(f32::EPSILON))
        .clamp(0.0, 1.0);
        let progress = 1.0 - (1.0 - linear).powi(3);
        let mut view = self.target.clone();
        view.destination = interpolate_rect(self.from, self.target.destination, progress);
        (view, progress, linear >= 1.0)
    }
}

fn interpolate_rect(from: Rect, to: Rect, progress: f32) -> Rect {
    Rect {
        x: interpolate_u32(from.x, to.x, progress),
        y: interpolate_u32(from.y, to.y, progress),
        width: interpolate_u32(from.width, to.width, progress).max(1),
        height: interpolate_u32(from.height, to.height, progress).max(1),
    }
}

fn interpolate_u32(from: u32, to: u32, progress: f32) -> u32 {
    (from as f32 + (to as f32 - from as f32) * progress).round() as u32
}

fn lens_animation_steps(pixels: u64) -> u32 {
    if pixels > 8_294_400 {
        4
    } else if pixels > 3_686_400 {
        6
    } else {
        8
    }
}

enum Session {
    Idle,
    Grid(GridSession),
    Mouse(MouseSession),
    Scroll,
}

pub struct State {
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
    shortcuts_active: bool,
    qh: QueueHandle<State>,
    loop_handle: LoopHandle<'static, State>,
    outputs: Vec<Output>,
    pointer_manager: Option<ZwlrVirtualPointerManagerV1>,
    screencopy_manager: Option<ZwlrScreencopyManagerV1>,
    relative_pointer: Option<ZwlrVirtualPointerV1>,
    focused_output: String,
    sway: Sway,
    config: Config,
    config_path: Option<PathBuf>,
    renderer: Renderer,
    session: Session,
    pending_grid: Option<GridSession>,
    pending_captures: Vec<PendingCapture>,
    capture_failed: bool,
    grid_screenshots: HashMap<String, Screenshot>,
    lens_animation: Option<LensAnimation>,
    lens_animation_timer_active: bool,
    motion_started: Option<Instant>,
    last_motion: Option<Instant>,
    motion_timer_active: bool,
    keyboard_focused: bool,
    keyboard_focus_epoch: u64,
    target: Option<(String, u32, u32)>,
}

pub fn run_daemon(options: DaemonOptionsWire) -> Result<(), WaylandError> {
    let options: DaemonOptions = options.into();
    info!(target: "mousr::wayland", "initializing daemon seat={:?} config={:?}", options.seat, options.config);
    let config = Config::load(options.config.as_deref())?;
    let (renderer, warning) = Renderer::new(&config.ui)?;
    if let Some(warning) = warning {
        eprintln!("mousr: {warning}");
    }
    let sway = Sway::from_env()?;
    let focused_output = sway.focused_output()?;
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
    if config.general.require_shortcut_inhibit && shortcut_manager.is_none() {
        return Err(WaylandError::MissingGlobal(
            "zwp_keyboard_shortcuts_inhibit_manager_v1".into(),
        ));
    }
    let pool =
        SlotPool::new(4, &shm).map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let capture_pool =
        SlotPool::new(4, &shm).map_err(|error| WaylandError::MissingGlobal(error.to_string()))?;
    let mut event_loop: EventLoop<'static, State> = EventLoop::try_new()?;
    let mut state = State {
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
        shortcuts_active: false,
        qh: qh.clone(),
        loop_handle: event_loop.handle(),
        outputs: Vec::new(),
        pointer_manager,
        screencopy_manager,
        relative_pointer: None,
        focused_output,
        sway,
        config,
        config_path: options.config,
        renderer,
        session: Session::Idle,
        pending_grid: None,
        pending_captures: Vec::new(),
        capture_failed: false,
        grid_screenshots: HashMap::new(),
        lens_animation: None,
        lens_animation_timer_active: false,
        motion_started: None,
        last_motion: None,
        motion_timer_active: false,
        keyboard_focused: false,
        keyboard_focus_epoch: 0,
        target: None,
    };
    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;
    state.select_seat(&qh, event_loop.handle())?;
    let _ = state.refresh_outputs()?;
    state.create_virtual_pointers(&qh);
    state.create_overlays(&qh);
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
                        state.command(command)
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
                pointer: None,
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

    fn reconcile_outputs(&mut self, qh: &QueueHandle<Self>, reason: &str) {
        match self.refresh_outputs() {
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
        self.overlays.clear();
        self.relative_pointer = None;
        for output in &mut self.outputs {
            output.pointer = None;
        }
        self.create_virtual_pointers(qh);
        self.create_overlays(qh);
    }

    fn create_virtual_pointers(&mut self, qh: &QueueHandle<Self>) {
        let (Some(manager), Some(seat)) = (&self.pointer_manager, &self.seat) else {
            warn!(target: "mousr::wayland", "virtual pointer unavailable; using Sway cursor commands");
            return;
        };
        self.relative_pointer = Some(manager.create_virtual_pointer(Some(seat), qh, ()));
        for output in &mut self.outputs {
            output.pointer = Some(manager.create_virtual_pointer_with_output(
                Some(seat),
                Some(&output.wl_output),
                qh,
                (),
            ));
        }
    }

    fn select_seat(
        &mut self,
        qh: &QueueHandle<Self>,
        loop_handle: LoopHandle<'static, Self>,
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
        qh: &QueueHandle<Self>,
        seat: &wl_seat::WlSeat,
        loop_handle: LoopHandle<'static, Self>,
    ) -> Result<(), String> {
        let keyboard = self
            .seat_state
            .get_keyboard_with_repeat(
                qh,
                seat,
                None,
                loop_handle,
                Box::new(|state, _, event| state.key(event, KeyState::Pressed, true)),
            )
            .map_err(|error| error.to_string())?;
        self.keyboard = Some(keyboard);
        self.seat = Some(seat.clone());
        Ok(())
    }

    fn create_overlays(&mut self, qh: &QueueHandle<Self>) {
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

    fn command(&mut self, command: Command) -> Response {
        debug!(target: "mousr::wayland", "dispatching command={command}");
        match self.try_command(command) {
            Ok(message) => Response::ok(message),
            Err(error) => Response::error(error),
        }
    }

    fn try_command(&mut self, command: Command) -> Result<&'static str, String> {
        match command {
            Command::Reload => {
                let config =
                    Config::load(self.config_path.as_deref()).map_err(|e| e.to_string())?;
                let (renderer, warning) = Renderer::new(&config.ui).map_err(|e| e.to_string())?;
                if let Some(warning) = warning {
                    eprintln!("mousr: {warning}");
                }
                self.config = config;
                self.renderer = renderer;
                if !matches!(self.session, Session::Idle) {
                    self.redraw().map_err(|e| e.to_string())?;
                }
                Ok("configuration reloaded")
            }
            Command::Cancel => {
                self.cancel();
                Ok("cancelled")
            }
            Command::Daemon(_) => Err("daemon requests cannot be nested".into()),
            Command::Grid(options) => {
                self.start_grid(options)?;
                Ok("grid active")
            }
            Command::Mouse => {
                self.refresh_focused_output()?;
                self.target = None;
                self.activate(Session::Mouse(MouseSession::default()));
                self.redraw_mode().map_err(|e| e.to_string())?;
                Ok("mouse mode active")
            }
            Command::Click(button) => {
                self.refresh_focused_output()?;
                self.cancel();
                self.click(button).map_err(|e| e.to_string())?;
                Ok("clicked")
            }
            Command::Scroll { direction, step } => {
                self.refresh_focused_output()?;
                self.cancel();
                self.scroll(direction, step).map_err(|e| e.to_string())?;
                Ok("scrolled")
            }
        }
    }

    fn start_grid(&mut self, options: GridOptions) -> Result<(), String> {
        self.target = None;
        self.refresh_focused_output()?;
        let selected: Vec<&Output> = if let Some(name) = options.output.as_deref() {
            let output = self
                .outputs
                .iter()
                .find(|output| output.name == name)
                .ok_or_else(|| format!("output {name:?} is not active"))?;
            vec![output]
        } else {
            match options.scope.unwrap_or(self.config.general.scope) {
                Scope::All => self.outputs.iter().collect(),
                Scope::Focused => self
                    .outputs
                    .iter()
                    .filter(|output| output.name == self.focused_output)
                    .collect(),
            }
        };
        let regions: Vec<Region> = selected
            .into_iter()
            .map(|output| Region {
                output: output.name.clone(),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: output.width,
                    height: output.height,
                },
            })
            .collect();
        let capture_outputs: Vec<String> =
            regions.iter().map(|region| region.output.clone()).collect();
        let settings = self.grid_settings();
        let layout = grid::build_with_minimum(
            &regions,
            settings,
            self.config.grid.root_min_tile_width,
            self.config.grid.root_min_tile_height,
        )
        .map_err(|e| e.to_string())?;
        let refinement_zoom = if self.screencopy_manager.is_some() {
            self.config.grid.refinement_zoom
        } else {
            1.0
        };
        let session = GridSession::new(
            layout,
            settings,
            self.config.grid.root_min_tile_width,
            self.config.grid.root_min_tile_height,
            options.max_depth.unwrap_or(self.config.grid.max_depth),
            refinement_zoom,
            options
                .auto_descend
                .unwrap_or(self.config.grid.auto_descend),
            options.action,
            self.config.grid.exit_on_scroll,
        );
        if refinement_zoom > 1.0 {
            self.begin_grid_capture(session, &capture_outputs)?;
        } else {
            if self.config.grid.refinement_zoom > 1.0 {
                warn!(target: "mousr::wayland", "refinement zoom requested but Wayland screencopy v3 is unavailable; continuing without magnification");
            }
            self.activate(Session::Grid(session));
            self.redraw().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn begin_grid_capture(
        &mut self,
        session: GridSession,
        output_names: &[String],
    ) -> Result<(), String> {
        self.cancel();
        self.grid_screenshots.clear();
        self.capture_failed = false;
        let manager = self
            .screencopy_manager
            .as_ref()
            .ok_or_else(|| "Wayland screencopy is unavailable".to_owned())?;
        for name in output_names {
            let output = self
                .outputs
                .iter()
                .find(|output| &output.name == name)
                .ok_or_else(|| format!("output {name:?} disappeared before capture"))?;
            let frame = manager.capture_output(0, &output.wl_output, &self.qh, ());
            self.pending_captures.push(PendingCapture {
                frame,
                output: name.clone(),
                logical_width: output.width,
                logical_height: output.height,
                format: None,
                width: 0,
                height: 0,
                stride: 0,
                buffer: None,
                y_invert: false,
            });
        }
        self.pending_grid = Some(session);
        Ok(())
    }

    fn finish_capture(&mut self, frame: &ZwlrScreencopyFrameV1, screenshot: Option<Screenshot>) {
        let Some(index) = self
            .pending_captures
            .iter()
            .position(|capture| &capture.frame == frame)
        else {
            return;
        };
        let capture = self.pending_captures.remove(index);
        capture.frame.destroy();
        if let Some(screenshot) = screenshot {
            self.grid_screenshots.insert(capture.output, screenshot);
        } else {
            self.capture_failed = true;
        }
        if self.pending_captures.is_empty()
            && let Some(mut session) = self.pending_grid.take()
        {
            if self.capture_failed {
                session.disable_refinement_zoom();
                self.grid_screenshots.clear();
            }
            self.activate(Session::Grid(session));
            if let Err(error) = self.redraw() {
                eprintln!("mousr: cannot draw captured grid: {error}");
                self.cancel();
            }
        }
    }

    fn refresh_focused_output(&mut self) -> Result<(), String> {
        let focused = self
            .sway
            .focused_output()
            .map_err(|error| error.to_string())?;
        if !self.outputs.iter().any(|output| output.name == focused) {
            return Err(format!(
                "focused Sway output {focused:?} is not advertised by Wayland"
            ));
        }
        self.focused_output = focused;
        Ok(())
    }

    fn grid_settings(&self) -> Settings {
        Settings {
            min_tile_width: self.config.grid.min_tile_width,
            min_tile_height: self.config.grid.min_tile_height,
            max_label_length: self.config.grid.max_label_length,
            max_cells: self.config.grid.max_cells,
        }
    }

    fn activate(&mut self, session: Session) {
        self.cancel();
        self.session = session;
        let interactive = self.overlays.iter().position(|overlay| {
            if !overlay.configured {
                return false;
            }
            match &self.session {
                Session::Grid(grid) => {
                    overlay.name == self.focused_output
                        && grid
                            .layout()
                            .tiles
                            .iter()
                            .any(|tile| tile.output == overlay.name)
                }
                Session::Mouse(_) | Session::Scroll => overlay.name == self.focused_output,
                Session::Idle => false,
            }
        });
        // Commit the keyboard target last so another output cannot win focus during activation.
        for (index, overlay) in self.overlays.iter().enumerate() {
            if !overlay.configured || Some(index) == interactive {
                continue;
            }
            overlay
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            overlay.layer.set_size(overlay.width, overlay.height);
            overlay.layer.commit();
        }
        let target = interactive.map(|index| &self.overlays[index]);
        if let Some(target) = target {
            target
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            target.layer.set_size(target.width, target.height);
            target.layer.commit();
        }
        if let (Some(manager), Some(seat), Some(target)) =
            (&self.shortcut_manager, &self.seat, target)
        {
            self.shortcut_inhibitor =
                Some(manager.inhibit_shortcuts(target.layer.wl_surface(), seat, &self.qh, ()));
        }
    }

    fn cancel(&mut self) {
        self.lens_animation = None;
        for capture in self.pending_captures.drain(..) {
            capture.frame.destroy();
        }
        self.pending_grid = None;
        if let Some(inhibitor) = self.shortcut_inhibitor.take() {
            inhibitor.destroy();
        }
        self.shortcuts_active = false;
        self.motion_started = None;
        self.last_motion = None;
        let previous = std::mem::replace(&mut self.session, Session::Idle);
        if let Session::Mouse(mut mouse) = previous {
            for effect in mouse.cancel() {
                if let Effect::Button { button, state } = effect {
                    let _ = self.button(button, state);
                }
            }
        }
        if let Err(error) = self.park_overlays() {
            eprintln!("mousr: cannot park overlays: {error}");
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

    fn begin_lens_animation(
        &mut self,
        previous: Option<View>,
        previous_depth: usize,
        current: Option<View>,
        current_depth: usize,
    ) {
        let Some(target) = current else {
            self.lens_animation = None;
            return;
        };
        let duration = Duration::from_millis(u64::from(self.config.ui.lens_animation_ms));
        if duration.is_zero() || previous_depth == current_depth {
            self.lens_animation = None;
            return;
        }
        let from = if current_depth > previous_depth {
            previous
                .as_ref()
                .map_or(target.source, |view| view.map(target.source))
        } else {
            previous.map_or(target.source, |view| view.destination)
        };
        if from == target.destination {
            self.lens_animation = None;
            return;
        }
        let pixels = self
            .outputs
            .iter()
            .find(|output| output.name == target.output)
            .map_or(0, |output| {
                u64::from(output.width) * u64::from(output.height)
            });
        let steps = lens_animation_steps(pixels);
        self.lens_animation = Some(LensAnimation {
            from,
            target,
            started: Instant::now(),
            duration,
            frame_interval: (duration / steps).max(Duration::from_millis(8)),
        });
        self.ensure_lens_animation_timer();
    }

    fn ensure_lens_animation_timer(&mut self) {
        if self.lens_animation_timer_active {
            return;
        }
        let Some(interval) = self
            .lens_animation
            .as_ref()
            .map(|animation| animation.frame_interval)
        else {
            return;
        };
        self.lens_animation_timer_active = true;
        let handle = self.loop_handle.clone();
        if let Err(error) = handle.insert_source(Timer::from_duration(interval), |_, _, state| {
            if let Err(error) = state.redraw() {
                eprintln!("mousr: cannot animate refinement lens: {error}");
                state.lens_animation = None;
            }
            if let Some(animation) = &state.lens_animation {
                TimeoutAction::ToDuration(animation.frame_interval)
            } else {
                state.lens_animation_timer_active = false;
                TimeoutAction::Drop
            }
        }) {
            self.lens_animation = None;
            self.lens_animation_timer_active = false;
            eprintln!("mousr: cannot start refinement animation: {error}");
        }
    }

    fn animated_lens_view(&mut self, current: Option<View>) -> (Option<View>, f32) {
        let Some(current) = current else {
            self.lens_animation = None;
            return (None, 1.0);
        };
        let Some(animation) = &self.lens_animation else {
            return (Some(current), 1.0);
        };
        if animation.target != current {
            self.lens_animation = None;
            return (Some(current), 1.0);
        }
        let (view, progress, finished) = animation.frame(Instant::now());
        if finished {
            self.lens_animation = None;
            (Some(current), 1.0)
        } else {
            (Some(view), progress)
        }
    }

    fn redraw(&mut self) -> Result<(), RenderError> {
        let current_view = match &self.session {
            Session::Grid(grid) => grid.view().cloned(),
            _ => return Ok(()),
        };
        let (animated_view, transition_progress) = self.animated_lens_view(current_view);
        let Session::Grid(grid) = &self.session else {
            return Ok(());
        };
        let hints = if self.config.ui.show_action_hints && grid.selected_tile().is_some() {
            grid_action_hints(&self.config.bindings.grid, grid.can_descend())
        } else {
            Vec::new()
        };
        let mut frames = Vec::new();
        for index in 0..self.overlays.len() {
            let overlay = &self.overlays[index];
            if !overlay.configured
                || !grid
                    .layout()
                    .tiles
                    .iter()
                    .any(|tile| tile.output == overlay.name)
            {
                continue;
            }
            let frame = self.renderer.render_grid(GridRender {
                width: overlay.width,
                height: overlay.height,
                output: &overlay.name,
                layout: grid.layout(),
                view: animated_view.as_ref().or_else(|| grid.view()),
                screenshot: self.grid_screenshots.get(&overlay.name),
                transition_progress,
                prefix: grid.prefix(),
                selected: grid.selected_tile().and_then(|selected| {
                    grid.layout()
                        .tiles
                        .iter()
                        .position(|tile| std::ptr::eq(tile, selected))
                }),
                hints: &hints,
                unmatched: self.config.grid.unmatched,
                unmatched_opacity: self.config.grid.unmatched_opacity,
                ui: &self.config.ui,
            })?;
            frames.push((index, frame));
        }
        for (index, frame) in frames {
            if let Err(error) = self.present(index, &frame.argb8888, frame.width, frame.height) {
                eprintln!("mousr: cannot present overlay: {error}");
            }
        }
        Ok(())
    }

    fn redraw_mode(&mut self) -> Result<(), RenderError> {
        let badge = match self.session {
            Session::Mouse(_) => "MOUSE",
            Session::Scroll => "SCROLL  u/e  y/o",
            _ => return Ok(()),
        };
        let target_output = self.target.as_ref().map(|target| target.0.as_str());
        let focused = Some(self.focused_output.as_str());
        let output_name = target_output.or(focused);
        let Some(index) = self
            .overlays
            .iter()
            .position(|overlay| overlay.configured && Some(overlay.name.as_str()) == output_name)
        else {
            return Ok(());
        };
        let overlay = &self.overlays[index];
        let point = self
            .target
            .as_ref()
            .filter(|target| target.0 == overlay.name)
            .map(|target| (target.1, target.2));
        let hints = match &self.session {
            Session::Mouse(mouse) if self.config.ui.show_action_hints => mouse_action_hints(
                &self.config.bindings.mouse,
                mouse.lock_pending(),
                mouse.locked_button(),
            ),
            _ => Vec::new(),
        };
        let frame = self.renderer.render_mode(
            overlay.width,
            overlay.height,
            badge,
            point,
            &hints,
            &self.config.ui,
        )?;
        if let Err(error) = self.present(index, &frame.argb8888, frame.width, frame.height) {
            eprintln!("mousr: cannot present mode overlay: {error}");
        }
        Ok(())
    }

    fn present(
        &mut self,
        index: usize,
        bytes: &[u8],
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
        canvas.copy_from_slice(bytes);
        let surface = self.overlays[index].layer.wl_surface();
        buffer
            .attach_to(surface)
            .map_err(|error| error.to_string())?;
        surface.damage_buffer(0, 0, width as i32, height as i32);
        self.overlays[index].layer.commit();
        Ok(())
    }

    fn key(&mut self, event: KeyEvent, state: KeyState, repeated: bool) {
        let symbol = input_symbol(event.utf8.as_deref(), event.keysym);
        let raw_code = event.raw_code;
        let previous_grid = match (&self.session, state) {
            (Session::Grid(grid), KeyState::Pressed) => Some((grid.view().cloned(), grid.depth())),
            _ => None,
        };
        let interrupted_animation =
            state == KeyState::Pressed && self.lens_animation.take().is_some();
        let effects = match &mut self.session {
            Session::Grid(grid) if state == KeyState::Pressed => {
                grid.key(&symbol, &self.config.bindings.grid)
            }
            Session::Mouse(mouse) => mouse.key(
                raw_code,
                &symbol,
                state,
                repeated,
                &self.config.bindings.mouse,
            ),
            Session::Scroll if state == KeyState::Pressed => {
                if symbol == self.config.bindings.mouse.cancel {
                    vec![Effect::Exit]
                } else {
                    scroll_key(&symbol, &self.config.bindings.mouse)
                        .map(Effect::Scroll)
                        .into_iter()
                        .collect()
                }
            }
            _ => Vec::new(),
        };
        if let Some((previous_view, previous_depth)) = previous_grid {
            let current_grid = match &self.session {
                Session::Grid(grid) => Some((grid.view().cloned(), grid.depth())),
                _ => None,
            };
            if let Some((current_view, current_depth)) = current_grid
                && current_depth != previous_depth
            {
                self.begin_lens_animation(
                    previous_view,
                    previous_depth,
                    current_view,
                    current_depth,
                );
            }
        }
        let redraws = effects
            .iter()
            .any(|effect| matches!(effect, Effect::Redraw));
        self.apply_effects(effects);
        if interrupted_animation && !redraws && matches!(self.session, Session::Grid(_)) {
            let _ = self.redraw();
        }
        if matches!(self.session, Session::Mouse(_)) {
            self.ensure_motion_timer();
        }
    }

    fn release_mouse_input(&mut self) {
        let effects = match &mut self.session {
            Session::Mouse(mouse) => mouse.release_all(),
            _ => return,
        };
        self.motion_started = None;
        self.last_motion = None;
        self.apply_effects(effects);
    }

    fn keyboard_entered(&mut self) {
        self.keyboard_focused = true;
        self.keyboard_focus_epoch = self.keyboard_focus_epoch.wrapping_add(1);
    }

    fn keyboard_left(&mut self) {
        self.keyboard_focused = false;
        self.keyboard_focus_epoch = self.keyboard_focus_epoch.wrapping_add(1);
        if matches!(self.session, Session::Idle) {
            return;
        }
        if let Session::Mouse(mouse) = &mut self.session {
            mouse.stop_motion();
        }
        self.motion_started = None;
        self.last_motion = None;

        let epoch = self.keyboard_focus_epoch;
        let handle = self.loop_handle.clone();
        if let Err(error) = handle.insert_source(
            Timer::from_duration(FOCUS_LOSS_GRACE),
            move |_, _, state| {
                if !state.keyboard_focused && state.keyboard_focus_epoch == epoch {
                    state.cancel();
                }
                TimeoutAction::Drop
            },
        ) {
            eprintln!("mousr: cannot monitor keyboard focus: {error}");
            self.cancel();
        }
    }

    fn ensure_motion_timer(&mut self) {
        if self.motion_timer_active || !self.mouse_is_moving() {
            return;
        }
        self.motion_timer_active = true;
        self.motion_tick();
        let handle = self.loop_handle.clone();
        let interval = motion_interval(self.config.motion.tick_hz);
        if let Err(error) = handle.insert_source(Timer::from_duration(interval), |_, _, state| {
            state.motion_tick();
            if state.mouse_is_moving() {
                TimeoutAction::ToDuration(motion_interval(state.config.motion.tick_hz))
            } else {
                state.motion_timer_active = false;
                TimeoutAction::Drop
            }
        }) {
            self.motion_timer_active = false;
            eprintln!("mousr: cannot start motion timer: {error}");
        }
    }

    fn mouse_is_moving(&self) -> bool {
        matches!(&self.session, Session::Mouse(mouse) if mouse.vector() != (0, 0))
    }

    fn motion_tick(&mut self) {
        let Session::Mouse(mouse) = &self.session else {
            return;
        };
        let (x, y) = mouse.vector();
        if x == 0 && y == 0 {
            self.motion_started = None;
            self.last_motion = None;
            return;
        }
        let now = Instant::now();
        let started = *self.motion_started.get_or_insert(now);
        let default_tick = motion_interval(self.config.motion.tick_hz);
        let elapsed = self.last_motion.replace(now).map_or(default_tick, |last| {
            now.duration_since(last).min(Duration::from_millis(50))
        });
        let speed = motion_speed(&self.config.motion, now.duration_since(started));
        let distance = motion_distance(speed, elapsed);
        if let Err(error) = self.move_cursor(f64::from(x) * distance, f64::from(y) * distance) {
            eprintln!("mousr: {error}");
            self.cancel();
        }
    }

    fn apply_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let result = match effect {
                Effect::Redraw => self.redraw().map_err(|e| e.to_string()),
                Effect::RedrawMode => self.redraw_mode().map_err(|e| e.to_string()),
                Effect::Warp { output, x, y } => {
                    self.warp(&output, x, y).map_err(|e| e.to_string())
                }
                Effect::Click(button) => self.click(button).map_err(|e| e.to_string()),
                Effect::DoubleClick(button) => self.double_click(button).map_err(|e| e.to_string()),
                Effect::Button { button, state } => {
                    self.button(button, state).map_err(|e| e.to_string())
                }
                Effect::Scroll(direction) => {
                    self.scroll(direction, None).map_err(|e| e.to_string())
                }
                Effect::EnterMouse => {
                    self.activate(Session::Mouse(MouseSession::default()));
                    let _ = self.redraw_mode();
                    Ok(())
                }
                Effect::EnterScroll => {
                    self.activate(Session::Scroll);
                    let _ = self.redraw_mode();
                    Ok(())
                }
                Effect::Exit => {
                    self.cancel();
                    Ok(())
                }
            };
            if let Err(error) = result {
                eprintln!("mousr: {error}");
                self.cancel();
                break;
            }
        }
    }

    fn warp(&mut self, output_name: &str, x: u32, y: u32) -> Result<(), CompositorError> {
        let output = self
            .outputs
            .iter()
            .find(|output| output.name == output_name)
            .ok_or_else(|| CompositorError::Command(format!("output {output_name} disappeared")))?;
        // Grid coordinates are local to the selected output. Sway's cursor
        // command is the only path here that accepts the compositor's global
        // coordinate space, including outputs positioned left of or above the
        // origin. An output-bound virtual pointer maps absolute coordinates
        // relative to its device and therefore cannot reliably warp between
        // outputs in an --scope all grid.
        self.sway.command(&format!(
            "seat {} cursor set {} {}",
            self.seat_name,
            i64::from(output.x) + i64::from(x),
            i64::from(output.y) + i64::from(y)
        ))?;
        self.target = Some((output_name.to_owned(), x, y));
        Ok(())
    }

    fn move_cursor(&self, x: f64, y: f64) -> Result<(), CompositorError> {
        if let Some(pointer) = self.active_pointer() {
            pointer.motion(self.time()?, x, y);
            pointer.frame();
            Ok(())
        } else {
            self.sway.command(&format!(
                "seat {} cursor move {x:.3} {y:.3}",
                self.seat_name
            ))
        }
    }

    fn click(&self, button: MouseButton) -> Result<(), CompositorError> {
        if let Some(pointer) = self.active_pointer() {
            let time = self.time()?;
            let button = button_code(button);
            pointer.button(time, button, wl_pointer::ButtonState::Pressed);
            pointer.button(time, button, wl_pointer::ButtonState::Released);
            pointer.frame();
            Ok(())
        } else {
            self.sway.command(&format!(
                "seat {} cursor press {}; seat {} cursor release {}",
                self.seat_name,
                button_number(button),
                self.seat_name,
                button_number(button)
            ))
        }
    }

    fn double_click(&self, button: MouseButton) -> Result<(), CompositorError> {
        self.click(button)?;
        self.click(button)
    }

    fn button(&self, button: MouseButton, state: KeyState) -> Result<(), CompositorError> {
        if let Some(pointer) = self.active_pointer() {
            let state = match state {
                KeyState::Pressed => wl_pointer::ButtonState::Pressed,
                KeyState::Released => wl_pointer::ButtonState::Released,
            };
            pointer.button(self.time()?, button_code(button), state);
            pointer.frame();
            return Ok(());
        }
        let action = if state == KeyState::Pressed {
            "press"
        } else {
            "release"
        };
        self.sway.command(&format!(
            "seat {} cursor {action} {}",
            self.seat_name,
            button_number(button)
        ))
    }

    fn scroll(&self, direction: Direction, step: Option<f64>) -> Result<(), CompositorError> {
        let configured = match direction {
            Direction::Up | Direction::Down => self.config.scroll.vertical_step,
            Direction::Left | Direction::Right => self.config.scroll.horizontal_step,
        };
        let amount = step.unwrap_or(configured);
        if let Some(pointer) = self.active_pointer() {
            let (axis, sign) = match direction {
                Direction::Up => (wl_pointer::Axis::VerticalScroll, -1.0),
                Direction::Down => (wl_pointer::Axis::VerticalScroll, 1.0),
                Direction::Left => (wl_pointer::Axis::HorizontalScroll, -1.0),
                Direction::Right => (wl_pointer::Axis::HorizontalScroll, 1.0),
            };
            let discrete = (amount / 15.0).round().max(1.0) as i32;
            pointer.axis_source(wl_pointer::AxisSource::Wheel);
            pointer.axis_discrete(self.time()?, axis, sign * amount, sign as i32 * discrete);
            pointer.frame();
            return Ok(());
        }
        let button = match direction {
            Direction::Up => 4,
            Direction::Down => 5,
            Direction::Left => 6,
            Direction::Right => 7,
        };
        let notches = (amount / 15.0).round().max(1.0) as usize;
        let action = format!(
            "seat {} cursor press button{button}; seat {} cursor release button{button}",
            self.seat_name, self.seat_name
        );
        self.sway.command(
            &std::iter::repeat_n(action, notches)
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    fn active_pointer(&self) -> Option<&ZwlrVirtualPointerV1> {
        if let Some(pointer) = &self.relative_pointer {
            return Some(pointer);
        }
        let output_name = self
            .target
            .as_ref()
            .map(|target| target.0.as_str())
            .unwrap_or(&self.focused_output);
        self.outputs
            .iter()
            .find(|output| output.name == output_name)
            .and_then(|output| output.pointer.as_ref())
            .or_else(|| {
                self.outputs
                    .iter()
                    .find_map(|output| output.pointer.as_ref())
            })
    }

    fn time(&self) -> Result<u32, CompositorError> {
        monotonic_time_ms().map_err(CompositorError::Clock)
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

fn monotonic_time_ms() -> Result<u32, std::io::Error> {
    let mut timespec = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut timespec) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(timespec_to_millis(&timespec))
}

fn timespec_to_millis(timespec: &libc::timespec) -> u32 {
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

fn motion_interval(tick_hz: u16) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(tick_hz))
}

fn motion_speed(config: &Motion, held: Duration) -> f64 {
    let range = config.max_speed - config.initial_speed;
    if range <= 0.0 || config.acceleration == 0.0 {
        return config.initial_speed;
    }
    let progress = (config.acceleration * held.as_secs_f64() / range).clamp(0.0, 1.0);
    let eased = match config.curve {
        MotionCurve::Linear => progress,
        MotionCurve::EaseIn => progress * progress,
        MotionCurve::EaseOut => 1.0 - (1.0 - progress).powi(2),
        MotionCurve::EaseInOut if progress < 0.5 => 2.0 * progress * progress,
        MotionCurve::EaseInOut => 1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0,
    };
    config.initial_speed + range * eased
}

fn motion_distance(speed: f64, elapsed: Duration) -> f64 {
    speed * elapsed.as_secs_f64()
}

fn button_number(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "button1",
        MouseButton::Middle => "button2",
        MouseButton::Right => "button3",
    }
}

fn scroll_key(symbol: &str, bindings: &crate::config::MouseBindings) -> Option<Direction> {
    if symbol == bindings.scroll_up {
        Some(Direction::Up)
    } else if symbol == bindings.scroll_down {
        Some(Direction::Down)
    } else if symbol == bindings.scroll_left {
        Some(Direction::Left)
    } else if symbol == bindings.scroll_right {
        Some(Direction::Right)
    } else {
        None
    }
}

fn grid_action_hints(bindings: &crate::config::GridBindings, can_descend: bool) -> Vec<ActionHint> {
    let mut hints = vec![
        ActionHint {
            key: bindings.left_click.clone(),
            action: "Left click",
        },
        ActionHint {
            key: bindings.middle_click.clone(),
            action: "Middle click",
        },
        ActionHint {
            key: bindings.right_click.clone(),
            action: "Right click",
        },
        ActionHint {
            key: bindings.double_click.clone(),
            action: "Double click",
        },
        ActionHint {
            key: bindings.move_only.clone(),
            action: "Move pointer",
        },
        ActionHint {
            key: bindings.enter_mouse.clone(),
            action: "Mouse mode",
        },
        ActionHint {
            key: bindings.scroll_up.clone(),
            action: "Scroll up",
        },
        ActionHint {
            key: bindings.scroll_down.clone(),
            action: "Scroll down",
        },
        ActionHint {
            key: bindings.scroll_left.clone(),
            action: "Scroll left",
        },
        ActionHint {
            key: bindings.scroll_right.clone(),
            action: "Scroll right",
        },
    ];
    if can_descend {
        hints.push(ActionHint {
            key: bindings.descend.clone(),
            action: "Refine grid",
        });
    }
    hints.extend([
        ActionHint {
            key: bindings.back.clone(),
            action: "Back",
        },
        ActionHint {
            key: bindings.cancel.clone(),
            action: "Cancel",
        },
        ActionHint {
            key: "q".into(),
            action: "Quit",
        },
    ]);
    hints
}

fn mouse_action_hints(
    bindings: &crate::config::MouseBindings,
    lock_pending: bool,
    locked_button: Option<MouseButton>,
) -> Vec<ActionHint> {
    let mut hints = vec![ActionHint {
        key: format!(
            "{} {} {} {}",
            bindings.left, bindings.down, bindings.up, bindings.right
        ),
        action: "Move pointer",
    }];
    if let Some(button) = locked_button {
        hints.push(ActionHint {
            key: bindings.button_lock.clone(),
            action: match button {
                MouseButton::Left => "Release left drag",
                MouseButton::Middle => "Release middle drag",
                MouseButton::Right => "Release right drag",
            },
        });
    } else if lock_pending {
        hints.extend([
            ActionHint {
                key: bindings.left_button.clone(),
                action: "Lock left",
            },
            ActionHint {
                key: bindings.middle_button.clone(),
                action: "Lock middle",
            },
            ActionHint {
                key: bindings.right_button.clone(),
                action: "Lock right",
            },
        ]);
    } else {
        hints.extend([
            ActionHint {
                key: format!(
                    "{} / {} {}",
                    bindings.left_button, bindings.button_lock, bindings.left_button
                ),
                action: "Left / lock",
            },
            ActionHint {
                key: format!(
                    "{} / {} {}",
                    bindings.middle_button, bindings.button_lock, bindings.middle_button
                ),
                action: "Middle / lock",
            },
            ActionHint {
                key: format!(
                    "{} / {} {}",
                    bindings.right_button, bindings.button_lock, bindings.right_button
                ),
                action: "Right / lock",
            },
        ]);
    }
    hints.extend([
        ActionHint {
            key: bindings.double_click.clone(),
            action: "Double click",
        },
        ActionHint {
            key: bindings.scroll_up.clone(),
            action: "Scroll up",
        },
        ActionHint {
            key: bindings.scroll_down.clone(),
            action: "Scroll down",
        },
        ActionHint {
            key: bindings.scroll_left.clone(),
            action: "Scroll left",
        },
        ActionHint {
            key: bindings.scroll_right.clone(),
            action: "Scroll right",
        },
        ActionHint {
            key: bindings.cancel.clone(),
            action: "Exit",
        },
        ActionHint {
            key: "q".into(),
            action: "Quit",
        },
    ]);
    hints
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
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.overlays.is_empty() {
            self.reconcile_outputs(qh, "output added");
        }
    }
    fn update_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.overlays.is_empty() {
            self.reconcile_outputs(qh, "output changed");
        }
    }
    fn output_destroyed(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.overlays.is_empty() {
            self.reconcile_outputs(qh, "output removed");
        }
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let known = self
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
            .overlays
            .iter_mut()
            .find(|overlay| overlay.layer.wl_surface() == layer.wl_surface())
        {
            overlay.configured = true;
            debug!(target: "mousr::wayland", "overlay configured output={}", overlay.name);
        }
        match self.session {
            Session::Grid(_) => {
                let _ = self.redraw();
            }
            Session::Mouse(_) | Session::Scroll => {
                let _ = self.redraw_mode();
            }
            Session::Idle => {}
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        let matches = self.seat_state.info(&seat).is_some_and(|info| {
            info.has_keyboard && info.name.as_deref().unwrap_or("seat0") == self.seat_name
        });
        if matches
            && self.seat.is_none()
            && !self.overlays.is_empty()
            && let Err(error) = self.bind_keyboard(qh, &seat, self.loop_handle.clone())
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
            || !self
                .seat_state
                .info(&seat)
                .is_some_and(|info| info.name.as_deref().unwrap_or("seat0") == self.seat_name)
        {
            return;
        }
        let selected_seat = self.seat.as_ref().is_some_and(|selected| selected == &seat);
        if (selected_seat || (self.keyboard.is_none() && !self.overlays.is_empty()))
            && let Err(error) = self.bind_keyboard(qh, &seat, self.loop_handle.clone())
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
            self.keyboard = None;
            self.cancel();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.keyboard = None;
        self.seat = None;
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
        self.key(event, KeyState::Pressed, false);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, KeyState::Pressed, true);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, KeyState::Released, false);
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
        &mut self.shm
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
                    .pending_captures
                    .iter()
                    .position(|capture| &capture.frame == frame)
                else {
                    return;
                };
                let (format, width, height, stride) = {
                    let capture = &state.pending_captures[index];
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
                let buffer = state.capture_pool.create_buffer(
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
                        state.pending_captures[index].buffer = Some(buffer);
                    }
                    Err(error) => {
                        eprintln!("mousr: cannot allocate screencopy buffer: {error}");
                        state.finish_capture(frame, None);
                    }
                }
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                if let Some(capture) = state
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
                    .pending_captures
                    .iter()
                    .position(|capture| &capture.frame == frame)
                    .and_then(|index| {
                        screenshot_from_capture(
                            &mut state.capture_pool,
                            &state.pending_captures[index],
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
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => state.shortcuts_active = true,
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                state.shortcuts_active = false;
                state.release_mouse_input();
                if state.config.general.require_shortcut_inhibit {
                    state.cancel();
                }
            }
            _ => {}
        }
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
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
    fn lens_rectangle_interpolates_position_and_size() {
        assert_eq!(
            interpolate_rect(
                Rect {
                    x: 100,
                    y: 50,
                    width: 100,
                    height: 50,
                },
                Rect {
                    x: 0,
                    y: 0,
                    width: 400,
                    height: 200,
                },
                0.5,
            ),
            Rect {
                x: 50,
                y: 25,
                width: 250,
                height: 125,
            }
        );
    }

    #[test]
    fn lens_animation_uses_fewer_frames_on_large_outputs() {
        assert_eq!(lens_animation_steps(1920 * 1080), 8);
        assert_eq!(lens_animation_steps(3840 * 2160), 6);
        assert_eq!(lens_animation_steps(7680 * 4320), 4);
    }

    #[test]
    fn monotonic_timestamp_converts_to_milliseconds() {
        let timespec = libc::timespec {
            tv_sec: 12,
            tv_nsec: 345_678_901,
        };

        assert_eq!(timespec_to_millis(&timespec), 12_345);
    }

    #[test]
    fn monotonic_timestamp_uses_wayland_u32_representation() {
        let timespec = libc::timespec {
            tv_sec: (u64::from(u32::MAX) + 1_001) as libc::time_t,
            tv_nsec: 0,
        };

        assert_eq!(timespec_to_millis(&timespec), 1_000_000);
    }

    #[test]
    fn hints_use_configured_bindings() {
        let grid = crate::config::GridBindings {
            left_click: "x".into(),
            ..crate::config::GridBindings::default()
        };
        let grid_hints = grid_action_hints(&grid, false);
        assert!(grid_hints.iter().any(|hint| hint.key == "x"));
        assert!(
            grid_hints
                .iter()
                .any(|hint| hint.key == grid.double_click && hint.action == "Double click")
        );
        assert!(!grid_hints.iter().any(|hint| hint.action == "Refine grid"));

        let mouse = crate::config::MouseBindings {
            left: "a".into(),
            ..crate::config::MouseBindings::default()
        };
        let mouse_hints = mouse_action_hints(&mouse, false, None);
        assert_eq!(mouse_hints[0].key, "a j k l");
        assert_eq!(mouse_hints[1].key, "s / v s");
        assert!(
            mouse_hints
                .iter()
                .any(|hint| hint.key == mouse.double_click && hint.action == "Double click")
        );

        let pending_hints = mouse_action_hints(&mouse, true, None);
        assert_eq!(pending_hints[1].key, "s");
        assert_eq!(pending_hints[1].action, "Lock left");

        let locked_hints = mouse_action_hints(&mouse, false, Some(MouseButton::Right));
        assert_eq!(locked_hints[1].key, "v");
        assert_eq!(locked_hints[1].action, "Release right drag");
    }

    #[test]
    fn motion_rate_controls_timer_interval() {
        assert_eq!(motion_interval(100), Duration::from_millis(10));
    }

    #[test]
    fn linear_curve_preserves_constant_acceleration() {
        let motion = Motion {
            initial_speed: 60.0,
            acceleration: 100.0,
            max_speed: 160.0,
            tick_hz: 100,
            curve: MotionCurve::Linear,
        };
        assert_eq!(motion_speed(&motion, Duration::from_millis(500)), 110.0);
    }

    #[test]
    fn curves_have_expected_early_ramp_order() {
        let speed = |curve| {
            motion_speed(
                &Motion {
                    initial_speed: 0.0,
                    acceleration: 100.0,
                    max_speed: 100.0,
                    tick_hz: 100,
                    curve,
                },
                Duration::from_millis(250),
            )
        };
        assert!(speed(MotionCurve::EaseIn) < speed(MotionCurve::Linear));
        assert!(speed(MotionCurve::Linear) < speed(MotionCurve::EaseOut));
        assert!(speed(MotionCurve::EaseInOut) < speed(MotionCurve::Linear));
    }

    #[test]
    fn fractional_motion_is_not_rounded_up() {
        let distance = motion_distance(60.0, Duration::from_secs_f64(1.0 / 120.0));
        assert!((distance - 0.5).abs() < 1e-6);
    }
}
