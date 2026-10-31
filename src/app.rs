use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use log::{debug, warn};

use crate::{
    animation::{LensAnimation, lens_animation_steps},
    backend::{self, BackendResult, DisplayBackend, Timer},
    cli::{Command, Direction, GridOptions},
    config::Config,
    grid::{Rect, Region},
    mode::{Effect, GridSession, GridTransition, KeyInput, KeyState, MouseSession, Session},
    motion::{motion_distance, motion_interval, motion_speed},
    render::{self, Renderer, Screenshot},
};

const FOCUS_LOSS_GRACE: Duration = Duration::from_millis(50);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(1);

struct Capture {
    id: u64,
    output: String,
    transition: GridTransition,
}

#[cfg(test)]
mod tests;

pub struct App<D> {
    pub(crate) display: D,
    config: Config,
    config_path: Option<PathBuf>,
    renderer: Renderer,
    session: Session,
    focused_output: String,
    target: Option<(String, u32, u32)>,
    screenshots: HashMap<String, Screenshot>,
    capture: Option<Capture>,
    capture_id: u64,
    lens_animation: Option<LensAnimation>,
    animation_timer_active: bool,
    motion_started: Option<Instant>,
    last_motion: Option<Instant>,
    motion_timer_active: bool,
    keyboard_focused: bool,
    focus_epoch: u64,
}

impl<D: DisplayBackend> App<D> {
    pub fn new(display: D, config: Config, config_path: Option<PathBuf>) -> BackendResult<Self> {
        if config.general.require_shortcut_inhibit && !display.can_inhibit_shortcuts() {
            return Err("required keyboard shortcut inhibition is unavailable".into());
        }
        let (renderer, warning) = Renderer::new(&config.ui)?;
        if let Some(warning) = warning {
            eprintln!("mousr: {warning}");
        }
        Ok(Self {
            display,
            config,
            config_path,
            renderer,
            session: Session::Idle,
            focused_output: String::new(),
            target: None,
            screenshots: HashMap::new(),
            capture: None,
            capture_id: 0,
            lens_animation: None,
            animation_timer_active: false,
            motion_started: None,
            last_motion: None,
            motion_timer_active: false,
            keyboard_focused: false,
            focus_epoch: 0,
        })
    }

    pub fn command(&mut self, command: Command) -> BackendResult<&'static str> {
        debug!(target: "mousr::app", "dispatching command={command}");
        let result = self.try_command(command);
        if result.is_err() {
            self.cancel();
        }
        result
    }

    fn try_command(&mut self, command: Command) -> BackendResult<&'static str> {
        match command {
            Command::Reload => {
                let config = Config::load(self.config_path.as_deref())?;
                let (renderer, warning) = Renderer::new(&config.ui)?;
                if let Some(warning) = warning {
                    eprintln!("mousr: {warning}");
                }
                self.config = config;
                self.renderer = renderer;
                self.redraw()?;
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
                self.activate(Session::Mouse(MouseSession::default()))?;
                self.redraw()?;
                Ok("mouse mode active")
            }
            Command::Click(button) => {
                self.refresh_focused_output()?;
                self.cancel();
                self.display.click(button)?;
                Ok("clicked")
            }
            Command::Scroll { direction, step } => {
                self.refresh_focused_output()?;
                self.cancel();
                self.scroll(direction, step)?;
                Ok("scrolled")
            }
        }
    }

    fn refresh_focused_output(&mut self) -> BackendResult {
        let focused = self.display.focused_output()?;
        if !self.display.outputs().any(|output| output.name == focused) {
            return Err(format!("focused output {focused:?} is not active").into());
        }
        self.focused_output = focused;
        Ok(())
    }

    fn start_grid(&mut self, options: GridOptions) -> BackendResult {
        self.refresh_focused_output()?;
        let outputs: Vec<_> = self
            .display
            .outputs()
            .map(|output| Region {
                output: output.name.into(),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: output.width,
                    height: output.height,
                },
            })
            .collect();
        let regions = backend::select_regions(
            &outputs,
            &self.focused_output,
            &options,
            self.config.general.scope,
        )?;
        let can_capture = self.display.can_capture();
        let session = GridSession::from_config(&regions, &options, &self.config, can_capture)?;
        if self.config.grid.refinement_zoom > 1.0 && !can_capture {
            warn!(target: "mousr::app", "screen capture unavailable; continuing without magnification");
        }
        self.target = None;
        self.activate(Session::Grid(session))?;
        self.redraw()
    }

    fn activate(&mut self, session: Session) -> BackendResult {
        self.cancel();
        self.session = session;
        let eligible = |name: &str| match &self.session {
            Session::Grid(grid) => grid.layout().tiles.iter().any(|tile| tile.output == name),
            Session::Mouse(_) | Session::Scroll => name == self.focused_output,
            Session::Idle => false,
        };
        let output = self
            .display
            .outputs()
            .filter(|output| output.ready && eligible(output.name))
            .min_by_key(|output| output.name != self.focused_output)
            .map(|output| output.name.to_owned());
        let output = output.ok_or("no configured output is available for keyboard input")?;
        self.display.focus(&output)
    }

    pub fn cancel(&mut self) {
        self.focus_epoch = self.focus_epoch.wrapping_add(1);
        self.capture = None;
        self.display.cancel_captures();
        self.screenshots.clear();
        self.lens_animation = None;
        self.motion_started = None;
        self.last_motion = None;
        let previous = std::mem::replace(&mut self.session, Session::Idle);
        if let Session::Mouse(mut mouse) = previous {
            for effect in mouse.cancel() {
                if let Effect::Button { button, state } = effect
                    && let Err(error) = self.display.button(button, state)
                {
                    eprintln!("mousr: cannot release mouse button: {error}");
                }
            }
        }
        if let Err(error) = self.display.park() {
            eprintln!("mousr: cannot park overlays: {error}");
        }
    }

    pub fn key(&mut self, input: KeyInput<'_>) {
        if self.capture.is_some() {
            if input.state == KeyState::Pressed
                && (input.symbol == self.config.bindings.grid.cancel || input.symbol == "q")
            {
                self.cancel();
            }
            return;
        }
        let interrupted = input.state == KeyState::Pressed && self.lens_animation.take().is_some();
        let outcome = self.session.key(
            input,
            &self.config.bindings.grid,
            &self.config.bindings.mouse,
        );
        if let Some(transition) = outcome.grid_transition {
            if transition.current_depth > transition.previous_depth
                && transition.current_view.is_some()
                && self.display.can_capture()
            {
                self.begin_capture(transition);
            } else {
                self.begin_lens_animation(transition);
            }
        }
        let redraws = outcome
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Redraw));
        self.apply_effects(outcome.effects);
        if interrupted && !redraws {
            self.check_redraw();
        }
        self.ensure_motion_timer();
    }

    fn begin_capture(&mut self, transition: GridTransition) {
        let Some(view) = transition.current_view.as_ref() else {
            return;
        };
        let output = view.output.clone();
        self.capture_id = self.capture_id.wrapping_add(1);
        let id = self.capture_id;
        self.capture = Some(Capture {
            id,
            output: output.clone(),
            transition,
        });
        let result = self
            .display
            .capture(id, &output)
            .and_then(|()| self.display.schedule(Timer::Capture(id), CAPTURE_TIMEOUT));
        if let Err(error) = result {
            warn!(target: "mousr::app", "cannot refresh screenshot: {error}");
            self.captured(id, &output, None);
        }
    }

    pub fn captured(&mut self, id: u64, output: &str, screenshot: Option<Screenshot>) {
        if !self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.id == id && capture.output == output)
        {
            return;
        }
        let capture = self.capture.take().expect("capture identity was checked");
        self.display.cancel_captures();
        self.screenshots.clear();
        if let Some(screenshot) = screenshot {
            self.screenshots.insert(output.to_owned(), screenshot);
            self.begin_lens_animation(capture.transition);
        } else {
            // A magnified grid without a matching image would select different pixels than it shows.
            if let Session::Grid(grid) = &mut self.session {
                grid.unmagnify();
            }
            self.lens_animation = None;
            warn!(target: "mousr::app", "fresh screenshot unavailable; continuing without magnification");
        }
        self.check_redraw();
    }

    fn begin_lens_animation(&mut self, transition: GridTransition) {
        let Some(target) = transition.current_view else {
            self.lens_animation = None;
            return;
        };
        let duration = Duration::from_millis(u64::from(self.config.ui.lens_animation_ms));
        if duration.is_zero() {
            self.lens_animation = None;
            return;
        }
        let from = if transition.current_depth > transition.previous_depth {
            transition
                .previous_view
                .as_ref()
                .map_or(target.source, |view| view.map(target.source))
        } else {
            transition
                .previous_view
                .map_or(target.source, |view| view.destination)
        };
        if from == target.destination {
            self.lens_animation = None;
            return;
        }
        let pixels = self
            .display
            .outputs()
            .find(|output| output.name == target.output)
            .map_or(0, |output| {
                u64::from(output.width) * u64::from(output.height)
            });
        let interval = (duration / lens_animation_steps(pixels)).max(Duration::from_millis(8));
        self.lens_animation = Some(LensAnimation {
            from,
            target,
            started: Instant::now(),
            duration,
            frame_interval: interval,
        });
        if !self.animation_timer_active {
            match self.display.schedule(Timer::Animation, interval) {
                Ok(()) => self.animation_timer_active = true,
                Err(error) => {
                    self.lens_animation = None;
                    eprintln!("mousr: cannot animate refinement: {error}");
                }
            }
        }
    }

    pub fn redraw(&mut self) -> BackendResult {
        // Configure and animation callbacks must not expose an overlay in a pending capture.
        if self.capture.is_some() {
            return Ok(());
        }
        match &self.session {
            Session::Grid(grid) => {
                let current = grid.view().cloned();
                let (view, progress) = if let Some(animation) = &self.lens_animation {
                    if current.as_ref() == Some(&animation.target) {
                        let (view, progress, finished) = animation.frame(Instant::now());
                        if finished {
                            self.lens_animation = None;
                            (current, 1.0)
                        } else {
                            (Some(view), progress)
                        }
                    } else {
                        self.lens_animation = None;
                        (current, 1.0)
                    }
                } else {
                    (current, 1.0)
                };
                let outputs: Vec<_> = self
                    .display
                    .outputs()
                    .filter(|output| output.ready)
                    .map(|output| (output.name.to_owned(), output.width, output.height))
                    .collect();
                let frames = render::grid_frames(
                    &mut self.renderer,
                    grid,
                    outputs
                        .iter()
                        .enumerate()
                        .map(|(i, (name, w, h))| (i, name.as_str(), *w, *h)),
                    &self.screenshots,
                    view.as_ref(),
                    progress,
                    &self.config,
                )?;
                for (index, frame) in frames {
                    self.display.present(&outputs[index].0, &frame)?;
                }
            }
            Session::Mouse(_) | Session::Scroll => {
                let name = self
                    .target
                    .as_ref()
                    .map_or(self.focused_output.as_str(), |target| target.0.as_str());
                let geometry = self
                    .display
                    .outputs()
                    .find(|output| output.ready && output.name == name)
                    .map(|output| (output.width, output.height));
                if let Some((width, height)) = geometry {
                    let point = self.target.as_ref().map(|target| (target.1, target.2));
                    if let Some(frame) = render::mode_frame(
                        &mut self.renderer,
                        &self.session,
                        width,
                        height,
                        point,
                        &self.config,
                    )? {
                        self.display.present(name, &frame)?;
                    }
                }
            }
            Session::Idle => {}
        }
        Ok(())
    }

    fn check_redraw(&mut self) {
        if let Err(error) = self.redraw() {
            eprintln!("mousr: cannot redraw: {error}");
            self.cancel();
        }
    }

    pub fn release_mouse_input(&mut self) {
        let effects = match &mut self.session {
            Session::Mouse(mouse) => mouse.release_all(),
            _ => return,
        };
        self.motion_started = None;
        self.last_motion = None;
        self.apply_effects(effects);
    }

    pub fn shortcuts_lost(&mut self) {
        self.release_mouse_input();
        if self.config.general.require_shortcut_inhibit {
            self.cancel();
        }
    }

    pub fn keyboard_entered(&mut self) {
        self.keyboard_focused = true;
        self.focus_epoch = self.focus_epoch.wrapping_add(1);
    }

    pub fn keyboard_left(&mut self) {
        self.keyboard_focused = false;
        self.focus_epoch = self.focus_epoch.wrapping_add(1);
        if matches!(self.session, Session::Idle) {
            return;
        }
        if let Session::Mouse(mouse) = &mut self.session {
            mouse.stop_motion();
        }
        self.motion_started = None;
        self.last_motion = None;
        if let Err(error) = self
            .display
            .schedule(Timer::Focus(self.focus_epoch), FOCUS_LOSS_GRACE)
        {
            eprintln!("mousr: cannot monitor keyboard focus: {error}");
            self.cancel();
        }
    }

    pub fn timer(&mut self, timer: Timer) -> Option<Duration> {
        match timer {
            Timer::Focus(epoch) => {
                if !self.keyboard_focused && self.focus_epoch == epoch {
                    self.cancel();
                }
                None
            }
            Timer::Capture(id) => {
                if let Some(capture) = &self.capture
                    && capture.id == id
                {
                    let output = capture.output.clone();
                    self.captured(id, &output, None);
                }
                None
            }
            Timer::Animation => {
                self.check_redraw();
                let interval = self
                    .lens_animation
                    .as_ref()
                    .map(|animation| animation.frame_interval);
                self.animation_timer_active = interval.is_some();
                interval
            }
            Timer::Motion => {
                self.motion_tick();
                self.motion_timer_active = self.mouse_is_moving();
                self.motion_timer_active
                    .then(|| motion_interval(self.config.motion.tick_hz))
            }
        }
    }

    fn mouse_is_moving(&self) -> bool {
        matches!(&self.session, Session::Mouse(mouse) if mouse.vector() != (0, 0))
    }

    fn ensure_motion_timer(&mut self) {
        if self.motion_timer_active || !self.mouse_is_moving() {
            return;
        }
        self.motion_tick();
        match self
            .display
            .schedule(Timer::Motion, motion_interval(self.config.motion.tick_hz))
        {
            Ok(()) => self.motion_timer_active = true,
            Err(error) => {
                eprintln!("mousr: cannot start motion timer: {error}");
                self.cancel();
            }
        }
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
        let elapsed = self
            .last_motion
            .replace(now)
            .map_or(motion_interval(self.config.motion.tick_hz), |last| {
                now.duration_since(last).min(Duration::from_millis(50))
            });
        let distance = motion_distance(
            motion_speed(&self.config.motion, now.duration_since(started)),
            elapsed,
        );
        if let Err(error) = self
            .display
            .move_cursor(f64::from(x) * distance, f64::from(y) * distance)
        {
            eprintln!("mousr: {error}");
            self.cancel();
        }
    }

    fn scroll(&mut self, direction: Direction, step: Option<f64>) -> BackendResult {
        let configured = match direction {
            Direction::Up | Direction::Down => self.config.scroll.vertical_step,
            Direction::Left | Direction::Right => self.config.scroll.horizontal_step,
        };
        self.display.scroll(direction, step.unwrap_or(configured))
    }

    fn apply_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let result = match effect {
                Effect::Redraw | Effect::RedrawMode => self.redraw(),
                Effect::Warp { output, x, y } => self.display.warp(&output, x, y).map(|()| {
                    self.target = Some((output, x, y));
                }),
                Effect::Click(button) => self.display.click(button),
                Effect::DoubleClick(button) => self
                    .display
                    .click(button)
                    .and_then(|()| self.display.click(button)),
                Effect::Button { button, state } => self.display.button(button, state),
                Effect::Scroll(direction) => self.scroll(direction, None),
                Effect::EnterMouse => self
                    .activate(Session::Mouse(MouseSession::default()))
                    .and_then(|()| self.redraw()),
                Effect::EnterScroll => self.activate(Session::Scroll).and_then(|()| self.redraw()),
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
}
