//! Text-editing operations behind the keypad.
//!
//! Pure string and caret arithmetic, with no Win32 dependency, so the fiddly
//! cases — inserting inside a bracket, backspacing at the start, clearing —
//! are all testable without a window.

/// A caret position expressed as a character index into the text.
pub type Caret = usize;

/// The result of applying a key to a text buffer.
pub struct Edit {
    pub text: String,
    pub caret: Caret,
}

/// Inserts `fragment` at `caret`, leaving the caret `back_from_end` characters
/// before the end of what was inserted.
///
/// `caret` is clamped rather than trusted: it arrives from a Win32 control and
/// could be stale after the text changed underneath us.
pub fn insert(text: &str, caret: Caret, fragment: &str, back_from_end: usize) -> Edit {
    let mut chars: Vec<char> = text.chars().collect();
    let at = caret.min(chars.len());
    let inserted: Vec<char> = fragment.chars().collect();
    let tail = chars.split_off(at);
    chars.extend_from_slice(&inserted);
    chars.extend_from_slice(&tail);
    let new_caret = at
        + inserted
            .len()
            .saturating_sub(back_from_end.min(inserted.len()));
    Edit {
        text: chars.into_iter().collect(),
        caret: new_caret,
    }
}

/// Removes the character before `caret`, or the selection when it spans text.
///
/// Windows' own delete key would need a real edit control to drive; doing it on
/// the string keeps the behaviour identical to typing.
pub fn backspace(text: &str, caret: Caret, selection_end: Caret) -> Edit {
    let mut chars: Vec<char> = text.chars().collect();
    let (low, high) = ordered(caret, selection_end);
    let low = low.min(chars.len());
    let high = high.min(chars.len());
    if high > low {
        chars.drain(low..high);
        return Edit {
            text: chars.into_iter().collect(),
            caret: low,
        };
    }
    if low == 0 {
        return Edit {
            text: text.to_string(),
            caret: 0,
        };
    }
    chars.remove(low - 1);
    Edit {
        text: chars.into_iter().collect(),
        caret: low - 1,
    }
}

/// Removes the selection, or the character after the caret when there is none.
pub fn delete_forward(text: &str, caret: Caret, selection_end: Caret) -> Edit {
    let mut chars: Vec<char> = text.chars().collect();
    let (low, high) = ordered(caret, selection_end);
    let low = low.min(chars.len());
    let high = high.min(chars.len());
    if high > low {
        chars.drain(low..high);
        return Edit {
            text: chars.into_iter().collect(),
            caret: low,
        };
    }
    if low < chars.len() {
        chars.remove(low);
    }
    Edit {
        text: chars.into_iter().collect(),
        caret: low,
    }
}

/// Borrows the most recent result, so `ans` refers to something.
///
/// The engine stores this as the variable `ans`; the keypad only inserts the
/// name, so nothing is needed here beyond knowing the convention.
pub const ANSWER_VARIABLE: &str = "ans";

fn ordered(a: Caret, b: Caret) -> (Caret, Caret) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}
