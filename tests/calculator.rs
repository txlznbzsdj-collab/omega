//! End-to-end tests for the calculator engine.
//!
//! These assert the promises the tool makes: exactness is preserved whenever
//! the true answer is exact, and derived constants agree with known values.

use omega::cli::format_duration;
use omega::Engine;
use std::time::Duration;

/// Evaluates one expression and renders it the way the CLI does.
fn calc(input: &str) -> String {
    let mut engine = Engine::new();
    let value = engine
        .eval(input)
        .unwrap_or_else(|error| panic!("`{input}` failed: {error}"));
    omega::format::render(&value, omega::format::Style::default()).text
}

/// Evaluates with no digit limit. Exact values are always rendered in full, so
/// this is only needed for inexact results.
fn calc_full(input: &str) -> String {
    let mut engine = Engine::new();
    let value = engine
        .eval(input)
        .unwrap_or_else(|error| panic!("`{input}` failed: {error}"));
    omega::format::render(
        &value,
        omega::format::Style {
            digits: 200,
            ..Default::default()
        },
    )
    .text
}

fn error_of(input: &str) -> String {
    let mut engine = Engine::new();
    match engine.eval(input) {
        Ok(value) => panic!(
            "`{input}` unexpectedly succeeded with {}",
            omega::format::render(&value, omega::format::Style::default()).text
        ),
        Err(error) => error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Exactness
// ---------------------------------------------------------------------------

#[test]
fn exact_results_are_never_abbreviated() {
    // Whatever the arithmetic produced is what gets printed, however long.
    for (expression, digits) in [
        ("2^64", 20),
        ("2^1000", 302),
        ("100!", 158),
        ("1000!", 2568),
        ("2^100000", 30103),
        ("5000!", 16326),
    ] {
        let rendered = calc(expression);
        let body: String = rendered.chars().filter(|c| c.is_ascii_digit()).collect();
        assert_eq!(
            body.len(),
            digits,
            "`{expression}` should print all {digits} digits, got {} characters",
            body.len()
        );
        assert!(
            !rendered.contains("..."),
            "`{expression}` was abbreviated: {rendered}"
        );
        assert!(
            !rendered.contains('#'),
            "`{expression}` carried a shortening note: {rendered}"
        );
    }
}

#[test]
fn timing_units_track_the_magnitude() {
    // Each magnitude reports in the unit that keeps significant digits, so a
    // fast result is never rounded away to `0.000 ms`.
    assert_eq!(format_duration(Duration::from_nanos(0)), "<1 us");
    assert_eq!(format_duration(Duration::from_nanos(1)), "1 ns");
    assert_eq!(format_duration(Duration::from_nanos(999)), "999 ns");
    assert_eq!(format_duration(Duration::from_nanos(1_500)), "1.500 us");
    assert_eq!(format_duration(Duration::from_micros(999)), "999.000 us");
    assert_eq!(format_duration(Duration::from_micros(1_500)), "1.500 ms");
    assert_eq!(format_duration(Duration::from_millis(999)), "999.000 ms");
    // A second or more shows seconds, with milliseconds kept alongside for
    // anyone who wants the finer figure.
    assert_eq!(
        format_duration(Duration::from_millis(1_500)),
        "1.500 s (1500.000 ms)"
    );
    assert_eq!(
        format_duration(Duration::from_millis(2_345)),
        "2.345 s (2345.000 ms)"
    );
}

#[test]
fn timing_preserves_sub_millisecond_detail() {
    // The whole point of not defaulting to milliseconds: a 300 us computation
    // must not display as `0.300 ms` and lose the unit, nor as `0 ms`.
    let micros = Duration::from_micros(300);
    let rendered = format_duration(micros);
    assert_eq!(rendered, "300.000 us");
    assert!(rendered.ends_with("us"), "{rendered}");
    assert_ne!(rendered, "0.000 ms");
}

#[test]
fn oversized_results_are_refused_with_their_real_size() {
    // This cannot be computed: it would need ~15.5 GiB. The error must say how
    // large the answer would have been rather than just "too large".
    let message = error_of("9999^9999999999");
    assert!(message.contains("39_999_565_680"), "{message}");
    assert!(message.contains("limit"), "{message}");
    assert!(message.contains("GiB"), "{message}");

    // An exponent beyond 64 bits is still sized rather than giving up.
    let message = error_of("2^(10^100)");
    assert!(message.contains("digits"), "{message}");
    assert!(message.contains("limit"), "{message}");

    // Something that does fit is still computed exactly: 9999^100 has 400
    // digits, and the leading and trailing digits are known.
    let value = calc("9999^100");
    assert_eq!(value.len(), 400, "9999^100 has 400 digits");
    assert!(value.starts_with("990049338691"), "{value}");
    assert!(value.ends_with("494999000001"), "{value}");
}

#[test]
fn size_reports_digit_counts_without_computing() {
    // Answers the question that cannot be answered by evaluating.
    assert_eq!(calc("size(9999, 9999999999)"), "39999565680");
    assert_eq!(calc("size(2, 1000000)"), "301030");
    assert_eq!(calc("size(10, 77)"), "78");
    assert_eq!(calc("size(2, 0)"), "1");

    // The estimate must agree with the real count for values that fit.
    for (base, exponent) in [
        (2u32, 100u32),
        (2, 1000),
        (3, 500),
        (9999, 50),
        (7, 1000),
        (123, 456),
    ] {
        let estimated = calc(&format!("size({base}, {exponent})"));
        let actual = calc(&format!("digits({base}^{exponent})"));
        assert_eq!(
            estimated, actual,
            "size({base}, {exponent}) disagreed with the real digit count"
        );
    }
}

#[test]
fn huge_integers_are_exact() {
    assert_eq!(calc("2^128"), "340282366920938463463374607431768211456");
    assert_eq!(calc("2^64"), "18446744073709551616");
    // 2^1000 has 302 digits; check the exact digit count and endpoints.
    let full = calc_full("2^1000").replace('_', "");
    assert_eq!(full.len(), 302);
    assert_eq!(
        full,
        "10715086071862673209484250490600018105614048117055336074437503883703510511249361224931983788156958581275946729175531468251871452856923140435984577574698574803934567774824230985421074605062371141877954182153046474983581941267398767559165543946077062914571196477686542167660429831652624386837205668069376"
    );
}

#[test]
fn division_keeps_exact_fractions() {
    assert_eq!(calc("1/3"), "1/3");
    assert_eq!(calc("1/3 + 1/6"), "1/2");
    assert_eq!(calc("2/4"), "1/2");
    assert_eq!(calc("10/4"), "5/2");
    // Exact division collapses back to an integer.
    assert_eq!(calc("4/2"), "2");
    assert_eq!(calc("100/10"), "10");
}

#[test]
fn decimals_are_exact_rationals() {
    // The classic binary floating point trap; exact rationals avoid it.
    assert_eq!(calc("0.1 + 0.2"), "0.3");
    assert_eq!(calc("0.1 + 0.2 - 0.3"), "0");
    assert_eq!(calc("1.5 * 4"), "6");
    assert_eq!(calc("10.5 / 3"), "7/2");
}

#[test]
fn negative_exponents_stay_exact() {
    assert_eq!(calc("2^-3"), "1/8");
    assert_eq!(calc("2^-10"), "1/1024");
    assert_eq!(calc("(2/3)^-2"), "9/4");
}

#[test]
fn factorial_is_exact() {
    assert_eq!(calc("0!"), "1");
    assert_eq!(calc("5!"), "120");
    assert_eq!(calc("20!"), "2432902008176640000");
    // 100! has exactly 158 digits.
    assert_eq!(calc_full("100!").replace('_', "").len(), 158);
    assert_eq!(
        calc_full("100!").replace('_', ""),
        "93326215443944152681699238856266700490715968264381621468592963895217599993229915608941463976156518286253697920827223758251185210916864000000000000000000000000"
    );
}

// ---------------------------------------------------------------------------
// Operator semantics
// ---------------------------------------------------------------------------

#[test]
fn precedence_follows_math_convention() {
    // Power binds tighter than unary minus, matching standard notation.
    assert_eq!(calc("-2^2"), "-4");
    assert_eq!(calc("(-2)^2"), "4");
    // Power is right associative.
    assert_eq!(calc("2^3^2"), "512");
    assert_eq!(calc("(2^3)^2"), "64");
    assert_eq!(calc("2 + 3 * 4"), "14");
    assert_eq!(calc("2 * 3 + 4"), "10");
    assert_eq!(calc("10 - 2 - 3"), "5");
}

#[test]
fn remainder_follows_dividend_sign() {
    assert_eq!(calc("10 % 3"), "1");
    assert_eq!(calc("-10 % 3"), "-1");
    assert_eq!(calc("10 % -3"), "1");
}

#[test]
fn number_literals_understand_bases() {
    assert_eq!(calc("0xff"), "255");
    assert_eq!(calc("0b1010"), "10");
    assert_eq!(calc("0o17"), "15");
    assert_eq!(calc("1000000"), "1000000");
    assert_eq!(calc("1e3"), "1000");
    assert_eq!(calc("0x10 + 0b1010 + 0o17"), "41");
}

#[test]
fn digits_are_printed_without_separators_by_default() {
    // The default output is a bare number, safe to pipe or paste anywhere.
    assert_eq!(calc("999"), "999");
    assert_eq!(calc("1000"), "1000");
    assert_eq!(calc("10000"), "10000");
    assert_eq!(calc("2^64"), "18446744073709551616");
    assert_eq!(calc("100!"), "93326215443944152681699238856266700490715968264381621468592963895217599993229915608941463976156518286253697920827223758251185210916864000000000000000000000000");
    // Typed separators are still accepted and ignored, so grouped input works.
    assert_eq!(calc("18_446_744_073_709_551_616 - 2^64"), "0");
    assert_eq!(calc("1_000 * 1_000"), "1000000");
}

#[test]
fn grouping_is_opt_in_and_configurable() {
    let value = "2^64";
    let cases = [
        // Underscore grouping happens when asked for.
        (
            omega::format::Grouping::Underscore,
            4,
            "18_446_744_073_709_551_616",
        ),
        (
            omega::format::Grouping::Comma,
            4,
            "18,446,744,073,709,551,616",
        ),
        (
            omega::format::Grouping::Space,
            4,
            "18 446 744 073 709 551 616",
        ),
        // Explicitly off, which is also the default.
        (omega::format::Grouping::None, 4, "18446744073709551616"),
        // A higher threshold leaves a shorter number ungrouped.
        (
            omega::format::Grouping::Underscore,
            7,
            "18_446_744_073_709_551_616",
        ),
    ];
    for (grouping, group_from, expected) in cases {
        let mut engine = Engine::new();
        let evaluated = engine.eval(value).unwrap();
        let rendered = omega::format::render(
            &evaluated,
            omega::format::Style {
                grouping,
                group_from,
                ..Default::default()
            },
        )
        .text;
        assert_eq!(rendered, expected, "grouping {grouping:?} at {group_from}");
    }

    // The threshold only suppresses grouping below it.
    let mut engine = Engine::new();
    let small = engine.eval("100000").unwrap();
    let rendered = omega::format::render(
        &small,
        omega::format::Style {
            grouping: omega::format::Grouping::Underscore,
            group_from: 7,
            ..Default::default()
        },
    )
    .text;
    assert_eq!(rendered, "100000");
}

#[test]
fn fractions_and_fractional_parts_are_not_grouped() {
    // Grouping a fraction would blur numerator and denominator together, so it
    // is skipped even when grouping is requested.
    let grouped = |expression: &str| {
        let mut engine = Engine::new();
        let evaluated = engine.eval(expression).unwrap();
        omega::format::render(
            &evaluated,
            omega::format::Style {
                grouping: omega::format::Grouping::Underscore,
                ..Default::default()
            },
        )
        .text
    };
    assert_eq!(grouped("1/1000000"), "1/1000000");
    assert_eq!(grouped("2^64 / 3"), "18446744073709551616/3");
    // Only the integer part of a decimal is grouped.
    assert_eq!(grouped("1234.5678901"), "1_234.5678901");
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn perfect_roots_return_exact_integers() {
    assert_eq!(calc("sqrt(4)"), "2");
    assert_eq!(calc("sqrt(16)"), "4");
    assert_eq!(calc("sqrt(0.25)"), "1/2");
    assert_eq!(calc("cbrt(27)"), "3");
    assert_eq!(calc("nthroot(1024, 10)"), "2");
    assert_eq!(
        calc("sqrt(10^100)"),
        "100000000000000000000000000000000000000000000000000"
    );
}

#[test]
fn irrational_roots_are_accurate() {
    assert!(calc("sqrt(2)").starts_with("1.4142135623730950488016887242097"));
    assert!(calc("sqrt(10)").starts_with("3.1622776601683793319988935444327"));
    assert!(calc("cbrt(2)").starts_with("1.2599210498948731647672106072782"));
    assert!(calc("nthroot(2, 10)").starts_with("1.0717734625362931642130063250233"));
    // Squaring an accurate root lands back on the argument. It is reported as
    // a real rather than promoted to an integer, because the value came from
    // rounding and claiming it as exact would overstate the arithmetic.
    assert!(calc("sqrt(2)*sqrt(2)").starts_with("2.000000000000000000000000000000"));
    assert!(calc("sqrt(3)*sqrt(3)").starts_with("3.000000000000000000000000000000"));
    assert!(calc("sqrt(5)*sqrt(5)").starts_with("5.000000000000000000000000000000"));
    // Perfect squares are recognised as exact from the start.
    assert_eq!(calc("sqrt(16)*sqrt(16)"), "16");
}

#[test]
fn constants_match_known_digits() {
    assert!(calc("pi").starts_with("3.1415926535897932384626433832795"));
    assert!(calc("e").starts_with("2.7182818284590452353602874713527"));
    assert!(calc("tau").starts_with("6.2831853071795864769252867665590"));
    assert!(calc("phi").starts_with("1.6180339887498948482045868343656"));
}

#[test]
fn logarithms_are_accurate() {
    assert!(calc("ln(2)").starts_with("0.69314718055994530941723212145818"));
    assert!(calc("ln(10)").starts_with("2.3025850929940456840179914546844"));
    assert!(calc("log10(1000)").starts_with("3.0"));
    assert!(calc("log(8, 2)").starts_with("3.0"));
    // Round trips through exp and ln must return the original.
    assert_eq!(calc("exp(ln(5))"), "5.0000000000000000000000000000000");
    assert_eq!(calc("ln(e)"), "1.0000000000000000000000000000000");
}

#[test]
fn trigonometry_is_accurate() {
    assert_eq!(calc("sin(0)"), "0");
    assert_eq!(calc("cos(0)"), "1");
    assert!(calc("sin(1)").starts_with("0.84147098480789650665250232163030"));
    assert!(calc("cos(1)").starts_with("0.54030230586813971740093660744298"));
    assert_eq!(calc("sin(pi/2)"), "1.0000000000000000000000000000000");
    assert_eq!(calc("cos(pi)"), "-1.0000000000000000000000000000000");
    // atan(1) * 4 must reproduce pi to every displayed digit.
    assert_eq!(calc("atan(1)*4"), calc("pi"));
    assert_eq!(calc("asin(1)*2"), calc("pi"));
}

#[test]
fn huge_trig_arguments_are_reduced() {
    // Reduction uses pi to the working precision, so this stays meaningful.
    let value = calc("sin(10^30)");
    assert!(
        value.starts_with("-0.090116901912138058030386428952987"),
        "{value}"
    );
    assert!(calc("atan(10^50)").starts_with("1.5707963267948966192313216916398"));
}

/// Rounds a reference decimal string to `digits` significant digits, ignoring
/// leading zeros, and returns just the digit sequence. Comparing sequences
/// avoids being distracted by a leading `0.` or the decimal point.
fn round_reference(digits: &str, keep: usize) -> String {
    // Significant digits start at the first non-zero digit.
    let mut significant: Vec<char> = digits
        .chars()
        .filter(|c| c.is_ascii_digit())
        .skip_while(|c| *c == '0')
        .collect();
    if significant.len() <= keep {
        return significant.into_iter().collect();
    }
    let round_up = significant[keep] >= '5';
    significant.truncate(keep);
    if round_up {
        let mut index = keep;
        loop {
            if index == 0 {
                significant.insert(0, '1');
                break;
            }
            index -= 1;
            if significant[index] == '9' {
                significant[index] = '0';
            } else {
                significant[index] = char::from_u32(significant[index] as u32 + 1).unwrap();
                break;
            }
        }
    }
    significant.into_iter().collect()
}

/// The significant digits omega displayed, with leading zeros dropped.
fn significant_digits(rendered: &str) -> String {
    rendered
        .chars()
        .filter(|c| c.is_ascii_digit())
        .skip_while(|c| *c == '0')
        .collect()
}

#[test]
fn high_precision_matches_reference_values() {
    // Reference digits from the standard published expansions.
    let references = [
        (
            "pi",
            "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745",
        ),
        (
            "sqrt(2)",
            "1.414213562373095048801688724209698078569671875376948073176679737990732478462107038850387534327641572735013846230912297024924836055850737212644121497099935831414",
        ),
        (
            "e",
            "2.718281828459045235360287471352662497757247093699959574966967627724076630353547594571382178525166427427466391932003059921817413596629043572900334295260595630738",
        ),
        (
            "ln(2)",
            "0.6931471805599453094172321214581765680755001343602552541206800094933936219696947156058633269964186875420014810205706857336855202357581305570326707516350759619307",
        ),
    ];

    for (expression, reference) in references {
        for digits in [50usize, 100] {
            let mut engine = Engine::new().with_precision(digits as u32);
            let value = engine
                .eval(expression)
                .unwrap_or_else(|error| panic!("`{expression}` failed: {error}"));
            let rendered = omega::format::render(
                &value,
                omega::format::Style {
                    digits,
                    ..Default::default()
                },
            )
            .text;
            let produced = significant_digits(&rendered);
            let expected = round_reference(reference, digits);
            assert_eq!(
                produced, expected,
                "`{expression}` at {digits} digits\n produced {rendered}\n expected {expected}"
            );
        }
    }
}

#[test]
fn precision_extends_to_a_thousand_digits() {
    let mut engine = Engine::new().with_precision(1000);
    let value = engine.eval("pi").unwrap();
    let rendered = omega::format::render(
        &value,
        omega::format::Style {
            digits: 1000,
            ..Default::default()
        },
    )
    .text;
    let produced = significant_digits(&rendered);
    assert_eq!(produced.len(), 1000);

    // pi to 1000 places, from the published expansion.
    let reference = "31415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679821480865132823066470938446095505822317253594081284811174502841027019385211055596446229489549303819644288109756659334461284756482337867831652712019091456485669234603486104543266482133936072602491412737245870066063155881748815209209628292540917153643678925903600113305305488204665213841469519415116094330572703657595919530921861173819326117931051185480744623799627495673518857527248912279381830119491298336733624406566430860213949463952247371907021798609437027705392171762931767523846748184676694051320005681271452635608277857713427577896091736371787214684409012249534301465495853710507922796892589235420199561121290219608640344181598136297747713099605187072113499999983729780499510597317328160963185950244594553469083026425223082533446850352619311881710100031378387528865875332083814206171776691473035982534904287554687311595628638823537875937519577818577805321712268066130019278766111959092164201989";
    let expected = round_reference(reference, 1000);
    assert_eq!(
        produced,
        expected,
        "pi at 1000 digits differs; produced ends ...{}, expected ends ...{}",
        &produced[produced.len() - 8..],
        &expected[expected.len() - 8..]
    );
}

#[test]
fn negative_bases_with_fractional_exponents() {
    // An odd denominator keeps the result real, and the sign is decided by the
    // numerator: an odd numerator keeps it negative.
    assert!(calc("(-8)^(1/3)").starts_with("-2.000000000000000000000000000000"));
    assert!(calc("(-8)^(2/3)").starts_with("4.000000000000000000000000000000"));
    assert!(calc("(-32)^(1/5)").starts_with("-2.000000000000000000000000000000"));
    assert!(calc("(-32)^(3/5)").starts_with("-8.000000000000000000000000000000"));
    // Consistency with the dedicated root function.
    assert_eq!(calc("cbrt(-8)"), "-2");
    assert_eq!(calc("cbrt(-27)"), "-3");
    // An even denominator has no real result.
    assert!(error_of("(-4)^(1/2)").contains("not a real number"));
    assert!(error_of("(-4)^(3/2)").contains("not a real number"));
}

#[test]
fn number_theory_helpers_work() {
    assert_eq!(calc("gcd(12, 18)"), "6");
    assert_eq!(calc("lcm(4, 6)"), "12");
    assert_eq!(calc("isprime(97)"), "1");
    assert_eq!(calc("isprime(100)"), "0");
    // A 39-digit Mersenne prime.
    assert_eq!(calc("isprime(2^127 - 1)"), "1");
    assert_eq!(calc("isprime(2^128 - 1)"), "0");
    assert_eq!(calc("digits(2^1000)"), "302");
}

#[test]
fn rounding_and_integer_functions() {
    assert_eq!(calc("floor(2.7)"), "2");
    assert_eq!(calc("floor(-2.1)"), "-3");
    assert_eq!(calc("ceil(2.1)"), "3");
    assert_eq!(calc("ceil(-2.7)"), "-2");
    assert_eq!(calc("trunc(-2.7)"), "-2");
    assert_eq!(calc("round(2.5)"), "3");
    assert_eq!(calc("round(-2.5)"), "-3");
    assert_eq!(calc("round(2.4)"), "2");
    assert_eq!(calc("abs(-5)"), "5");
    assert_eq!(calc("min(3, 7)"), "3");
    assert_eq!(calc("max(3, 7)"), "7");
}

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------

#[test]
fn variables_persist_within_a_session() {
    let mut engine = Engine::new();
    engine.eval("x = 5").unwrap();
    assert_eq!(
        omega::format::render(&engine.eval("x^2").unwrap(), Default::default()).text,
        "25"
    );
    engine.eval("y = x + 1").unwrap();
    assert_eq!(
        omega::format::render(&engine.eval("y").unwrap(), Default::default()).text,
        "6"
    );
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn division_by_zero_is_reported() {
    assert!(error_of("1/0").contains("division by zero"));
    assert!(error_of("1 % 0").contains("division by zero"));
}

#[test]
fn domain_errors_are_reported_clearly() {
    assert!(error_of("sqrt(-4)").contains("not a real number"));
    assert!(error_of("ln(0)").contains("only defined for positive"));
    assert!(error_of("ln(-1)").contains("only defined for positive"));
    assert!(error_of("asin(2)").contains("only defined on"));
}

#[test]
fn unknown_names_and_bad_syntax_are_reported() {
    assert!(error_of("nope(1)").contains("unknown function"));
    assert!(error_of("undefined_var").contains("not defined"));
    assert!(error_of("1 +").contains("syntax error"));
    assert!(error_of("(1 + 2").contains("syntax error"));
    assert!(error_of("1 2").contains("syntax error"));
}

#[test]
fn arity_is_checked() {
    assert!(error_of("sqrt(1, 2)").contains("takes 1 argument"));
    assert!(error_of("gcd(1)").contains("takes 2 argument"));
}
