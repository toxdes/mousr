use crate::cli::MouseButton;

pub struct ActionHint {
    pub key: String,
    pub action: &'static str,
}

fn hint(key: &str, action: &'static str) -> ActionHint {
    ActionHint {
        key: key.to_owned(),
        action,
    }
}

pub(crate) fn grid_action_hints(
    bindings: &crate::config::GridBindings,
    can_descend: bool,
) -> Vec<ActionHint> {
    let mut hints = vec![
        hint(&bindings.left_click, "Left click"),
        hint(&bindings.middle_click, "Middle click"),
        hint(&bindings.right_click, "Right click"),
        hint(&bindings.double_click, "Double click"),
        hint(&bindings.move_only, "Move pointer"),
        hint(&bindings.enter_mouse, "Mouse mode"),
        hint(&bindings.scroll_up, "Scroll up"),
        hint(&bindings.scroll_down, "Scroll down"),
        hint(&bindings.scroll_left, "Scroll left"),
        hint(&bindings.scroll_right, "Scroll right"),
    ];
    if can_descend {
        hints.push(hint(&bindings.descend, "Refine grid"));
    }
    hints.extend([
        hint(&bindings.back, "Back"),
        hint(&bindings.cancel, "Cancel"),
        hint("q", "Quit"),
    ]);
    hints
}

pub(crate) fn mouse_action_hints(
    bindings: &crate::config::MouseBindings,
    lock_pending: bool,
    locked_button: Option<MouseButton>,
) -> Vec<ActionHint> {
    let mut hints = vec![hint(
        &format!(
            "{} {} {} {}",
            bindings.left, bindings.down, bindings.up, bindings.right
        ),
        "Move pointer",
    )];
    if let Some(button) = locked_button {
        hints.push(hint(
            &bindings.button_lock,
            match button {
                MouseButton::Left => "Release left drag",
                MouseButton::Middle => "Release middle drag",
                MouseButton::Right => "Release right drag",
            },
        ));
    } else if lock_pending {
        hints.extend([
            hint(&bindings.left_button, "Lock left"),
            hint(&bindings.middle_button, "Lock middle"),
            hint(&bindings.right_button, "Lock right"),
        ]);
    } else {
        for (key, action) in [
            (&bindings.left_button, "Left / lock"),
            (&bindings.middle_button, "Middle / lock"),
            (&bindings.right_button, "Right / lock"),
        ] {
            hints.push(hint(
                &format!("{key} / {} {key}", bindings.button_lock),
                action,
            ));
        }
    }
    hints.extend([
        hint(&bindings.double_click, "Double click"),
        hint(&bindings.scroll_up, "Scroll up"),
        hint(&bindings.scroll_down, "Scroll down"),
        hint(&bindings.scroll_left, "Scroll left"),
        hint(&bindings.scroll_right, "Scroll right"),
        hint(&bindings.cancel, "Exit"),
        hint("q", "Quit"),
    ]);
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
