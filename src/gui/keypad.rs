//! The button keypad: what each key is, what it inserts, and how the grid is
//! arranged.
//!
//! Kept separate from the window code and free of any Win32 type so the layout
//! arithmetic and the insertion rules can be tested without opening a window.

/// Base identifier for keypad buttons. Button `n` gets `ID_KEY_FIRST + n`, so
/// a `WM_COMMAND` identifies its key by subtracting this and indexing the grid.
pub const ID_KEY_FIRST: i32 = 100;

/// One key on the pad.
pub struct Key {
    /// Text drawn on the button.
    pub label: &'static str,
    /// Text inserted into the expression when the key is pressed.
    ///
    /// Usually the same as `label`, but not always: `π` inserts `pi`, and the
    /// function keys insert an opening parenthesis so the argument can be typed
    /// straight away.
    pub insert: &'static str,
    /// Caret offset from the end of `insert`, in characters. Zero means the
    /// caret goes after everything inserted; `sqrt(` wants the caret inside the
    /// parenthesis.
    pub caret: usize,
}

impl Key {
    const fn new(label: &'static str, insert: &'static str) -> Key {
        Key {
            label,
            insert,
            caret: 0,
        }
    }

    /// A key that inserts a completed call, leaving the caret inside the
    /// brackets so the argument can be typed straight away.
    ///
    /// Both brackets are inserted rather than the opening one alone: the user
    /// should not have to close a call the button opened for them.
    const fn call(label: &'static str, insert: &'static str) -> Key {
        Key {
            label,
            insert,
            caret: 1,
        }
    }
}

/// The keypad, row by row. Row count is derived, so adding a row here is enough
/// to change the layout.
pub const ROWS: &[&[Key]] = &[
    &[
        Key::new("7", "7"),
        Key::new("8", "8"),
        Key::new("9", "9"),
        Key::new("(", "("),
        Key::new(")", ")"),
        Key::new("^", "^"),
        Key::new("!", "!"),
        Key::new("%", "%"),
    ],
    &[
        Key::new("4", "4"),
        Key::new("5", "5"),
        Key::new("6", "6"),
        Key::new("+", "+"),
        Key::new("-", "-"),
        Key::new("*", "*"),
        Key::new("/", "/"),
        Key::new(".", "."),
    ],
    &[
        Key::new("1", "1"),
        Key::new("2", "2"),
        Key::new("3", "3"),
        Key::call("sqrt", "sqrt()"),
        Key::call("ln", "ln()"),
        Key::call("log", "log10()"),
        Key::call("exp", "exp()"),
        Key::new("pi", "pi"),
    ],
    &[
        Key::new("0", "0"),
        Key::new("00", "00"),
        Key::new("x", "x"),
        Key::call("sin", "sin()"),
        Key::call("cos", "cos()"),
        Key::call("tan", "tan()"),
        Key::call("atan", "atan()"),
        Key::new("e", "e"),
    ],
    &[
        Key::new("C", "\u{1}"),
        Key::new("Del", "\u{2}"),
        Key::new("=", "\u{3}"),
        Key::call("abs", "abs()"),
        Key::call("gcd", "gcd()"),
        Key::call("nthroot", "nthroot()"),
        Key::new("ans", "ans"),
        Key::new("E", "e"),
    ],
];

/// Control characters used as key actions. They cannot be typed into an edit
/// control on purpose, so they never collide with ordinary text insertion.
pub const ACTION_CLEAR: &str = "\u{1}";
pub const ACTION_BACKSPACE: &str = "\u{2}";
pub const ACTION_EVALUATE: &str = "\u{3}";

/// Every key in the pad, in grid order.
pub fn keys() -> impl Iterator<Item = &'static Key> {
    ROWS.iter().flat_map(|row| row.iter())
}

/// The key at `index` in grid order, or `None` if out of range.
pub fn key_at(index: usize) -> Option<&'static Key> {
    let wanted = index;
    let mut seen = 0usize;
    for row in ROWS {
        for key in row.iter() {
            if seen == wanted {
                return Some(key);
            }
            seen += 1;
        }
    }
    None
}

/// How many keys the pad holds in total.
pub fn key_count() -> usize {
    ROWS.iter().map(|row| row.len()).sum()
}

/// The widest row, which sets the column count for a uniform grid.
pub fn columns() -> usize {
    ROWS.iter().map(|row| row.len()).max().unwrap_or(0)
}

/// Cell rectangle for `index` within a `width` x `height` grid area, in pixels.
///
/// Grid coordinates rather than a fixed pixel size, so the pad scales with the
/// window. The final row stretches to the bottom edge so no gap is left.
pub fn cell(index: usize, width: i32, height: i32) -> (i32, i32, i32, i32) {
    let columns = columns().max(1) as i32;
    let rows = ROWS.len().max(1) as i32;
    let column = (index as i32) % columns;
    let row = (index as i32) / columns;

    let left = column * width / columns;
    let right = (column + 1) * width / columns;
    let top = row * height / rows;
    let bottom = (row + 1) * height / rows;
    (left, top, right - left, bottom - top)
}
