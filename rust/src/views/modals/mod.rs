//! Modal overlay system for sqtop-rust.
//!
//! # Architecture
//!
//! Python sqtop uses `textual.screen.ModalScreen[T]` with `dismiss(value)` callbacks.
//! Ratatui has no screen stack, so we model modals as data:
//!
//! 1. **State**: `App::modal` holds an enum (`Modal::None`, `Modal::Confirm(...)`, etc.)
//! 2. **Input routing**: Key events go to the active modal first via `Modal::handle_key(...)`
//! 3. **Outcome**: Each modal returns an `Outcome` enum (its equivalent of `dismiss(value)`)
//! 4. **Action**: The App matches on the outcome and performs the requested action
//! 5. **Rendering**: Modals render as centered overlays via `Modal::render(...)`
//!
//! ## Example flow
//!
//! ```ignore
//! // User presses a key that opens a modal
//! app.modal = Modal::Confirm(ConfirmState::new("Cancel job?"));
//!
//! // Next frame: modal is rendered as overlay
//! modal.render(f, area);
//!
//! // User presses 'y'
//! match modal.handle_key(key_event, config) {
//!     ModalOutcome::Dismiss(ConfirmResult::Yes) => {
//!         // App performs the action
//!         app.modal = Modal::None;
//!     }
//!     ModalOutcome::Continue => { /* keep modal open */ }
//!     _ => {}
//! }
//! ```

pub mod bulk_actions;
pub mod column_toggle;
pub mod confirm;
pub mod job_actions;
pub mod keybindings_help;
pub mod notify;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Outcome of a modal handling a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalOutcome<T> {
    /// Modal should close and return this result.
    Dismiss(T),
    /// Modal remains open.
    Continue,
}

/// Helper to center a rect within an area with the given width/height.
/// Returns a rect no larger than the available area.
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centered_rect_fits_within_area() {
        let area = Rect::new(0, 0, 100, 50);
        let centered = centered_rect(area, 40, 20);
        assert_eq!(centered.width, 40);
        assert_eq!(centered.height, 20);
        assert!(centered.x + centered.width <= area.width);
        assert!(centered.y + centered.height <= area.height);
    }

    #[test]
    fn test_centered_rect_clamps_to_area() {
        let area = Rect::new(0, 0, 30, 20);
        let centered = centered_rect(area, 100, 50);
        assert_eq!(centered.width, 30);
        assert_eq!(centered.height, 20);
    }
}
