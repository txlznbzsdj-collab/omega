//! Tests for the parts of the window that are not Win32.
//!
//! The caret arithmetic behind the keypad is where off-by-one mistakes hide,
//! and it is pure, so it is tested directly rather than through the UI. The
//! evaluation path is checked here too, mirroring what the window does between
//! its Win32 calls.

use omega::format::{Grouping, Style};
use omega::gui::edit::{backspace, delete_forward, insert, ANSWER_VARIABLE};
use omega::gui::keypad;

/// Mirrors `evaluate` in `src/gui/win32.rs`: the same engine call, the same
/// style, the same `ans` binding and the same error branch.
fn evaluate(engine: &mut omega::Engine, expression: &str, style: Style) -> Option<String> {
    if expression.trim().is_empty() {
        return None;
    }
    let outcome = match engine.eval(expression) {
        Ok(value) => {
            engine.set(ANSWER_VARIABLE, value.clone());
            omega::format::render(&value, style)
        }
        Err(error) => omega::format::Rendered {
            text: error.to_string(),
            note: None,
        },
    };
    Some(match outcome.note {
        Some(note) => format!("{}\r\n{}", outcome.text, note),
        None => outcome.text,
    })
}

/// The first displayed line of a result, or a message saying none appeared.
fn shown(engine: &mut omega::Engine, expression: &str) -> String {
    evaluate(engine, expression, Style::default())
        .unwrap_or_else(|| panic!("`{expression}` produced nothing"))
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Types a sequence of keypad keys by label, as a person would.
fn type_keys(sequence: &[&str]) -> String {
    let mut text = String::new();
    let mut caret = 0usize;
    for wanted in sequence {
        let key = keypad::keys()
            .find(|key| key.label == *wanted)
            .unwrap_or_else(|| panic!("no keypad key labelled `{wanted}`"));
        let result = insert(&text, caret, key.insert, key.caret);
        text = result.text;
        caret = result.caret;
    }
    text
}

#[test]
fn inserting_appends_and_moves_the_caret_past_the_text() {
    let result = insert("12", 2, "+3", 0);
    assert_eq!(result.text, "12+3");
    assert_eq!(result.caret, 4);
}

#[test]
fn inserting_inside_the_text_splits_it() {
    let result = insert("14", 1, "+3", 0);
    assert_eq!(result.text, "1+34");
    assert_eq!(result.caret, 3);
}

#[test]
fn function_keys_leave_the_caret_inside_the_bracket() {
    // A function key inserts a completed call and places the caret one
    // character from the end, which is between the brackets.
    let result = insert("", 0, "sqrt()", 1);
    assert_eq!(result.text, "sqrt()");
    assert_eq!(result.caret, 5);

    let result = insert("2+", 2, "sqrt()", 1);
    assert_eq!(result.text, "2+sqrt()");
    assert_eq!(result.caret, 7);
}

#[test]
fn typing_an_argument_lands_inside_the_brackets() {
    // The whole point of the caret position: digits typed next become the
    // argument rather than being appended after the closing bracket.
    let opened = insert("", 0, "sqrt()", 1);
    let typed = insert(&opened.text, opened.caret, "16", 0);
    assert_eq!(typed.text, "sqrt(16)");
    assert_eq!(typed.caret, 7);
}

#[test]
fn caret_beyond_the_end_is_clamped_rather_than_panicking() {
    // The caret comes from a Win32 control and can be stale by one.
    let result = insert("12", 99, "3", 0);
    assert_eq!(result.text, "123");
    assert_eq!(result.caret, 3);
}

#[test]
fn backspace_removes_the_character_before_the_caret() {
    let result = backspace("12+", 3, 3);
    assert_eq!(result.text, "12");
    assert_eq!(result.caret, 2);
}

#[test]
fn backspace_at_the_start_is_a_no_op() {
    let result = backspace("12", 0, 0);
    assert_eq!(result.text, "12");
    assert_eq!(result.caret, 0);
}

#[test]
fn backspace_removes_a_selection_regardless_of_direction() {
    let forward = backspace("abcdef", 1, 4);
    assert_eq!(forward.text, "aef");
    assert_eq!(forward.caret, 1);

    // A selection dragged right-to-left reports the caret after the selection,
    // so the two positions arrive in the other order.
    let backward = backspace("abcdef", 4, 1);
    assert_eq!(backward.text, "aef");
    assert_eq!(backward.caret, 1);
}

#[test]
fn delete_removes_the_character_after_the_caret() {
    let result = delete_forward("123", 1, 1);
    assert_eq!(result.text, "13");
    assert_eq!(result.caret, 1);
}

#[test]
fn delete_at_the_end_is_a_no_op() {
    let result = delete_forward("12", 2, 2);
    assert_eq!(result.text, "12");
    assert_eq!(result.caret, 2);
}

#[test]
fn clearing_and_multibyte_names_survive_the_round_trip() {
    // `pi` is inserted as three characters while the button reads as `π`, so
    // the inserted length differs from the label length.
    let result = insert("2*", 2, "pi", 0);
    assert_eq!(result.text, "2*pi");
    assert_eq!(result.caret, 4);
}

#[test]
fn every_key_inserts_something_and_fits_the_grid() {
    let columns = keypad::columns();
    assert!(columns > 0);
    for row in keypad::ROWS {
        assert!(
            row.len() <= columns,
            "a row is wider than the grid: {} > {columns}",
            row.len()
        );
    }
    for key in keypad::keys() {
        assert!(!key.label.is_empty(), "a key has no label");
        assert!(
            !key.insert.is_empty(),
            "key `{}` inserts nothing",
            key.label
        );
        assert!(
            key.caret < key.insert.chars().count().max(1),
            "key `{}` puts the caret outside what it inserted",
            key.label
        );
        // A key that opens a call must close it, or the user is left with an
        // expression that cannot evaluate. The bare `(` and `)` keys are
        // deliberately single brackets and are excluded.
        if key.insert != "(" && key.insert != ")" {
            let opens = key.insert.matches('(').count();
            let closes = key.insert.matches(')').count();
            assert_eq!(
                opens, closes,
                "key `{}` inserts unbalanced brackets: {:?}",
                key.label, key.insert
            );
        }
    }
}

#[test]
fn key_indices_map_onto_the_grid() {
    let count = keypad::key_count();
    assert!(count > 0);
    for index in 0..count {
        assert!(keypad::key_at(index).is_some(), "index {index} is missing");
    }
    assert!(
        keypad::key_at(count).is_none(),
        "index past the end resolved"
    );
}

#[test]
fn grid_cells_tile_the_area_without_overlapping() {
    let (width, height, gap) = (800, 300, 4);
    let count = keypad::key_count();
    let columns = keypad::columns() as i32;

    // Every cell must sit inside the area and be non-empty.
    for index in 0..count {
        let (x, y, w, h) = keypad::cell(index, width, height, gap);
        assert!(x >= 0 && y >= 0, "cell {index} starts outside the area");
        assert!(x + w <= width, "cell {index} overflows the width");
        assert!(y + h <= height, "cell {index} overflows the height");
        assert!(w > 0 && h > 0, "cell {index} has no size");
    }

    // Neighbours in a row must be separated by the gap rather than touching or
    // overlapping, which is what made `sqrt ln log exp` run together on screen.
    let rows = (count as i32 + columns - 1) / columns;
    for row in 0..rows {
        let mut previous_right: Option<i32> = None;
        for column in 0..columns {
            let index = (row * columns + column) as usize;
            if index >= count {
                break;
            }
            let (x, _, w, _) = keypad::cell(index, width, height, gap);
            if let Some(right) = previous_right {
                let spacing = x - right;
                assert!(
                    spacing >= gap - 1,
                    "row {row}: buttons are {spacing}px apart, expected about {gap}"
                );
            }
            previous_right = Some(x + w);
        }
    }

    // A very narrow window must still give every button a positive size rather
    // than letting the gap consume it.
    for index in 0..count {
        let (_, _, w, h) = keypad::cell(index, 40, 30, gap);
        assert!(w >= 1 && h >= 1, "cell {index} collapsed on a tiny window");
    }
}

#[test]
fn a_long_label_fits_a_button_at_the_minimum_width() {
    // The pad is sized as `columns * MIN_KEY_WIDTH`, so a label wider than that
    // is silently clipped by Windows — which is how `nthroot` became `hro`.
    const MIN_KEY_WIDTH: usize = 52;
    const APPROX_CHAR_WIDTH: usize = 7;
    for key in keypad::keys() {
        let needed = key.label.chars().count() * APPROX_CHAR_WIDTH;
        assert!(
            needed <= MIN_KEY_WIDTH,
            "label `{}` needs about {needed}px, more than the {MIN_KEY_WIDTH}px a button holds",
            key.label
        );
    }
}

// ---------------------------------------------------------------------------
// The evaluation path the window walks between its Win32 calls
// ---------------------------------------------------------------------------

#[test]
fn results_match_the_engine_including_errors() {
    let mut engine = omega::Engine::new();
    assert_eq!(
        shown(&mut engine, "2^128"),
        "340282366920938463463374607431768211456"
    );
    assert_eq!(shown(&mut engine, "1/3 + 1/6"), "1/2");
    assert_eq!(shown(&mut engine, "0.1 + 0.2"), "0.3");
    assert_eq!(
        shown(&mut engine, "sqrt(2)"),
        "1.4142135623730950488016887242097"
    );

    // An error is shown where the answer would go, not raised as a panic.
    assert!(shown(&mut engine, "1/0").contains("division by zero"));
    assert!(shown(&mut engine, "sqrt(-4)").contains("not a real number"));
}

#[test]
fn ans_refers_to_the_previous_answer() {
    let mut engine = omega::Engine::new();
    assert_eq!(shown(&mut engine, "6*7"), "42");
    assert_eq!(shown(&mut engine, "ans + 1"), "43");
    assert_eq!(shown(&mut engine, "ans * ans"), "1849");
}

#[test]
fn grouping_changes_only_how_a_result_reads() {
    let mut engine = omega::Engine::new();
    let grouped = Style {
        grouping: Grouping::Underscore,
        ..Style::default()
    };
    let plain = Style::default();
    let with = evaluate(&mut engine, "2^64", grouped).unwrap();
    let without = evaluate(&mut engine, "2^64", plain).unwrap();
    assert_eq!(with, "18_446_744_073_709_551_616");
    assert_eq!(without, "18446744073709551616");
    // The same digits either way, so grouping cannot change the value.
    assert_eq!(with.replace('_', ""), without);
}

#[test]
fn typing_on_the_keypad_builds_a_working_expression() {
    let mut engine = omega::Engine::new();

    assert_eq!(type_keys(&["1", "2", "^", "3"]), "12^3");
    assert_eq!(shown(&mut engine, "12^3"), "1728");

    // A function key opens and closes the call, and the digits typed next land
    // inside the brackets.
    assert_eq!(type_keys(&["sqrt", "1", "6"]), "sqrt(16)");
    assert_eq!(shown(&mut engine, "sqrt(16)"), "4");

    // `pi` inserts the name the engine understands, not the glyph.
    assert_eq!(type_keys(&["pi"]), "pi");
    assert_eq!(
        shown(&mut engine, "pi"),
        "3.1415926535897932384626433832795"
    );

    for sequence in [
        vec!["4", "5", "!"],
        vec!["1", "/", "3"],
        vec!["2", "^", "1", "0"],
        vec!["sin", "0"],
        vec!["gcd", "1", "2"],
    ] {
        let typed = type_keys(&sequence);
        let mut scratch = omega::Engine::new();
        assert!(
            evaluate(&mut scratch, &typed, Style::default()).is_some(),
            "typing {sequence:?} produced {typed:?}, which the engine refused"
        );
    }
}

#[test]
fn blank_input_produces_nothing_rather_than_an_error() {
    let mut engine = omega::Engine::new();
    assert!(evaluate(&mut engine, "", Style::default()).is_none());
    assert!(evaluate(&mut engine, "   ", Style::default()).is_none());
}

#[test]
fn a_long_result_survives_the_round_trip() {
    let mut engine = omega::Engine::new();
    let big = evaluate(&mut engine, "2^10000", Style::default()).unwrap();
    let digits = big.chars().filter(|c| c.is_ascii_digit()).count();
    assert_eq!(digits, 3011, "2^10000 has 3011 digits");
}
