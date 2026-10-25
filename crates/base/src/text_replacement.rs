//! The system's text replacements: macOS System Settings → Keyboard → Text
//! Replacements, such as "omw" → "On my way!".
//!
//! AppKit applies them only inside `NSTextView`, and GPUI draws its own text
//! inputs, so an input asks here when a typed boundary character (a space or
//! sentence punctuation) ends a word.

use std::ops::Range;

/// Whether typing `new_text` ends a word, so the shortcut before it may expand.
///
/// Only whitespace and sentence punctuation count. `-`, `_`, `/`, `:`, `@`,
/// `#` and quotes stay inside a word, so shortcuts such as `q-dev` keep
/// working and completion triggers in front of the cursor never shift.
pub fn is_word_boundary(new_text: &str) -> bool {
    matches!(
        new_text,
        " " | "\t" | "\n" | "." | "," | "!" | "?" | ";" | ")" | "]" | "}"
    )
}

/// The shortcut `text` ends with and its expansion: the byte range of the
/// shortcut inside `text`, and the text to put there.
///
/// `text` is what comes before the boundary character just typed. `None` when
/// the user turned text replacement off, no shortcut matches, or the platform
/// has no replacements.
pub fn replacement_before(text: &str) -> Option<(Range<usize>, String)> {
    find(text, platform::replacements())
}

/// The longest shortcut that ends `text` and starts a word there.
fn find(
    text: &str,
    replacements: impl IntoIterator<Item = (String, String)>,
) -> Option<(Range<usize>, String)> {
    replacements
        .into_iter()
        .filter(|(shortcut, expansion)| {
            !shortcut.is_empty()
                && shortcut != expansion
                && text.ends_with(shortcut.as_str())
                && text[..text.len() - shortcut.len()]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric())
        })
        .max_by_key(|(shortcut, _)| shortcut.len())
        .map(|(shortcut, expansion)| (text.len() - shortcut.len()..text.len(), expansion))
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::NSSpellChecker;

    /// The user's replacements, or none when they turned text replacement off
    /// (Edit → Substitutions → Text Replacement, which AppKit saves for every
    /// app).
    pub(super) fn replacements() -> Vec<(String, String)> {
        if !NSSpellChecker::isAutomaticTextReplacementEnabled() {
            return Vec::new();
        }
        let (shortcuts, expansions) = NSSpellChecker::sharedSpellChecker()
            .userReplacementsDictionary()
            .to_vecs();
        shortcuts
            .iter()
            .zip(expansions.iter())
            .map(|(shortcut, expansion)| (shortcut.to_string(), expansion.to_string()))
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub(super) fn replacements() -> Vec<(String, String)> {
        Vec::new()
    }
}
