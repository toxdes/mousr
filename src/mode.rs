use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cli::{Direction, GridAction, MouseButton},
    config::{GridBindings, MouseBindings},
    grid::{self, Layout, Settings, Tile},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Redraw,
    Warp {
        output: String,
        x: u32,
        y: u32,
    },
    Click(MouseButton),
    Button {
        button: MouseButton,
        state: KeyState,
    },
    Scroll(Direction),
    RedrawMode,
    EnterMouse,
    EnterScroll,
    Exit,
}

#[derive(Debug, Clone)]
pub struct GridSession {
    levels: Vec<Layout>,
    prefix: String,
    selected: Option<usize>,
    settings: Settings,
    max_depth: u8,
    auto_descend: bool,
    fixed_action: GridAction,
}

impl GridSession {
    pub fn new(
        layout: Layout,
        settings: Settings,
        max_depth: u8,
        auto_descend: bool,
        fixed_action: GridAction,
    ) -> Self {
        Self {
            levels: vec![layout],
            prefix: String::new(),
            selected: None,
            settings,
            max_depth,
            auto_descend,
            fixed_action,
        }
    }

    pub fn layout(&self) -> &Layout {
        self.levels
            .last()
            .expect("a grid session always has a root level")
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn selected_tile(&self) -> Option<&Tile> {
        self.selected
            .and_then(|index| self.layout().tiles.get(index))
    }

    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    pub fn can_descend(&self) -> bool {
        if self.depth() >= usize::from(self.max_depth) {
            return false;
        }
        self.selected_tile()
            .and_then(|tile| grid::descend(tile, self.settings).ok().flatten())
            .is_some()
    }

    pub fn key(&mut self, symbol: &str, bindings: &GridBindings) -> Vec<Effect> {
        if symbol == bindings.cancel {
            return vec![Effect::Exit];
        }
        if symbol == bindings.back {
            return self.back();
        }
        if self.selected.is_some() {
            return self.selected_key(symbol, bindings);
        }
        if symbol.chars().count() == 1
            && symbol
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_lowercase)
        {
            return self.push_label(symbol);
        }
        Vec::new()
    }

    fn push_label(&mut self, symbol: &str) -> Vec<Effect> {
        self.prefix.push_str(symbol);
        let matching: Vec<usize> = self
            .layout()
            .matching(&self.prefix)
            .map(|(index, _)| index)
            .collect();
        if matching.is_empty() {
            self.prefix.pop();
            return Vec::new();
        }
        if self.prefix.len() == usize::from(self.layout().label_length) {
            self.selected = self.layout().exact(&self.prefix);
            if self.auto_descend && self.try_descend() {
                return vec![Effect::Redraw];
            }
            if self.fixed_action != GridAction::Choose {
                return self.apply_action(self.fixed_action);
            }
        }
        vec![Effect::Redraw]
    }

    fn selected_key(&mut self, symbol: &str, bindings: &GridBindings) -> Vec<Effect> {
        let action = if symbol == bindings.left_click {
            Some(GridAction::Left)
        } else if symbol == bindings.middle_click {
            Some(GridAction::Middle)
        } else if symbol == bindings.right_click {
            Some(GridAction::Right)
        } else if symbol == bindings.scroll_up {
            Some(GridAction::ScrollUp)
        } else if symbol == bindings.scroll_down {
            Some(GridAction::ScrollDown)
        } else if symbol == bindings.scroll_left {
            Some(GridAction::ScrollLeft)
        } else if symbol == bindings.scroll_right {
            Some(GridAction::ScrollRight)
        } else if symbol == bindings.enter_mouse {
            Some(GridAction::Mouse)
        } else if symbol == bindings.move_only {
            Some(GridAction::Move)
        } else {
            None
        };
        if let Some(action) = action {
            return self.apply_action(action);
        }
        if symbol == bindings.descend && self.try_descend() {
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    fn try_descend(&mut self) -> bool {
        if self.depth() >= usize::from(self.max_depth) {
            return false;
        }
        let Some(tile) = self.selected_tile().cloned() else {
            return false;
        };
        let Ok(Some(layout)) = grid::descend(&tile, self.settings) else {
            return false;
        };
        self.levels.push(layout);
        self.prefix.clear();
        self.selected = None;
        true
    }

    fn back(&mut self) -> Vec<Effect> {
        if self.selected.take().is_some() {
            self.prefix.pop();
            return vec![Effect::Redraw];
        }
        if !self.prefix.is_empty() {
            self.prefix.pop();
            return vec![Effect::Redraw];
        }
        if self.levels.len() > 1 {
            self.levels.pop();
            self.prefix.clear();
            self.selected = None;
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    fn apply_action(&self, action: GridAction) -> Vec<Effect> {
        let Some(tile) = self.selected_tile() else {
            return Vec::new();
        };
        let (x, y) = tile.bounds.center();
        let mut effects = vec![Effect::Warp {
            output: tile.output.clone(),
            x,
            y,
        }];
        match action {
            GridAction::Choose => return Vec::new(),
            GridAction::Move => effects.push(Effect::Exit),
            GridAction::Mouse => effects.push(Effect::EnterMouse),
            GridAction::Left => effects.extend([Effect::Click(MouseButton::Left), Effect::Exit]),
            GridAction::Middle => {
                effects.extend([Effect::Click(MouseButton::Middle), Effect::Exit])
            }
            GridAction::Right => effects.extend([Effect::Click(MouseButton::Right), Effect::Exit]),
            GridAction::Scroll => effects.push(Effect::EnterScroll),
            GridAction::ScrollUp => effects.extend([Effect::Scroll(Direction::Up), Effect::Exit]),
            GridAction::ScrollDown => {
                effects.extend([Effect::Scroll(Direction::Down), Effect::Exit])
            }
            GridAction::ScrollLeft => {
                effects.extend([Effect::Scroll(Direction::Left), Effect::Exit])
            }
            GridAction::ScrollRight => {
                effects.extend([Effect::Scroll(Direction::Right), Effect::Exit])
            }
        }
        effects
    }
}

#[derive(Debug, Default)]
pub struct MouseSession {
    directions: BTreeMap<u32, DirectionKey>,
    buttons: BTreeMap<u32, ButtonKey>,
    lock_pending: bool,
    locked_button: Option<ButtonKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DirectionKey {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ButtonKey {
    Left,
    Middle,
    Right,
}

impl MouseSession {
    pub fn key(
        &mut self,
        raw_code: u32,
        symbol: &str,
        state: KeyState,
        repeated: bool,
        bindings: &MouseBindings,
    ) -> Vec<Effect> {
        if symbol == bindings.cancel && state == KeyState::Pressed {
            return self.cancel();
        }
        if state == KeyState::Released {
            self.directions.remove(&raw_code);
            let Some(button) = self.buttons.remove(&raw_code) else {
                return Vec::new();
            };
            return (!self.button_is_down(button))
                .then_some(Effect::Button {
                    button: mouse_button(button),
                    state,
                })
                .into_iter()
                .collect();
        }
        if symbol == bindings.button_lock {
            if repeated {
                return Vec::new();
            }
            let mut effects = Vec::new();
            if let Some(button) = self.locked_button.take() {
                if !self.button_is_down(button) {
                    effects.push(Effect::Button {
                        button: mouse_button(button),
                        state: KeyState::Released,
                    });
                }
            } else {
                self.lock_pending = !self.lock_pending;
            }
            effects.push(Effect::RedrawMode);
            return effects;
        }
        let mut effects = Vec::new();
        if self.lock_pending {
            self.lock_pending = false;
            if let Some((button, _)) = button_binding(symbol, bindings) {
                let was_down = self.button_is_down(button);
                self.locked_button = Some(button);
                if !was_down {
                    effects.push(Effect::Button {
                        button: mouse_button(button),
                        state: KeyState::Pressed,
                    });
                }
                effects.push(Effect::RedrawMode);
                return effects;
            }
            effects.push(Effect::RedrawMode);
        }
        if let Some(direction) = movement_binding(symbol, bindings) {
            self.directions.insert(raw_code, direction);
            return effects;
        }
        if let Some((key, button)) = button_binding(symbol, bindings) {
            if repeated {
                return effects;
            }
            let already_held = self.button_is_down(key);
            let inserted = self.buttons.insert(raw_code, key).is_none();
            if inserted && !already_held {
                effects.push(Effect::Button { button, state });
            }
            return effects;
        }
        if let Some(direction) = scroll_binding(symbol, bindings) {
            effects.push(Effect::Scroll(direction));
        }
        effects
    }

    pub fn vector(&self) -> (i8, i8) {
        let contains = |direction| self.directions.values().any(|held| *held == direction);
        let x = i8::from(contains(DirectionKey::Right)) - i8::from(contains(DirectionKey::Left));
        let y = i8::from(contains(DirectionKey::Down)) - i8::from(contains(DirectionKey::Up));
        (x, y)
    }

    pub fn release_all(&mut self) -> Vec<Effect> {
        let mut buttons = self.buttons.values().copied().collect::<BTreeSet<_>>();
        buttons.extend(self.locked_button);
        let effects = buttons
            .into_iter()
            .map(|button| Effect::Button {
                button: mouse_button(button),
                state: KeyState::Released,
            })
            .collect();
        self.buttons.clear();
        self.directions.clear();
        self.lock_pending = false;
        self.locked_button = None;
        effects
    }

    pub fn lock_pending(&self) -> bool {
        self.lock_pending
    }

    pub fn locked_button(&self) -> Option<MouseButton> {
        self.locked_button.map(mouse_button)
    }

    fn button_is_down(&self, button: ButtonKey) -> bool {
        self.locked_button == Some(button) || self.buttons.values().any(|held| *held == button)
    }

    pub fn cancel(&mut self) -> Vec<Effect> {
        let mut effects = self.release_all();
        effects.push(Effect::Exit);
        effects
    }
}

fn mouse_button(button: ButtonKey) -> MouseButton {
    match button {
        ButtonKey::Left => MouseButton::Left,
        ButtonKey::Middle => MouseButton::Middle,
        ButtonKey::Right => MouseButton::Right,
    }
}

fn movement_binding(symbol: &str, bindings: &MouseBindings) -> Option<DirectionKey> {
    if symbol == bindings.left {
        Some(DirectionKey::Left)
    } else if symbol == bindings.down {
        Some(DirectionKey::Down)
    } else if symbol == bindings.up {
        Some(DirectionKey::Up)
    } else if symbol == bindings.right {
        Some(DirectionKey::Right)
    } else {
        None
    }
}

fn button_binding(symbol: &str, bindings: &MouseBindings) -> Option<(ButtonKey, MouseButton)> {
    if symbol == bindings.left_button {
        Some((ButtonKey::Left, MouseButton::Left))
    } else if symbol == bindings.middle_button {
        Some((ButtonKey::Middle, MouseButton::Middle))
    } else if symbol == bindings.right_button {
        Some((ButtonKey::Right, MouseButton::Right))
    } else {
        None
    }
}

fn scroll_binding(symbol: &str, bindings: &MouseBindings) -> Option<Direction> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Rect, Region};

    fn grid_session() -> GridSession {
        let settings = Settings {
            min_tile_width: 24,
            min_tile_height: 24,
            max_label_length: 1,
            max_cells: 4,
        };
        let layout = grid::build(
            &[Region {
                output: "DP-1".into(),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 100,
                },
            }],
            settings,
        )
        .unwrap();
        GridSession::new(layout, settings, 2, false, GridAction::Choose)
    }

    #[test]
    fn grid_click_warps_before_clicking() {
        let mut session = grid_session();
        session.key("a", &GridBindings::default());
        let effects = session.key("s", &GridBindings::default());
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Warp { .. },
                Effect::Click(MouseButton::Left),
                Effect::Exit
            ]
        ));
    }

    #[test]
    fn grid_can_transition_to_mouse_mode() {
        let mut session = grid_session();
        session.key("a", &GridBindings::default());
        let effects = session.key("g", &GridBindings::default());
        assert!(matches!(
            effects.as_slice(),
            [Effect::Warp { .. }, Effect::EnterMouse]
        ));
    }

    #[test]
    fn invalid_grid_label_does_not_change_state() {
        let mut session = grid_session();
        let effects = session.key("z", &GridBindings::default());
        assert!(effects.is_empty());
        assert!(session.prefix().is_empty());
        assert!(session.selected_tile().is_none());
    }

    #[test]
    fn escape_exits_grid_after_selection() {
        let mut session = grid_session();
        session.key("a", &GridBindings::default());
        assert_eq!(
            session.key("Escape", &GridBindings::default()),
            vec![Effect::Exit]
        );
    }

    #[test]
    fn refine_hint_tracks_available_depth() {
        let mut session = grid_session();
        assert!(!session.can_descend());
        session.key("a", &GridBindings::default());
        assert!(session.can_descend());
        session.key("Return", &GridBindings::default());
        assert_eq!(session.depth(), 2);
        assert!(!session.can_descend());
    }

    #[test]
    fn held_button_is_released_on_cancel() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        assert_eq!(
            session.key(31, "s", KeyState::Pressed, false, &bindings),
            vec![Effect::Button {
                button: MouseButton::Left,
                state: KeyState::Pressed
            }]
        );
        assert_eq!(
            session.cancel(),
            vec![
                Effect::Button {
                    button: MouseButton::Left,
                    state: KeyState::Released
                },
                Effect::Exit
            ]
        );
    }

    #[test]
    fn opposite_motion_keys_cancel() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        session.key(35, "h", KeyState::Pressed, false, &bindings);
        session.key(38, "l", KeyState::Pressed, false, &bindings);
        assert_eq!(session.vector(), (0, 0));
    }

    #[test]
    fn horizontal_scroll_is_emitted() {
        let effects = MouseSession::default().key(
            21,
            "y",
            KeyState::Pressed,
            false,
            &MouseBindings::default(),
        );
        assert_eq!(effects, vec![Effect::Scroll(Direction::Left)]);
    }

    #[test]
    fn release_uses_physical_key_identity() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        session.key(38, "l", KeyState::Pressed, false, &bindings);
        session.key(38, "L", KeyState::Released, false, &bindings);
        assert_eq!(session.vector(), (0, 0));
    }

    #[test]
    fn focus_loss_releases_all_input_without_exiting() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        session.key(38, "l", KeyState::Pressed, false, &bindings);
        session.key(31, "s", KeyState::Pressed, false, &bindings);
        assert_eq!(
            session.release_all(),
            vec![Effect::Button {
                button: MouseButton::Left,
                state: KeyState::Released,
            }]
        );
        assert_eq!(session.vector(), (0, 0));
    }

    #[test]
    fn duplicate_physical_button_keys_emit_one_button_pair() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        assert_eq!(
            session.key(31, "s", KeyState::Pressed, false, &bindings),
            vec![Effect::Button {
                button: MouseButton::Left,
                state: KeyState::Pressed,
            }]
        );
        assert!(
            session
                .key(32, "s", KeyState::Pressed, false, &bindings)
                .is_empty()
        );
        assert!(
            session
                .key(31, "s", KeyState::Released, false, &bindings)
                .is_empty()
        );
        assert_eq!(
            session.key(32, "s", KeyState::Released, false, &bindings),
            vec![Effect::Button {
                button: MouseButton::Left,
                state: KeyState::Released,
            }]
        );
    }

    #[test]
    fn button_lock_uses_a_sequential_prefix() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        assert_eq!(
            session.key(47, "v", KeyState::Pressed, false, &bindings),
            vec![Effect::RedrawMode]
        );
        assert!(session.lock_pending());
        assert_eq!(
            session.key(31, "s", KeyState::Pressed, false, &bindings),
            vec![
                Effect::Button {
                    button: MouseButton::Left,
                    state: KeyState::Pressed,
                },
                Effect::RedrawMode,
            ]
        );
        assert_eq!(session.locked_button(), Some(MouseButton::Left));
        assert!(
            session
                .key(31, "s", KeyState::Released, false, &bindings)
                .is_empty()
        );
        session.key(38, "l", KeyState::Pressed, false, &bindings);
        assert_eq!(session.vector(), (1, 0));
        assert_eq!(
            session.key(47, "v", KeyState::Pressed, false, &bindings),
            vec![
                Effect::Button {
                    button: MouseButton::Left,
                    state: KeyState::Released,
                },
                Effect::RedrawMode,
            ]
        );
        assert_eq!(session.locked_button(), None);
    }

    #[test]
    fn button_lock_supports_every_mouse_button() {
        for (raw_code, symbol, button) in [
            (31, "s", MouseButton::Left),
            (32, "d", MouseButton::Middle),
            (33, "f", MouseButton::Right),
        ] {
            let bindings = MouseBindings::default();
            let mut session = MouseSession::default();
            session.key(47, "v", KeyState::Pressed, false, &bindings);
            let effects = session.key(raw_code, symbol, KeyState::Pressed, false, &bindings);
            assert_eq!(
                effects[0],
                Effect::Button {
                    button,
                    state: KeyState::Pressed
                }
            );
            assert_eq!(session.locked_button(), Some(button));
        }
    }

    #[test]
    fn escape_releases_a_locked_button_before_exit() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        session.key(47, "v", KeyState::Pressed, false, &bindings);
        session.key(33, "f", KeyState::Pressed, false, &bindings);
        assert_eq!(
            session.key(1, "Escape", KeyState::Pressed, false, &bindings),
            vec![
                Effect::Button {
                    button: MouseButton::Right,
                    state: KeyState::Released,
                },
                Effect::Exit,
            ]
        );
    }
}
