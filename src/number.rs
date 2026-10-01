//! The numeric tower: unbounded integers, exact rationals, and high-precision
//! reals.
//!
//! # Precision policy
//!
//! A real carries its own working precision (a decimal digit count). Binary
//! operations use the *maximum* of the operand precisions, so precision is
//! never silently lost; a result only rounds when the user asks for a shorter
//! decimal expansion. Values are only converted to reals when an operation has
//! no exact answer - `1/4` stays rational, `sqrt(4)` stays an integer.

use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer as _;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::fmt;

/// Decimal digits of working precision used when the caller does not ask for
/// more. Chosen so that a default `pi`/`sqrt` agrees with a double to the last
/// bit while staying cheap.
pub const DEFAULT_PRECISION: u32 = 64;

/// Default number of significant digits shown when a value is formatted.
pub const DEFAULT_DIGITS: usize = 32;

pub type Int = BigInt;

// ---------------------------------------------------------------------------
// Rationals
// ---------------------------------------------------------------------------

/// An exact fraction kept in lowest terms with a positive denominator.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    num: BigInt,
    den: BigInt,
}

impl Rational {
    pub fn new(num: BigInt, den: BigInt) -> Result<Rational, NumError> {
        if den.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        Ok(Rational::new_unchecked(num, den))
    }

    /// Normalizes a fraction that is known to have a non-zero denominator.
    pub fn new_unchecked(num: BigInt, den: BigInt) -> Rational {
        let mut num = num;
        let mut den = den;
        if den.is_negative() {
            num = -num;
            den = -den;
        }
        let g = num.gcd(&den);
        if !g.is_one() && !g.is_zero() {
            num /= &g;
            den /= &g;
        }
        Rational { num, den }
    }

    pub fn from_int(value: BigInt) -> Rational {
        Rational {
            num: value,
            den: BigInt::one(),
        }
    }

    pub fn numerator(&self) -> &BigInt {
        &self.num
    }

    pub fn denominator(&self) -> &BigInt {
        &self.den
    }

    pub fn is_integer(&self) -> bool {
        self.den.is_one()
    }

    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.num.is_negative()
    }

    pub fn neg(&self) -> Rational {
        Rational {
            num: -self.num.clone(),
            den: self.den.clone(),
        }
    }

    pub fn abs(&self) -> Rational {
        Rational {
            num: self.num.abs(),
            den: self.den.clone(),
        }
    }

    pub fn add(&self, other: &Rational) -> Rational {
        Rational::new_unchecked(
            &self.num * &other.den + &other.num * &self.den,
            &self.den * &other.den,
        )
    }

    pub fn sub(&self, other: &Rational) -> Rational {
        Rational::new_unchecked(
            &self.num * &other.den - &other.num * &self.den,
            &self.den * &other.den,
        )
    }

    pub fn mul(&self, other: &Rational) -> Rational {
        Rational::new_unchecked(&self.num * &other.num, &self.den * &other.den)
    }

    pub fn div(&self, other: &Rational) -> Result<Rational, NumError> {
        if other.num.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        Ok(Rational::new_unchecked(
            &self.num * &other.den,
            &self.den * &other.num,
        ))
    }

    /// Exact value when the fraction is a perfect `n`-th root, otherwise `None`.
    pub fn nth_root_exact(&self, n: &BigInt) -> Option<Rational> {
        if n <= &BigInt::zero() {
            return None;
        }
        if self.num.is_negative() {
            // Only odd roots of negative numbers stay real.
            if n.is_even() {
                return None;
            }
            let r = exact_nth_root(&(-&self.num), n)?;
            let d = exact_nth_root(&self.den, n)?;
            return Some(Rational::new_unchecked(-r, d));
        }
        let r = exact_nth_root(&self.num, n)?;
        let d = exact_nth_root(&self.den, n)?;
        Some(Rational::new_unchecked(r, d))
    }

    /// Truncated integer part.
    pub fn trunc(&self) -> BigInt {
        &self.num / &self.den
    }

    pub fn cmp_exact(&self, other: &Rational) -> Ordering {
        (&self.num * &other.den).cmp(&(&other.num * &self.den))
    }

    pub fn to_f64(&self) -> f64 {
        self.num.to_f64().unwrap_or(f64::NAN) / self.den.to_f64().unwrap_or(f64::NAN)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den.is_one() {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

/// Returns the exact integer `n`-th root when the input is a perfect power.
fn exact_nth_root(value: &BigInt, n: &BigInt) -> Option<BigInt> {
    if value.is_zero() {
        return Some(BigInt::zero());
    }
    if value.is_one() {
        return Some(BigInt::one());
    }
    let exponent = n.to_u32()?;
    if exponent == 0 {
        return None;
    }
    if exponent == 1 {
        return Some(value.clone());
    }
    // Bit length bounds: the root has at most ceil(bits / exponent) bits.
    let magnitude = value.magnitude();
    let bits = magnitude.bits();
    let mut low = BigUint::zero();
    let mut high = BigUint::one() << (bits / exponent as u64 + 1);
    while low < high {
        let mid = (&low + &high) >> 1u32;
        match pow_biguint(&mid, exponent).cmp(magnitude) {
            Ordering::Equal => return Some(BigInt::from_biguint(Sign::Plus, mid)),
            Ordering::Less => low = &mid + 1u32,
            Ordering::Greater => high = mid,
        }
    }
    None
}

fn pow_biguint(base: &BigUint, exponent: u32) -> BigUint {
    base.pow(exponent)
}

// ---------------------------------------------------------------------------
// High-precision reals
// ---------------------------------------------------------------------------

/// A fixed-point number scaled by `10^scale`, so `value = mantissa / 10^scale`.
///
/// Decimal scaling keeps printing exact and makes the rounding mode obvious;
/// guard digits absorbed into `scale` keep the error where it belongs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Real {
    mantissa: BigInt,
    scale: u32,
}

const GUARD_DIGITS: u32 = 8;

impl Real {
    pub fn zero() -> Real {
        Real {
            mantissa: BigInt::zero(),
            scale: 0,
        }
    }

    pub fn from_int(value: &BigInt) -> Real {
        Real {
            mantissa: value.clone(),
            scale: 0,
        }
    }

    /// Builds a real from a scaled mantissa: `mantissa / 10^scale`.
    pub fn from_scaled(mantissa: BigInt, scale: u32) -> Real {
        Real { mantissa, scale }
    }

    pub fn mantissa(&self) -> &BigInt {
        &self.mantissa
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn is_zero(&self) -> bool {
        self.mantissa.is_zero()
    }

    pub fn is_positive(&self) -> bool {
        self.mantissa.is_positive()
    }

    pub fn is_negative(&self) -> bool {
        self.mantissa.is_negative()
    }

    /// True when the value is an integer and carries no fractional tail, so it
    /// can be folded back into an exact integer without losing anything.
    pub fn is_exact_integer(&self) -> bool {
        self.frac_part_is_zero()
    }

    /// True when this real is exact by construction rather than the rounded
    /// output of an operation.
    ///
    /// A decimal literal such as `10.5` keeps the minimal scale that writes it
    /// down, while a computed result is padded out to the working precision
    /// (see [`DEFAULT_DIGITS`]). The scale therefore distinguishes the two.
    pub fn is_literal_exact(&self) -> bool {
        (self.scale as usize) < DEFAULT_DIGITS
    }

    /// Number of decimal digits in the integer part of the value.
    pub fn integer_digits(&self) -> usize {
        decimal_digits(&self.truncate().abs()).max(1)
    }

    /// True when `|self| < 10^-epsilon_digits`, decided without building the
    /// comparison value.
    ///
    /// Series summations test this on every iteration; constructing `10^-n` as
    /// a `Real` each time allocated a large integer per step, which dominated
    /// the cost of the transcendentals. Counting digits in the mantissa answers
    /// the same question by inspection.
    pub fn is_below_epsilon(&self, epsilon_digits: u32) -> bool {
        if self.mantissa.is_zero() {
            return true;
        }
        // |self| < 10^-e  iff  mantissa / 10^scale < 10^-e
        //                 iff  mantissa < 10^(scale - e)
        //                 iff  digits(mantissa) <= scale - e
        let mantissa_digits = decimal_digits(&self.mantissa.abs()) as u64;
        let scale = self.scale as u64;
        let exponent = epsilon_digits as u64;
        scale >= exponent && mantissa_digits <= scale - exponent
    }

    fn frac_part_is_zero(&self) -> bool {
        if self.scale == 0 {
            return true;
        }
        (&self.mantissa % pow10(self.scale)).is_zero()
    }

    pub fn neg(&self) -> Real {
        Real {
            mantissa: -self.mantissa.clone(),
            scale: self.scale,
        }
    }

    pub fn abs(&self) -> Real {
        Real {
            mantissa: self.mantissa.abs(),
            scale: self.scale,
        }
    }

    /// Rescales to `scale` digits, rounding half away from zero.
    pub fn with_scale(&self, scale: u32, rounding: Rounding) -> Real {
        if scale >= self.scale {
            let factor = pow10(scale - self.scale);
            return Real {
                mantissa: &self.mantissa * factor,
                scale,
            };
        }
        Real {
            mantissa: round_div(&self.mantissa, &pow10(self.scale - scale), rounding),
            scale,
        }
    }

    /// Rounds the value to `digits` significant digits.
    ///
    /// The leading digit of `mantissa * 10^-scale` sits at decimal exponent
    /// `magnitude_digits - scale - 1`. Keeping `digits` significant digits
    /// means rounding to that exponent minus `digits - 1`, which is a rescale
    /// to `scale + digits - magnitude_digits`.
    pub fn round_significant(&self, digits: usize, rounding: Rounding) -> Real {
        if self.mantissa.is_zero() || digits == 0 {
            return Real::zero();
        }
        let magnitude_digits = decimal_digits(&self.mantissa.abs()) as i64;
        if magnitude_digits <= digits as i64 {
            return self.clone();
        }
        let target = self.scale as i64 + digits as i64 - magnitude_digits;
        let target_scale = target.max(0) as u32;
        self.with_scale(target_scale, rounding)
    }

    pub fn add(&self, other: &Real) -> Real {
        let scale = self.scale.max(other.scale);
        let a = self.with_scale(scale, Rounding::HalfAwayFromZero);
        let b = other.with_scale(scale, Rounding::HalfAwayFromZero);
        Real {
            mantissa: a.mantissa + b.mantissa,
            scale,
        }
    }

    pub fn sub(&self, other: &Real) -> Real {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Real) -> Real {
        Real {
            mantissa: &self.mantissa * &other.mantissa,
            scale: self.scale + other.scale,
        }
    }

    /// Multiplication reduced to `precision` significant digits. The exact
    /// product is formed first and rounded once, which avoids compounding the
    /// error of the operands.
    pub fn mul_to(&self, other: &Real, precision: u32) -> Real {
        self.mul(other)
            .round_significant(precision as usize, Rounding::HalfAwayFromZero)
    }

    /// `self` raised to a non-negative integer power, keeping `precision`
    /// significant digits. Uses binary exponentiation, so the intermediate
    /// values stay bounded even for large exponents.
    pub fn pow_u32(&self, exponent: u32, precision: u32) -> Real {
        if exponent == 0 {
            return Real::from_int(&BigInt::one());
        }
        let mut result = Real::from_int(&BigInt::one());
        let mut base = self.clone();
        let mut remaining = exponent;
        while remaining > 0 {
            if remaining & 1 == 1 {
                result = result.mul_to(&base, precision);
            }
            remaining >>= 1;
            if remaining > 0 {
                base = base.mul_to(&base, precision);
            }
        }
        result
    }

    /// Division rounded to `precision` significant digits of the *result*.
    ///
    /// The result is `(a * 10^sa') / (b * 10^sb')`, so its decimal exponent can
    /// differ wildly from the operands'. The quotient is therefore computed at
    /// a scale derived from the operand magnitudes plus the requested
    /// significant digits, instead of a fixed scale that would round a tiny
    /// quotient to zero.
    pub fn div(&self, other: &Real, precision: u32) -> Result<Real, NumError> {
        if other.mantissa.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        let significant = precision.max(1) as i64 + GUARD_DIGITS as i64;

        // Decimal exponent of the leading digit of each operand.
        let a_digits = decimal_digits(&self.mantissa.abs()) as i64;
        let b_digits = decimal_digits(&other.mantissa.abs()) as i64;
        // Value of a / b is roughly 10^magnitude.
        let magnitude = (a_digits - self.scale as i64) - (b_digits - other.scale as i64);

        // Choose a result scale that leaves `significant` digits after the
        // leading digit of the quotient.
        let scale = (significant - magnitude - 1).max(GUARD_DIGITS as i64) as u32;

        // quotient = a * 10^(sb - sa + scale) / b, computed exactly before rounding.
        let shift = other.scale as i64 + scale as i64 - self.scale as i64;
        let numerator = if shift >= 0 {
            &self.mantissa * pow10(shift as u32)
        } else {
            let divisor = pow10((-shift) as u32);
            let (q, r) = num_integer::Integer::div_rem(&self.mantissa, &divisor);
            if r.is_zero() {
                q
            } else {
                // Exact division is impossible; keep the extra shift as a
                // fractional tail so no significant digits are invented.
                &self.mantissa / &divisor
            }
        };
        let mantissa = round_div(&numerator, &other.mantissa, Rounding::HalfAwayFromZero);
        Ok(Real { mantissa, scale })
    }

    /// Division that stays exact when it can: an evenly dividing quotient keeps
    /// `scale = 0` and can be folded straight back into an integer.
    pub fn div_exact_or(&self, other: &Real, precision: u32) -> Result<Real, NumError> {
        if other.mantissa.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        if self.scale == 0 && other.scale == 0 {
            let (quotient, remainder) = self.mantissa.div_rem(&other.mantissa);
            if remainder.is_zero() {
                return Ok(Real {
                    mantissa: quotient,
                    scale: 0,
                });
            }
        }
        self.div(other, precision)
    }

    pub fn cmp_exact(&self, other: &Real) -> Ordering {
        let scale = self.scale.max(other.scale);
        let a = self.with_scale(scale, Rounding::HalfAwayFromZero);
        let b = other.with_scale(scale, Rounding::HalfAwayFromZero);
        a.mantissa.cmp(&b.mantissa)
    }

    /// Largest integer `<= self`.
    pub fn floor(&self) -> BigInt {
        if self.scale == 0 {
            return self.mantissa.clone();
        }
        floor_div(&self.mantissa, &pow10(self.scale))
    }

    /// The value truncated toward zero.
    pub fn truncate(&self) -> BigInt {
        if self.scale == 0 {
            return self.mantissa.clone();
        }
        &self.mantissa / pow10(self.scale)
    }

    /// Nearest `f64`, or infinity when the magnitude exceeds what `f64` can
    /// hold. Used only for display heuristics and the `tofloat` helper, never
    /// for the arithmetic itself.
    ///
    /// The value is `mantissa * 10^-scale`, so the conversion is a single
    /// multiply by a (possibly tiny or huge) power of ten. `BigInt::to_f64`
    /// already saturates to infinity for mantissas outside `f64` range.
    pub fn to_f64(&self) -> f64 {
        if self.mantissa.is_zero() {
            return 0.0;
        }
        let m = self.mantissa.to_f64().unwrap_or(f64::NAN);
        if !m.is_finite() {
            return m;
        }
        let exponent = -(self.scale as i64);
        if exponent < -320 {
            // Beyond f64's range; the sign is still meaningful.
            return if self.mantissa.is_negative() {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
        if exponent > 320 {
            return 0.0;
        }
        // Split the scaling in two so neither factor overflows on its own.
        let half = exponent / 2;
        m * 10f64.powi(half as i32) * 10f64.powi((exponent - half) as i32)
    }

    pub fn from_f64(value: f64) -> Option<Real> {
        if !value.is_finite() {
            return None;
        }
        // The shortest decimal that round-trips through f64.
        let text = format!("{value:e}");
        real_from_decimal_str(&text).ok()
    }
}

/// Parses a decimal literal such as `3.14159`, `1e-9`, or `2.5E10`.
pub fn real_from_decimal_str(text: &str) -> Result<Real, NumError> {
    let (mantissa_text, exponent) = match text.split_once(['e', 'E']) {
        Some((m, e)) => (
            m,
            e.parse::<i64>()
                .map_err(|_| NumError::Parse(format!("invalid exponent in `{text}`")))?,
        ),
        None => (text, 0),
    };

    let (int_part, frac_part) = match mantissa_text.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa_text, ""),
    };

    let digits = format!("{int_part}{frac_part}");
    let digits = if digits.is_empty() {
        "0".to_string()
    } else {
        digits
    };
    let mantissa = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| NumError::Parse(format!("invalid number `{text}`")))?;

    let scale = frac_part.len() as i64 - exponent;
    if scale < 0 {
        Ok(Real {
            mantissa: mantissa * pow10((-scale) as u32),
            scale: 0,
        })
    } else {
        Ok(Real {
            mantissa,
            scale: scale as u32,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// Round half away from zero - the convention most people expect.
    HalfAwayFromZero,
    /// Discard extra digits.
    Truncate,
    /// Round toward negative infinity.
    Floor,
}

fn round_div(numerator: &BigInt, denominator: &BigInt, rounding: Rounding) -> BigInt {
    if denominator.is_zero() {
        return BigInt::zero();
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder.is_zero() {
        return quotient;
    }
    match rounding {
        Rounding::Truncate => quotient,
        Rounding::Floor => {
            if numerator.is_negative() != denominator.is_negative() {
                quotient - 1
            } else {
                quotient
            }
        }
        Rounding::HalfAwayFromZero => {
            let doubled = (remainder.abs() << 1u32).cmp(&denominator.abs());
            if doubled == Ordering::Less {
                quotient
            } else if numerator.is_negative() {
                quotient - 1
            } else {
                quotient + 1
            }
        }
    }
}

fn floor_div(numerator: &BigInt, denominator: &BigInt) -> BigInt {
    round_div(numerator, denominator, Rounding::Floor)
}

pub fn pow10(exponent: u32) -> BigInt {
    BigInt::from(10u32).pow(exponent)
}

/// Number of decimal digits in `value`, ignoring any sign, computed from its
/// bit length.
///
/// Far cheaper than converting to a string and measuring the result, which
/// costs a full divide-and-conquer base conversion. Callers that only need the
/// count, never the digits, should prefer this.
pub fn decimal_digits(value: &BigInt) -> usize {
    if value.is_zero() {
        return 1;
    }
    let magnitude = value.magnitude();
    let bits = magnitude.bits();
    // Overestimate the digit count from the bit length, then correct downward.
    let ten = BigUint::from(10u32);
    let mut estimate = ((bits as f64) * std::f64::consts::LOG10_2) as usize + 1;
    let mut power = ten.pow(estimate as u32);
    while power <= *magnitude {
        estimate += 1;
        power *= &ten;
    }
    while estimate > 1 && power > magnitude * &ten {
        estimate -= 1;
        power /= &ten;
    }
    estimate
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumError {
    DivisionByZero,
    NegativeRoot { degree: String },
    Overflow(String),
    Domain(String),
    Parse(String),
    Undefined(String),
}

impl fmt::Display for NumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumError::DivisionByZero => f.write_str("division by zero"),
            NumError::NegativeRoot { degree } => write!(
                f,
                "even root (degree {degree}) of a negative number is not a real number"
            ),
            NumError::Overflow(m) => write!(f, "value too large: {m}"),
            NumError::Domain(m) => write!(f, "argument out of domain: {m}"),
            NumError::Parse(m) => write!(f, "{m}"),
            NumError::Undefined(m) => write!(f, "undefined: {m}"),
        }
    }
}

impl std::error::Error for NumError {}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<BigInt> for Real {
    fn from(value: BigInt) -> Real {
        Real::from_int(&value)
    }
}

impl From<&BigInt> for Real {
    fn from(value: &BigInt) -> Real {
        Real::from_int(value)
    }
}

impl From<Rational> for Real {
    fn from(value: Rational) -> Real {
        Real::from_int(value.numerator())
            .div_exact_or(&Real::from_int(value.denominator()), DEFAULT_PRECISION)
            .unwrap_or_else(|_| Real::zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationals_stay_exact() {
        let third = Rational::new(BigInt::from(1), BigInt::from(3)).unwrap();
        let two = Rational::from_int(BigInt::from(2));
        // 1/3 + 2 = 7/3, kept exact rather than rounded.
        assert_eq!(third.add(&two).to_string(), "7/3");
        // 1/3 * 3 collapses back to an integer.
        assert_eq!(
            third.mul(&Rational::from_int(BigInt::from(3))).to_string(),
            "1"
        );
        // 1/4 has an exact decimal expansion of 0.25.
        let quarter = Rational::new(BigInt::from(1), BigInt::from(4)).unwrap();
        assert_eq!(Real::from(quarter).to_f64(), 0.25);
    }

    #[test]
    fn perfect_roots_are_found() {
        let v = Rational::from_int(BigInt::from(1_000_000));
        let r = v.nth_root_exact(&BigInt::from(3)).unwrap();
        assert_eq!(r.to_string(), "100");
        assert!(Rational::from_int(BigInt::from(2))
            .nth_root_exact(&BigInt::from(2))
            .is_none());
    }

    #[test]
    fn decimal_parsing_round_trips() {
        let r = real_from_decimal_str("1.5e-3").unwrap();
        assert_eq!(r.mantissa(), &BigInt::from(15));
        assert_eq!(r.scale(), 4);
        assert_eq!(r.to_f64(), 0.0015);
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        let r = real_from_decimal_str("2.5").unwrap();
        assert_eq!(
            r.round_significant(1, Rounding::HalfAwayFromZero).to_f64(),
            3.0
        );
        let r = real_from_decimal_str("-2.5").unwrap();
        assert_eq!(
            r.round_significant(1, Rounding::HalfAwayFromZero).to_f64(),
            -3.0
        );
        // Rounding to more digits than the value carries is a no-op.
        let r = real_from_decimal_str("1.25").unwrap();
        assert_eq!(
            r.round_significant(10, Rounding::HalfAwayFromZero).to_f64(),
            1.25
        );
    }

    #[test]
    fn division_rounds_to_precision() {
        let one = Real::from_int(&BigInt::one());
        let three = Real::from_int(&BigInt::from(3));
        let third = one.div(&three, 16).unwrap();
        assert_eq!(third.to_f64(), 1.0 / 3.0);
        // 4/2 is exact, so it stays an integer while 1/3 cannot.
        assert!(Real::from_int(&BigInt::from(4))
            .div_exact_or(&Real::from_int(&BigInt::from(2)), 16)
            .unwrap()
            .is_exact_integer());
        assert!(!one.div_exact_or(&three, 16).unwrap().is_exact_integer());
    }
}

#[cfg(test)]
mod dd_equiv {
    use super::*;
    #[test]
    fn bit_estimate_matches_string_length() {
        // The bit-length estimate replaced a string conversion in the
        // formatter, so it must agree exactly, including at powers of ten and
        // for negative values where the sign must be ignored.
        // The bit-length estimate replaced a string conversion in the formatter,
        // so it must agree exactly, including at powers of ten.
        let mut cases: Vec<BigInt> = Vec::new();
        for k in 0u32..400 {
            cases.push(BigInt::from(10u32).pow(k));
            cases.push(BigInt::from(10u32).pow(k) - 1u32);
            cases.push(BigInt::from(10u32).pow(k) + 1u32);
        }
        for v in [0i64, 1, 9, 10, 99, 100, 999, 1000, -1, -999, -1000] {
            cases.push(BigInt::from(v));
        }
        for k in 1u32..300 {
            cases.push(BigInt::from(2u32).pow(k));
            cases.push(BigInt::from(3u32).pow(k));
        }
        for v in &cases {
            let expected = v.abs().to_str_radix(10).len();
            assert_eq!(decimal_digits(v), expected, "for {}", v);
        }
        assert!(cases.len() > 1200, "the sweep should be broad");
    }
}
