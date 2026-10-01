//! Command line front end: an interactive REPL, one-shot evaluation, and a
//! filter mode for pipelines.

use crate::eval::Engine;
use crate::format::{self, Grouping, Style};
use crate::lexer;
use crate::value::Value;
use std::io::{self, BufRead, IsTerminal, Write};
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = r#"omega - an arbitrary-precision calculator

USAGE
  omega                     start an interactive session
  omega "<expression>"      evaluate one expression and print the result
  echo "<expr>" | omega     read expressions from standard input

  omega --help              show this help
  omega --version           show the version
  omega -e "<expr>"         same as passing the expression directly
  omega -d <n> "<expr>"     show n significant digits for an inexact result
  omega -v "<expr>"         print variable bindings after evaluating

DIGIT GROUPING
  Numbers are printed as bare digits by default: 18446744073709551616.
  Ask for separators when you want them; they are only a display aid and are
  ignored if you paste the number back in.

  omega -g "<expr>"          separate every three digits with _: 18_446_744_...
  omega -g , "<expr>"        use a comma instead: 18,446,744,...
  omega -g space "<expr>"    use a space instead: 18 446 744 ...
  omega -g none "<expr>"     bare digits (the default)
  omega --group-from 7 "1e6" only group numbers with 7 or more digits

NUMBERS
  Integers are unbounded and no digit is ever withheld: 2^1000000 prints all
  301030 of its digits. A division that does not divide evenly stays exact as
  a fraction, so 1/3 is 1/3 rather than 0.3333. A value with no exact answer
  (pi, sqrt(2), ln) is computed to a high working precision and printed to 32
  significant digits; raise that with -d.
  Bases use 0x, 0o, 0b prefixes, and _ may group digits: 1_000_000

TIMING
  Every result is followed by how long it took, measured from the moment the
  expression was accepted until the value was ready to print.

    $ omega "2^1000"
    1071508607186267320948425049060001810561404811705533607443750388370351...
      time  98.500 us

  The figure is real elapsed time, not an estimate, and the unit is chosen
  from the magnitude: ns, us, ms, then s with ms in brackets. Processing
  starts before this figure and printing happens after it, so the wall clock
  time of a shell command is higher, mostly by process startup.

  Note that turning a large integer into decimal digits costs far more than
  computing it. 2^1000000 is evaluated in well under a millisecond but takes
  roughly a fifth of a second to render, and the reported time includes that
  rendering, since it is part of what you wait for.

LIMITS
  A result is held in memory, so it must fit. Anything up to a billion digits
  is computed; beyond that the request is refused, and the error says how
  large the answer would have been.

    $ omega "9999^9999999999"
    omega: value too large: the exact result would have about
    39_999_565_680 digits, which exceeds the 1_000_000_000 digit limit
    (roughly 15.5 GiB of memory)

  When you only want to know how long a number is, size(b, e) reports the
  digit count of b^e without computing it, and works at any scale:

    $ omega "size(9999, 9999999999)"
    39999565680

    $ omega "size(2, 1000000)"
    301030

  digits(x) reports the digit count of a value that already exists.

OPERATORS
  + - * / %          arithmetic; ^ is power (right associative)
  !                  factorial (postfix), ~ is a synonym for unary minus
  ( )                grouping; = assigns a variable
  -2^2 is -(2^2) = -4, and 2^3^2 is 2^(3^2) = 512

FUNCTIONS
  sqrt(x)  cbrt(x)  nthroot(x, n)  abs(x)  sign(x)  min(a,b)  max(a,b)
  floor(x) ceil(x)  trunc(x)  round(x)  frac(x)  pow(a,b)  float(x)
  ln(x)    log10(x) log(x)   log(b, x)   exp(x)
  sin(x)   cos(x)   tan(x)   asin(x)     acos(x)   atan(x)
  sinh(x)  cosh(x)
  gcd(a,b) lcm(a,b)  isprime(n)  digits(n)  size(b, e)

CONSTANTS
  pi  e  tau  phi

SESSION COMMANDS
  Work both in the interactive prompt and in a piped script:
    vars            list the variables currently bound
    clear           forget every variable
    help            print this help
    quit            leave the interactive session (also Ctrl-D)

EXAMPLES
  omega "2^128"                      340282366920938463463374607431768211456
  omega "1/3 + 1/6"                  1/2
  omega -d 50 "sqrt(2)"              1.414213562373095048801688724209698078569671875377
  omega "1000!"                      the exact value, all 2568 digits
  omega "isprime(2^127 - 1)"         1
  printf '2^64\nsqrt(2)\n' | omega   evaluate a list of expressions
"#;

/// Runs the front end and returns the process exit code.
pub fn run(args: Vec<String>) -> i32 {
    let mut style = Style::default();
    let mut show_vars = false;
    let mut statements: Vec<String> = Vec::new();
    // `--digits` sets both how many digits are shown and how many are computed,
    // so an irrational result can actually be produced to that many places.
    let mut requested_precision: Option<u32> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--help" | "-h" => {
                print!("{HELP}");
                return 0;
            }
            "--version" | "-V" => {
                println!("omega {VERSION}");
                return 0;
            }
            "--vars" | "-v" => show_vars = true,
            // Grouping is off unless asked for, so `-g` alone selects the
            // conventional underscore separator.
            "-g" | "--group" => {
                let next = args.get(index + 1).map(String::as_str);
                match next {
                    Some("_") | Some("underscore") => {
                        style.grouping = Grouping::Underscore;
                        index += 1;
                    }
                    Some(",") | Some("comma") => {
                        style.grouping = Grouping::Comma;
                        index += 1;
                    }
                    Some(" ") | Some("space") => {
                        style.grouping = Grouping::Space;
                        index += 1;
                    }
                    Some("none") | Some("off") => {
                        style.grouping = Grouping::None;
                        index += 1;
                    }
                    // No argument, or an expression follows: default to `_`.
                    _ => style.grouping = Grouping::Underscore,
                }
            }
            "--group-from" => {
                index += 1;
                match args.get(index).and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) => style.group_from = n,
                    None => {
                        eprintln!("omega: --group-from needs a digit count");
                        return 2;
                    }
                }
            }
            "--digits" | "-d" => {
                index += 1;
                match args.get(index).and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) if n > 0 => {
                        style.digits = n;
                        requested_precision = Some(n as u32);
                    }
                    _ => {
                        eprintln!("omega: --digits needs a positive integer");
                        return 2;
                    }
                }
            }
            "--eval" | "-e" => {
                index += 1;
                match args.get(index) {
                    Some(expr) => statements.push(expr.clone()),
                    None => {
                        eprintln!("omega: --eval needs an expression");
                        return 2;
                    }
                }
            }
            other if other.starts_with('-') && other.len() > 1 && !is_expression(other) => {
                eprintln!("omega: unknown option `{other}` (try --help)");
                return 2;
            }
            other => statements.push(other.to_string()),
        }
        index += 1;
    }

    let mut engine = match requested_precision {
        Some(precision) => Engine::new().with_precision(precision),
        None => Engine::new(),
    };

    if !statements.is_empty() {
        let joined = statements.join(" ");
        return match evaluate_and_print(&mut engine, &joined, style, show_vars) {
            Ok(()) => 0,
            Err(code) => code,
        };
    }

    let stdin = io::stdin();
    if stdin.is_terminal() {
        repl(&mut engine, style, show_vars)
    } else {
        filter(&mut engine, style, stdin.lock())
    }
}

/// `-5` and `-2^2` are expressions, not flags.
fn is_expression(arg: &str) -> bool {
    arg.len() > 1
        && arg[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '(')
}

/// A session command that is not an expression.
enum Command {
    Quit,
    Handled,
    Expression,
}

/// Recognizes the small set of session commands. They are available in both
/// the interactive session and when reading a script from standard input, so a
/// piped line behaves the same way it would if typed.
fn session_command(engine: &mut Engine, line: &str, style: Style) -> Command {
    match line {
        "quit" | "exit" | ":q" => Command::Quit,
        "help" | "?" => {
            print!("{HELP}");
            Command::Handled
        }
        "vars" => {
            let items = engine.variables();
            if items.is_empty() {
                println!("(no variables)");
            } else {
                for (name, value) in items {
                    println!("  {name} = {}", format::render(value, style).text);
                }
            }
            Command::Handled
        }
        "clear" => {
            engine.clear();
            println!("(variables cleared)");
            Command::Handled
        }
        _ => Command::Expression,
    }
}

fn repl(engine: &mut Engine, style: Style, show_vars: bool) -> i32 {
    println!("omega {VERSION} - arbitrary-precision calculator");
    println!("Type an expression, or `help` for usage. Ctrl-D or `quit` exits.");
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        let _ = io::stdout().flush();
        let line = match lines.next() {
            Some(Ok(line)) => line,
            _ => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match session_command(engine, trimmed, style) {
            Command::Quit => break,
            Command::Handled => continue,
            Command::Expression => {
                let _ = evaluate_and_print(engine, trimmed, style, show_vars);
            }
        }
    }
    0
}

fn filter<R: BufRead>(engine: &mut Engine, style: Style, reader: R) -> i32 {
    let mut code = 0;
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match session_command(engine, trimmed, style) {
            Command::Quit => break,
            Command::Handled => continue,
            Command::Expression => {}
        }
        if evaluate_and_print(engine, trimmed, style, false).is_err() {
            code = 1;
        }
    }
    code
}

fn evaluate_and_print(
    engine: &mut Engine,
    input: &str,
    style: Style,
    show_vars: bool,
) -> Result<(), i32> {
    match evaluate(engine, input, style, show_vars) {
        Ok(result) => {
            // The value goes to stdout so it can be piped. The timing goes to
            // stderr whenever stdout is redirected, so `omega "2^64" | bc`
            // still receives a bare number while an interactive user sees the
            // figure. When stdout is a terminal both are shown together.
            println!("{}", result.value);
            if io::stdout().is_terminal() {
                println!("  time  {}", format_duration(result.elapsed));
            } else {
                eprintln!("  time  {}", format_duration(result.elapsed));
            }
            Ok(())
        }
        Err(message) => {
            eprintln!("omega: {message}");
            Err(1)
        }
    }
}

/// A finished evaluation: the text to print and how long producing it took.
struct Outcome {
    value: String,
    elapsed: Duration,
}

fn evaluate(
    engine: &mut Engine,
    input: &str,
    style: Style,
    show_vars: bool,
) -> Result<Outcome, String> {
    // The whole pipeline is timed: parsing, arithmetic and rendering. Timing
    // the arithmetic alone would be misleading, because turning a large integer
    // into decimal digits costs far more than computing it — `2^1000000` is
    // evaluated in well under a millisecond but takes a fifth of a second to
    // render. The figure reported is the part the user actually waits for.
    let started = Instant::now();
    let outcome = engine.eval(input);

    let value = match outcome {
        Ok(value) => value,
        Err(error) => {
            // Offer to close unbalanced parentheses; this is the one syntax
            // slip worth repairing automatically.
            if let Ok((_, Some(position))) = lexer::tokenize(input) {
                if let Ok(fixed) = balance(input, position) {
                    if let Ok(value) = engine.eval(&fixed) {
                        let rendered = format::render(&value, style);
                        return Ok(Outcome {
                            value: with_note(rendered, Some("added the missing `)`".into())),
                            elapsed: started.elapsed(),
                        });
                    }
                }
            }
            return Err(error.to_string());
        }
    };

    let rendered = format::render(&value, style);
    let elapsed = started.elapsed();
    let mut output = with_note(rendered, None);
    if show_vars {
        for (name, bound) in engine.variables() {
            output.push_str(&format!(
                "\n  {name} = {}",
                format::render(bound, style).text
            ));
        }
    }
    Ok(Outcome {
        value: output,
        elapsed,
    })
}

/// Formats an elapsed time for display, choosing the unit from the magnitude
/// so the printed figure always carries significant digits.
///
/// Sub-microsecond work is reported in microseconds rather than being rounded
/// away to `0.000 ms`, and anything at or above a second is reported in
/// seconds with milliseconds alongside, so a slow computation is still
/// readable at a glance.
pub fn format_duration(elapsed: Duration) -> String {
    let nanos = elapsed.as_nanos();
    if nanos == 0 {
        return "<1 us".to_string();
    }
    if nanos < 1_000 {
        return format!("{nanos} ns");
    }
    if nanos < 1_000_000 {
        // Microseconds: useful for the very fast cases.
        let micros = nanos as f64 / 1_000.0;
        return format!("{micros:.3} us");
    }
    let millis = nanos as f64 / 1_000_000.0;
    if millis < 1_000.0 {
        return format!("{millis:.3} ms");
    }
    let seconds = nanos as f64 / 1_000_000_000.0;
    format!("{seconds:.3} s ({millis:.3} ms)")
}

fn with_note(rendered: format::Rendered, extra: Option<String>) -> String {
    match (rendered.note, extra) {
        (Some(note), Some(extra)) => format!("{}  # {note}; {extra}", rendered.text),
        (Some(note), None) => format!("{}  # {note}", rendered.text),
        (None, Some(extra)) => format!("{}  # {extra}", rendered.text),
        (None, None) => rendered.text,
    }
}

/// Closes unclosed parentheses at `position`.
fn balance(input: &str, position: usize) -> Result<String, ()> {
    let chars: Vec<char> = input.chars().collect();
    if position >= chars.len() {
        return Err(());
    }
    let mut depth = 0i32;
    for ch in &chars[position..] {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    if depth <= 0 {
        return Err(());
    }
    Ok(format!("{input}{}", ")".repeat(depth as usize)))
}

/// Convenience wrapper used by tests and library consumers.
pub fn evaluate_once(input: &str) -> Result<Value, String> {
    let mut engine = Engine::new();
    engine.eval(input).map_err(|error| error.to_string())
}
