//! The evaluated value type and the promotion rules between numeric kinds.

use crate::number::{Int, NumError, Rational, Real, DEFAULT_PRECISION};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;

/// A calculator value. The variants form a tower: a value starts as an exact
/// integer, widens to a rational when a division does not divide evenly, and
/// widens to a real only when an operation has no exact answer.
#[derive(Clone, Debug)]
pub enum Value {
    Int(Int),
    Rational(Rational),
    Real(Real),
}

/// For an exact fraction `p/q` in lowest terms, reports whether `p` and `q` are
/// odd. Returns `None` when the exponent is not an exact fraction, in which
/// case the sign of a negative base cannot be reasoned about.
fn parity(exponent: &Value) -> Option<(bool, bool)> {
    let rational = match exponent {
        Value::Rational(r) => r,
        Value::Int(_) | Value::Real(_) => return None,
    };
    let odd_denominator = num_integer::Integer::is_odd(rational.denominator());
    // The numerator of a value like `1/3` is 1; for `2/3` it is 2. Negative
    // numerators use the magnitude, since the sign is handled by the caller.
    let odd_numerator = num_integer::Integer::is_odd(&rational.numerator().abs());
    Some((odd_denominator, odd_numerator))
}

/// The denominator of an exact fraction, for error messages.
fn denominator_of(exponent: &Value) -> String {
    match exponent {
        Value::Rational(r) => r.denominator().to_string(),
        _ => "even".to_string(),
    }
}

impl Value {
    pub fn from_big(int: BigInt) -> Value {
        Value::Int(int)
    }

    pub fn zero() -> Value {
        Value::Int(BigInt::zero())
    }

    pub fn one() -> Value {
        Value::Int(BigInt::one())
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Value::Int(v) => v.is_zero(),
            Value::Rational(v) => v.is_zero(),
            Value::Real(v) => v.is_zero(),
        }
    }

    pub fn is_negative(&self) -> bool {
        match self {
            Value::Int(v) => v.is_negative(),
            Value::Rational(v) => v.is_negative(),
            Value::Real(v) => v.is_negative(),
        }
    }

    /// True when the value is exactly representable without rounding.
    pub fn is_exact(&self) -> bool {
        !matches!(self, Value::Real(_))
    }

    /// The precision this value should carry through an operation.
    pub fn precision(&self) -> u32 {
        match self {
            Value::Real(v) => v.scale().max(DEFAULT_PRECISION),
            _ => DEFAULT_PRECISION,
        }
    }

    /// Widens to a real, rounding to `precision` decimal places.
    pub fn to_real(&self, precision: u32) -> Real {
        match self {
            Value::Int(v) => Real::from_int(v),
            Value::Rational(v) => Real::from_int(v.numerator())
                .div(&Real::from_int(v.denominator()), precision)
                .unwrap_or_else(|_| Real::zero()),
            Value::Real(v) => v.clone(),
        }
    }

    /// Widens to an exact rational. Only reals cannot be represented exactly,
    /// so they are converted through their decimal expansion.
    pub fn to_rational(&self) -> Rational {
        match self {
            Value::Int(v) => Rational::from_int(v.clone()),
            Value::Rational(v) => v.clone(),
            Value::Real(v) => {
                Rational::new_unchecked(v.mantissa().clone(), crate::number::pow10(v.scale()))
            }
        }
    }

    /// Recovers an exact integer when the value genuinely is one, so that
    /// `4/2` reports as `2` rather than `2.0`.
    ///
    /// A real is only collapsed when it is exact by construction. A real that
    /// came out of rounding (say `sqrt(3)*sqrt(3)`, which rounds to `3.000...0`
    /// at the working precision) is left as a real, because promoting it to an
    /// integer would claim a precision the arithmetic never established.
    pub fn simplify(self) -> Value {
        match self {
            Value::Rational(r) if r.is_integer() => Value::Int(r.numerator().clone()),
            Value::Real(r) if r.is_exact_integer() && r.is_literal_exact() => {
                Value::Int(r.truncate())
            }
            other => other,
        }
    }

    /// Compares two values exactly, without rounding to a common type.
    pub fn cmp_value(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Real(a), Value::Real(b)) => a.cmp_exact(b),
            (Value::Real(a), b) => a.cmp_exact(&b.to_real(a.scale().max(DEFAULT_PRECISION))),
            (a, Value::Real(b)) => a.to_real(b.scale().max(DEFAULT_PRECISION)).cmp_exact(b),
            _ => self.to_rational().cmp_exact(&other.to_rational()),
        }
    }

    pub fn to_f64(&self) -> f64 {
        match self {
            Value::Int(v) => v.to_f64().unwrap_or(f64::NAN),
            Value::Rational(v) => v.to_f64(),
            Value::Real(v) => v.to_f64(),
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => v.to_i64(),
            Value::Rational(v) if v.is_integer() => v.numerator().to_i64(),
            Value::Real(v) => v.to_f64().to_i64(),
            _ => None,
        }
    }

    pub fn to_u32(&self) -> Option<u32> {
        match self {
            Value::Int(v) => v.to_u32(),
            Value::Rational(v) if v.is_integer() => v.numerator().to_u32(),
            Value::Real(v) => v.to_f64().to_u32(),
            _ => None,
        }
    }

    pub fn to_usize(&self) -> Option<usize> {
        self.to_u32().map(|v| v as usize)
    }

    /// Exact integer value, when the number is one.
    pub fn as_int(&self) -> Option<&BigInt> {
        match self {
            Value::Int(v) => Some(v),
            Value::Rational(v) if v.is_integer() => Some(v.numerator()),
            _ => None,
        }
    }

    pub fn neg(&self) -> Value {
        match self {
            Value::Int(v) => Value::Int(-v),
            Value::Rational(v) => Value::Rational(v.neg()),
            Value::Real(v) => Value::Real(v.neg()),
        }
    }

    pub fn abs(&self) -> Value {
        match self {
            Value::Int(v) => Value::Int(v.abs()),
            Value::Rational(v) => Value::Rational(v.abs()),
            Value::Real(v) => Value::Real(v.abs()),
        }
    }

    pub fn add(&self, other: &Value) -> Result<Value, NumError> {
        Ok(match (self, other) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                let precision = self.precision().max(other.precision());
                Value::Real(
                    self.to_real(precision)
                        .add(&other.to_real(precision))
                        .round_significant(
                            precision as usize,
                            crate::number::Rounding::HalfAwayFromZero,
                        ),
                )
            }
            _ => Value::Rational(self.to_rational().add(&other.to_rational())),
        }
        .simplify())
    }

    pub fn sub(&self, other: &Value) -> Result<Value, NumError> {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Value) -> Result<Value, NumError> {
        Ok(match (self, other) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                let precision = self.precision().max(other.precision());
                Value::Real(
                    self.to_real(precision)
                        .mul_to(&other.to_real(precision), precision),
                )
            }
            _ => Value::Rational(self.to_rational().mul(&other.to_rational())),
        }
        // `1.5 * 4` is exactly 6, so it should read as `6`, not `6.0`.
        .simplify())
    }

    /// Division that stays exact whenever it can: `4/2` is `2`, `1/3` is the
    /// rational `1/3`, `10.5 / 3` is `7/2`, and only a genuinely inexact
    /// operand (a previously rounded result such as `pi`) forces a real.
    pub fn div(&self, other: &Value) -> Result<Value, NumError> {
        if other.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        Ok(match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                let (quotient, remainder) = num_integer::Integer::div_rem(a, b);
                if remainder.is_zero() {
                    Value::Int(quotient)
                } else {
                    Value::Rational(Rational::new_unchecked(a.clone(), b.clone()))
                }
            }
            // A decimal literal or an exact integer is a rational in disguise,
            // so both operands being exact means the quotient can be exact too.
            (lhs, rhs) if lhs.is_decimal_exact() && rhs.is_decimal_exact() => {
                Value::Rational(lhs.to_rational().div(&rhs.to_rational())?)
            }
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                let precision = self.precision().max(other.precision());
                Value::Real(
                    self.to_real(precision)
                        .div_exact_or(&other.to_real(precision), precision)?,
                )
            }
            _ => {
                let result = self.to_rational().div(&other.to_rational())?;
                Value::Rational(result)
            }
        }
        .simplify())
    }

    /// True when the value is stored exactly: an integer, a fraction, or a
    /// real that came from a decimal literal rather than from rounding.
    fn is_decimal_exact(&self) -> bool {
        match self {
            Value::Int(_) | Value::Rational(_) => true,
            Value::Real(v) => v.is_literal_exact(),
        }
    }

    /// The remainder, following the sign of the dividend like Rust's `%`.
    pub fn rem(&self, other: &Value) -> Result<Value, NumError> {
        if other.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        if let (Some(a), Some(b)) = (self.as_int(), other.as_int()) {
            return Ok(Value::Int(a % b));
        }
        let precision = self.precision().max(other.precision());
        let a = self.to_real(precision);
        let b = other.to_real(precision);
        let quotient = a.div(&b, precision)?;
        // a - b * trunc(a / b)
        let truncated = Real::from_int(&quotient.truncate());
        Ok(Value::Real(a.sub(&b.mul(&truncated))).simplify())
    }

    /// `self` raised to `exponent`. Integer exponents are computed exactly
    /// whenever possible, including negative ones (which produce a rational),
    /// so `2^1000` and `2^-3` are both exact.
    pub fn pow(&self, exponent: &Value) -> Result<Value, NumError> {
        if let Some(exp) = exponent.as_int() {
            return self.pow_int(exp);
        }
        // A fractional exponent means a root, which is generally irrational.
        let precision = self.precision().max(exponent.precision());
        let base = self.to_real(precision);
        let exp = exponent.to_real(precision);
        if base.is_negative() {
            // `x^(p/q)` in lowest terms is real for negative `x` exactly when
            // `q` is odd: the root supplies the sign, and the `p`-th power
            // then decides whether it survives. So `(-8)^(1/3)` is `-2` while
            // `(-8)^(2/3)` is `+4`.
            let (odd_root, odd_power) = match parity(exponent) {
                Some(parity) => parity,
                // A non-rational exponent over a negative base is undefined.
                None => {
                    return Err(NumError::Domain(format!(
                        "a negative base with a fractional exponent has no real result (base {})",
                        crate::format::render_plain(self)
                    )))
                }
            };
            if !odd_root {
                return Err(NumError::NegativeRoot {
                    degree: denominator_of(exponent),
                });
            }
            let magnitude = base.abs();
            let result = crate::eval::real_pow(&magnitude, &exp, precision)?;
            return Ok(Value::Real(if odd_power { result.neg() } else { result }));
        }
        if base.is_zero() {
            if exp.is_negative() {
                return Err(NumError::DivisionByZero);
            }
            return Ok(Value::zero());
        }
        let result = crate::eval::real_pow(&base, &exp, precision)?;
        Ok(Value::Real(result))
    }

    fn pow_int(&self, exponent: &BigInt) -> Result<Value, NumError> {
        if exponent.is_zero() {
            return Ok(Value::one());
        }
        let negative = exponent.is_negative();
        let magnitude = exponent.abs();

        // The real constraint is how many digits the result would have, not the
        // size of the exponent itself. `2^1000000` has a four-digit exponent and
        // is instant; `9999^9999999999` has a ten-digit exponent and would need
        // roughly 40 GB. Both limits are enforced below, but the size of the
        // result is what decides, so it is what gets checked and reported.
        let digits = self.result_digits(&magnitude);
        if let Some(digits) = digits {
            if digits > MAX_EXACT_DIGITS {
                return Err(NumError::Overflow(format!(
                    "the exact result would have about {} digits, which exceeds the {} digit limit ({})",
                    group_digits(digits),
                    group_digits(MAX_EXACT_DIGITS),
                    self.describe_size(digits)
                )));
            }
        }

        let result = match self {
            Value::Int(base) => {
                let exp = magnitude
                    .to_u32()
                    .ok_or_else(|| NumError::Overflow(self.describe_unsizeable(&magnitude)))?;
                Value::Int(base.pow(exp))
            }
            Value::Rational(base) => {
                let exp = magnitude
                    .to_u32()
                    .ok_or_else(|| NumError::Overflow(self.describe_unsizeable(&magnitude)))?;
                Value::Rational(Rational::new_unchecked(
                    base.numerator().pow(exp),
                    base.denominator().pow(exp),
                ))
            }
            Value::Real(base) => {
                let exp = magnitude
                    .to_u32()
                    .ok_or_else(|| NumError::Overflow(self.describe_unsizeable(&magnitude)))?;
                let precision = base.scale().max(DEFAULT_PRECISION);
                Value::Real(base.pow_u32(exp, precision).round_significant(
                    precision as usize,
                    crate::number::Rounding::HalfAwayFromZero,
                ))
            }
        };

        if negative {
            if result.is_zero() {
                return Err(NumError::DivisionByZero);
            }
            Value::one().div(&result)
        } else {
            Ok(result)
        }
    }

    /// Decimal digits the result of raising `self` to `exponent` would have, or
    /// `None` when the figure cannot be bounded cheaply.
    ///
    /// Uses `digits(base^e) = 1 + floor(e * log10(base))`, which is exact for
    /// the magnitudes involved here apart from a possible off-by-one. Only when
    /// the result would be small does the exact value matter, and there `e`
    /// always fits comfortably in an `f64`.
    pub fn result_digits(&self, exponent: &BigInt) -> Option<u128> {
        if exponent.is_zero() {
            return Some(1);
        }
        // An `f64` represents the exponent with ~15 significant digits, which
        // is far more than enough to decide between "fits" and "needs 15 GiB".
        // Converting through `f64` also handles exponents beyond `u64`, such as
        // `2^(2^64)`, instead of giving up on them.
        let exponent = exponent.abs().to_f64()?;
        if !exponent.is_finite() || exponent < 0.0 {
            return None;
        }
        // log10(|base|) as an f64 is ample for the same reason.
        let log10_base = match self {
            Value::Int(base) => base.abs().to_f64()?.log10(),
            Value::Rational(base) => {
                let n = base.numerator().abs().to_f64()?;
                let d = base.denominator().to_f64()?;
                if d == 0.0 || n == 0.0 {
                    return Some(1);
                }
                (n / d).log10()
            }
            Value::Real(base) => {
                if base.is_zero() {
                    return Some(1);
                }
                base.abs().to_f64().log10()
            }
        };
        if !log10_base.is_finite() {
            return None;
        }
        if log10_base <= 0.0 {
            // |base| <= 1, so the result never exceeds a couple of digits.
            return Some(1);
        }
        let digits = exponent * log10_base + 1.0;
        if !digits.is_finite() {
            // The count itself overflows; report the largest u128 so callers
            // treat it as impossibly large rather than as unknown.
            return Some(u128::MAX);
        }
        if digits >= u128::MAX as f64 {
            return Some(u128::MAX);
        }
        Some(digits as u128)
    }

    /// A human-sized description of how much memory a result of `digits` digits
    /// would occupy, so an oversized request reads as a physical fact.
    fn describe_size(&self, digits: u128) -> String {
        // A decimal digit needs slightly over 3.32 bits; report bytes.
        const BYTES_PER_DIGIT: f64 = 0.415;
        if digits == u128::MAX {
            return "more memory than this universe holds".to_string();
        }
        let bytes = digits as f64 * BYTES_PER_DIGIT;
        let gib = bytes / (1024.0 * 1024.0 * 1024.0);
        if gib >= 1.0e9 {
            // Beyond a billion gibibytes the exponent is the readable figure.
            format!("roughly {:.1}e9 GiB of memory", gib / 1.0e9)
        } else if gib >= 1.0 {
            format!("roughly {gib:.1} GiB of memory")
        } else {
            let mib = bytes / (1024.0 * 1024.0);
            format!("roughly {mib:.1} MiB of memory")
        }
    }

    /// Message for an exponent that is too large even to size up.
    ///
    /// Reached only when the base is negative or otherwise unmeasurable; a
    /// positive base beyond all limits is caught by the digit check instead,
    /// which produces the more informative message.
    fn describe_unsizeable(&self, exponent: &BigInt) -> String {
        format!(
            "raising this base to {} would produce a result too large to expand exactly",
            crate::format::render_plain(&Value::Int(exponent.clone()))
        )
    }
}

/// Largest number of decimal digits a single exact result may have.
///
/// This is a memory bound, not a mathematical one: a result is held as a
/// `BigInt` and then rendered to decimal, so an exabyte-scale answer is not
/// something this process can produce. Set high enough that any computation a
/// person would actually wait for succeeds.
pub const MAX_EXACT_DIGITS: u128 = 1_000_000_000;

/// Inserts `_` every three digits of a plain integer count.
fn group_digits(value: u128) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    let offset = text.len() % 3;
    for (index, ch) in text.chars().enumerate() {
        if index > 0 && (index + 3 - offset) % 3 == 0 {
            out.push('_');
        }
        out.push(ch);
    }
    out
}
