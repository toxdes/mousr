use std::collections::BTreeSet;

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
    directions: BTreeSet<DirectionKey>,
    buttons: BTreeSet<ButtonKey>,
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
        symbol: &str,
        state: KeyState,
        repeated: bool,
        bindings: &MouseBindings,
    ) -> Vec<Effect> {
        if symbol == bindings.cancel && state == KeyState::Pressed {
            return self.cancel();
        }
        if let Some(direction) = movement_binding(symbol, bindings) {
            match state {
                KeyState::Pressed => {
                    self.directions.insert(direction);
                }
                KeyState::Released => {
                    self.directions.remove(&direction);
                }
            }
            return Vec::new();
        }
        if let Some((key, button)) = button_binding(symbol, bindings) {
            if repeated {
                return Vec::new();
            }
            let changed = match state {
                KeyState::Pressed => self.buttons.insert(key),
                KeyState::Released => self.buttons.remove(&key),
            };
            return changed
                .then_some(Effect::Button { button, state })
                .into_iter()
                .collect();
        }
        if state == KeyState::Pressed
            && let Some(direction) = scroll_binding(symbol, bindings)
        {
            return vec![Effect::Scroll(direction)];
        }
        Vec::new()
    }

    pub fn vector(&self) -> (i8, i8) {
        let x = i8::from(self.directions.contains(&DirectionKey::Right))
            - i8::from(self.directions.contains(&DirectionKey::Left));
        let y = i8::from(self.directions.contains(&DirectionKey::Down))
            - i8::from(self.directions.contains(&DirectionKey::Up));
        (x, y)
    }

    pub fn cancel(&mut self) -> Vec<Effect> {
        let mut effects: Vec<Effect> = self
            .buttons
            .iter()
            .map(|button| Effect::Button {
                button: match button {
                    ButtonKey::Left => MouseButton::Left,
                    ButtonKey::Middle => MouseButton::Middle,
                    ButtonKey::Right => MouseButton::Right,
                },
                state: KeyState::Released,
            })
            .collect();
        self.buttons.clear();
        self.directions.clear();
        effects.push(Effect::Exit);
        effects
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
        let effects = session.key("f", &GridBindings::default());
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
    fn held_button_is_released_on_cancel() {
        let bindings = MouseBindings::default();
        let mut session = MouseSession::default();
        assert_eq!(
            session.key("f", KeyState::Pressed, false, &bindings),
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
        session.key("h", KeyState::Pressed, false, &bindings);
        session.key("l", KeyState::Pressed, false, &bindings);
        assert_eq!(session.vector(), (0, 0));
    }

    #[test]
    fn horizontal_scroll_is_emitted() {
        let effects =
            MouseSession::default().key("y", KeyState::Pressed, false, &MouseBindings::default());
        assert_eq!(effects, vec![Effect::Scroll(Direction::Left)]);
    }
}
