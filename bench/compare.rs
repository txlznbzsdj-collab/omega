// Standalone benchmark of omega's numeric core.
//
// Runs the same workloads as the Python and C++ benchmarks so the three can be
// compared directly. Every operation is routed through the engine, so the
// figures include whatever omega adds on top of the underlying bignum.

use omega::Engine;
use std::time::Instant;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    if which == "conv" {
        conversion_scaling();
        return;
    }
    let all = which == "all";

    if all || which == "pow" {
        bench(
            "2^1000000",
            || {
                evaluate("2^1000000");
            },
            1,
        );
    }
    if all || which == "pow_big" {
        bench(
            "2^10000000",
            || {
                evaluate("2^10000000");
            },
            1,
        );
    }
    if all || which == "fact" {
        bench(
            "10000!",
            || {
                evaluate("10000!");
            },
            1,
        );
    }
    if all || which == "fact_big" {
        bench(
            "50000!",
            || {
                evaluate("50000!");
            },
            1,
        );
    }
    if all || which == "str" {
        bench(
            "str(2^1000000)",
            || {
                render("2^1000000");
            },
            1,
        );
    }
    if all || which == "sqrt" {
        bench(
            "sqrt(2)@1000",
            || {
                let mut engine = Engine::new().with_precision(1000);
                let _ = engine.eval("sqrt(2)").unwrap();
            },
            1,
        );
    }
    if all || which == "pi" {
        bench(
            "pi@1000 (machin)",
            || {
                let mut engine = Engine::new().with_precision(1000);
                let _ = engine.eval("pi").unwrap();
            },
            1,
        );
    }
    if all || which == "mul" {
        bench(
            "a*b (large)",
            || {
                evaluate("3^500000 * 7^400000");
            },
            1,
        );
    }
    if all || which == "gcd" {
        bench(
            "gcd(2^1000000,2^600000)",
            || {
                evaluate("gcd(2^1000000, 2^600000)");
            },
            1,
        );
    }
}

/// Parses and evaluates, leaving the value in its internal form.
fn evaluate(input: &str) -> omega::Value {
    let mut engine = Engine::new();
    engine
        .eval(input)
        .unwrap_or_else(|error| panic!("`{input}`: {error}"))
}

/// Parses, evaluates, and converts to decimal text.
fn render(input: &str) -> String {
    let value = evaluate(input);
    omega::format::render(&value, omega::format::Style::default()).text
}

fn bench(label: &str, mut work: impl FnMut(), reps: usize) {
    // Warm up so allocation and caches are settled before measuring.
    work();
    let mut best = f64::MAX;
    for _ in 0..reps {
        let started = Instant::now();
        work();
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if elapsed < best {
            best = elapsed;
        }
    }
    println!("{label}\t{best:.3}");
}

/// Decimal conversion cost on its own, measured in-process so that process
/// startup does not swamp the figure. Conversion is the dominant cost for a
/// large exact result.
fn conversion_scaling() {
    for exponent in [250_000u32, 500_000, 1_000_000, 2_000_000] {
        let value = evaluate(&format!("2^{exponent}"));
        let mut best = f64::MAX;
        let mut digits = 0;
        for _ in 0..3 {
            let started = Instant::now();
            let text = omega::format::render(&value, omega::format::Style::default()).text;
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            digits = text.len();
            if elapsed < best {
                best = elapsed;
            }
            std::hint::black_box(&text);
        }
        println!("2^{exponent}\t{best:.1}\t{digits} digits");
    }
}
