// Wire-level proof that the GUI evaluation path is correct.
//
// The GUI's own `evaluate` cannot be driven headlessly because it reads and
// writes real Win32 edit controls. What can be checked is the logic it performs
// between those two Win32 calls, which is reproduced here verbatim: the same
// engine method, the same render call, the same error branch, and the same
// trailing-note assembly. If this differs from the GUI, the GUI is wrong.

fn gui_evaluate_logic(engine: &mut omega::Engine, expression: &str) -> Option<String> {
    if expression.trim().is_empty() {
        return None;
    }
    let outcome = match engine.eval(expression) {
        Ok(value) => omega::format::render(&value, omega::format::Style::default()),
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

fn check(engine: &mut omega::Engine, input: &str, expected_contains: &str) {
    let shown =
        gui_evaluate_logic(engine, input).unwrap_or_else(|| panic!("`{input}` produced nothing"));
    let first_line = shown.lines().next().unwrap_or("");
    let ok = first_line.contains(expected_contains);
    println!(
        "{}  {:>12} -> {}",
        if ok { "ok  " } else { "FAIL" },
        format!("{input:?}"),
        &first_line[..first_line.len().min(56)]
    );
    assert!(
        ok,
        "`{input}`: expected {expected_contains:?} in {first_line:?}"
    );
}

fn main() {
    let mut engine = omega::Engine::new();
    check(
        &mut engine,
        "2^128",
        "340282366920938463463374607431768211456",
    );
    check(&mut engine, "1/3 + 1/6", "1/2");
    check(&mut engine, "0.1 + 0.2", "0.3");
    check(&mut engine, "sqrt(2)", "1.4142135623730950488016887242097");
    check(
        &mut engine,
        "100!",
        "93326215443944152681699238856266700490715968264381621",
    );
    check(&mut engine, "(-8)^(2/3)", "4.000000");

    // Errors must surface in the output pane rather than being swallowed.
    check(&mut engine, "1/0", "division by zero");
    check(&mut engine, "sqrt(-4)", "not a real number");

    // Variables persist across evaluations, as they do in the CLI.
    check(&mut engine, "x = 42", "42");
    check(&mut engine, "x^2", "1764");

    // Whitespace-only input must be a no-op, not a panic and not an error.
    assert!(gui_evaluate_logic(&mut engine, "   ").is_none());
    assert!(gui_evaluate_logic(&mut engine, "").is_none());
    println!("ok     blank input is a no-op");

    // A long result must survive intact; the round-trip through a String is
    // what the GUI hands to SetWindowTextW.
    let big = gui_evaluate_logic(&mut engine, "2^10000").unwrap();
    let digits = big.chars().filter(|c| c.is_ascii_digit()).count();
    println!("ok     2^10000 -> {digits} digits carried through");
    assert_eq!(digits, 3011, "2^10000 has 3011 digits");

    println!("\nall checks passed");
}
