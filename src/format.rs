//! Rendering values for display.
//!
//! Exact values are always printed in full: whatever the arithmetic produced is
//! what appears, however many digits it takes. Inexact values cannot be printed
//! in full because they have no finite decimal expansion, so they are shown to a
//! fixed number of significant digits and the number of *computed* digits is
//! reported alongside, so a shortened display is never mistaken for the whole
//! answer.

use crate::number::Real;
use crate::value::Value;
use num_bigint::BigInt;
use num_traits::Zero;

/// Significant digits shown for an inexact (real) value.
pub const REAL_DIGITS: usize = 32;

/// Decimal digits of the integer part, used in place of printing the number.
pub fn digit_count(value: &BigInt) -> usize {
    crate::number::decimal_digits(value)
}

/// Renders a value with no separators at all, used in error messages and
/// anywhere the text may be fed back into the parser.
pub fn render_plain(value: &Value) -> String {
    let plain = Style {
        grouping: Grouping::None,
        ..Style::default()
    };
    render(value, plain).text
}

/// How the integer part of a number is broken up for readability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    /// Print the digits with nothing between them. This is the default: the
    /// output is always a bare number, safe to pipe or paste anywhere.
    None,
    /// Insert `_` every three digits, once the integer part reaches the
    /// threshold below.
    Underscore,
    /// Insert `,` every three digits.
    Comma,
    /// Insert a space every three digits.
    Space,
}

impl Grouping {
    fn separator(self) -> Option<char> {
        match self {
            Grouping::Underscore => Some('_'),
            Grouping::Comma => Some(','),
            Grouping::Space => Some(' '),
            Grouping::None => None,
        }
    }
}

/// How a value should be displayed.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Significant digits used for inexact results. Exact results ignore this
    /// and are always printed in full.
    pub digits: usize,
    /// Separator inserted between groups of three digits. Off by default.
    pub grouping: Grouping,
    /// Grouping only applies once the integer part has at least this many
    /// digits. Four is the point where a separator starts helping readability.
    pub group_from: usize,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            digits: REAL_DIGITS,
            grouping: Grouping::None,
            group_from: 4,
        }
    }
}

/// The rendered text for a value, plus an optional note about how much was
/// withheld. A note means the displayed text is not the entire number.
pub struct Rendered {
    pub text: String,
    pub note: Option<String>,
}

pub fn render(value: &Value, style: Style) -> Rendered {
    match value {
        // Integers and fractions are exact, so every digit they hold is shown.
        Value::Int(v) => Rendered {
            text: group_thousands(&v.to_string(), style),
            note: None,
        },
        Value::Rational(v) => Rendered {
            // A fraction is left alone: grouping `7/1_000_000` would make the
            // numerator and denominator harder to tell apart, not easier.
            text: v.to_string(),
            note: None,
        },
        Value::Real(v) => {
            let text = render_real(v, style);
            let exact = exact_digits_available(v);
            let note = if exact > style.digits {
                Some(format!(
                    "{} significant digits shown, {} computed; pass --digits <n> for more",
                    style.digits, exact
                ))
            } else {
                None
            };
            Rendered { text, note }
        }
    }
}

/// Significant digits the real actually carries, inferred from how far the
/// mantissa was scaled.
fn exact_digits_available(value: &Real) -> usize {
    if value.is_zero() {
        return 1;
    }
    // Only the count is wanted, so measure it from the bit length rather than
    // converting the whole mantissa to decimal a second time.
    let mantissa_digits = crate::number::decimal_digits(value.mantissa());
    mantissa_digits
        .saturating_sub(value.scale() as usize)
        .max(1)
}

/// Renders a real to `style.digits` significant digits in plain decimal
/// notation, falling back to scientific notation when the exponent is unwieldy.
pub fn render_real(value: &Real, style: Style) -> String {
    if value.is_zero() {
        return "0".to_string();
    }
    let negative = value.is_negative();

    // Truncate to `digits` significant digits with correct rounding.
    let rounded = value
        .abs()
        .round_significant(style.digits, crate::number::Rounding::HalfAwayFromZero);
    let mantissa = rounded.mantissa();
    let scale = rounded.scale();
    if mantissa.is_zero() {
        return "0".to_string();
    }
    let raw = mantissa.to_str_radix(10);
    let total = raw.len() as i64;
    let exponent = total - scale as i64 - 1;

    let body = if (-6..21).contains(&exponent) {
        plain_decimal(&raw, scale, style)
    } else {
        scientific(&raw, exponent)
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// Places the decimal point inside `digits`, which is scaled by `10^scale`.
fn plain_decimal(digits: &str, scale: u32, style: Style) -> String {
    let scale = scale as usize;
    if scale == 0 {
        return group_thousands(digits, style);
    }
    if digits.len() > scale {
        let split = digits.len() - scale;
        let (int_part, frac_part) = digits.split_at(split);
        format!("{}.{}", group_thousands(int_part, style), frac_part)
    } else {
        let zeros = "0".repeat(scale - digits.len());
        format!("0.{zeros}{digits}")
    }
}

/// `d.ddddEn` with only the significant digits that were computed.
fn scientific(digits: &str, exponent: i64) -> String {
    let (head, tail) = digits.split_at(1);
    if tail.is_empty() {
        format!("{head}e{exponent}")
    } else {
        format!("{head}.{tail}e{exponent}")
    }
}

/// Inserts separators every three digits of the integer part.
///
/// Grouping only starts at `style.group_from` digits, and the fractional part
/// is never grouped. Passing `Grouping::None` disables it entirely.
fn group_thousands(digits: &str, style: Style) -> String {
    let separator = match style.grouping.separator() {
        Some(separator) => separator,
        None => return digits.to_string(),
    };
    let (sign, body) = match digits.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", digits),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if int_part.len() < style.group_from {
        return match frac_part {
            Some(f) => format!("{sign}{int_part}.{f}"),
            None => format!("{sign}{int_part}"),
        };
    }
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    let offset = int_part.len() % 3;
    for (index, ch) in int_part.chars().enumerate() {
        if index > 0 && (index + 3 - offset) % 3 == 0 {
            grouped.push(separator);
        }
        grouped.push(ch);
    }
    match frac_part {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Compact digit count, used by the `nth` helpers and diagnostics.
pub fn summarize(value: &Value) -> usize {
    match value {
        Value::Int(v) if !v.is_zero() => digit_count(v),
        _ => 1,
    }
}
