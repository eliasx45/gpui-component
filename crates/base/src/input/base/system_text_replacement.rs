//! The system's text replacements ("omw" → "On my way!") in plain text inputs.

use gpui::{Context, Window};

use super::{InputBaseState, InputModeKind, RopeExt as _, undo_manager::EditIntent};
use crate::text_replacement;

/// How far back a shortcut is looked for, in bytes. Shortcuts are short.
const LOOKBEHIND: usize = 256;

impl<M: InputModeKind> InputBaseState<M> {
    /// Whether typing `new_text` may expand the shortcut in front of it. Read
    /// before the edit, which ends any IME composition.
    ///
    /// Code editors keep their text verbatim, and masked, patterned or
    /// validated fields could reject the expansion.
    pub(super) fn may_expand_text_replacement(&self, new_text: &str) -> bool {
        text_replacement::is_word_boundary(new_text)
            && !self.silent_replace_text
            && !self.replaying_history
            && self.ime_marked_range.is_none()
            && self.is_editable()
            && !self.is_code_editor()
            && !self.masked
            && self.mask_pattern.is_none()
            && self.validate.is_none()
    }

    /// Replace the shortcut that ends right before the just-typed `boundary`
    /// with its expansion, as its own undo step: ⌘Z brings back what was typed.
    pub(super) fn expand_text_replacement(
        &mut self,
        boundary: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selections.is_single() || !self.active_selection().is_empty() {
            return;
        }
        let cursor = self.cursor();
        let Some(boundary_start) = cursor.checked_sub(boundary.len()) else {
            return;
        };
        if self.text.slice(boundary_start..cursor).to_string() != boundary {
            return;
        }
        let row = self.text.offset_to_point(boundary_start).row;
        let line_start = self.text.line_start_offset(row);
        let start = self.text.clip_offset(
            line_start.max(boundary_start.saturating_sub(LOOKBEHIND)),
            sum_tree::Bias::Right,
        );
        let before = self.text.slice(start..boundary_start).to_string();
        let Some((shortcut, expansion)) = text_replacement::replacement_before(&before) else {
            return;
        };
        let range = start + shortcut.start..cursor;
        // An inline token next to the shortcut would widen the edit over it.
        if self.normalize_token_range(range.clone()) != range {
            return;
        }
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        self.replace_text_in_ranges(&[(range, format!("{expansion}{boundary}"))], window, cx);
    }
}
