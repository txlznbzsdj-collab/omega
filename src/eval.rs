//! Expression evaluation: built-in functions, constants, and variables.

use crate::format;
use crate::lexer;
use crate::number::{pow10, NumError, Real, Rounding};
use crate::parser::{self, BinOp, Expr};
use crate::value::Value;
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug)]
pub enum EvalError {
    Syntax(String),
    Math(NumError),
    Unknown(String),
    Usage(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Syntax(m) => write!(f, "syntax error: {m}"),
            EvalError::Math(m) => write!(f, "{m}"),
            EvalError::Unknown(m) => write!(f, "{m}"),
            EvalError::Usage(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<NumError> for EvalError {
    fn from(value: NumError) -> EvalError {
        EvalError::Math(value)
    }
}

impl From<parser::ParseError> for EvalError {
    fn from(value: parser::ParseError) -> EvalError {
        EvalError::Syntax(value.message)
    }
}

impl From<lexer::LexError> for EvalError {
    fn from(value: lexer::LexError) -> EvalError {
        EvalError::Syntax(value.message)
    }
}

/// The evaluation context: variable bindings kept between inputs.
pub struct Engine {
    vars: HashMap<String, Value>,
    precision: u32,
}

/// Working precision used for transcendental functions. Raised automatically
/// when a value carries more digits than this.
pub const FUNCTION_PRECISION: u32 = 80;

/// Extra digits computed beyond what is displayed, so the last shown digit is
/// correctly rounded rather than merely truncated.
pub const PRECISION_GUARD: u32 = 12;

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            vars: HashMap::new(),
            precision: FUNCTION_PRECISION,
        }
    }

    /// Sets how many digits of an irrational result must be correct.
    ///
    /// A few guard digits are added on top: the last computed digit of a
    /// series or a root is not trustworthy, and computing exactly `precision`
    /// digits would leave the final displayed digit off by one.
    pub fn with_precision(mut self, precision: u32) -> Engine {
        self.precision = precision.max(16).saturating_add(PRECISION_GUARD);
        self
    }

    pub fn precision(&self) -> u32 {
        self.precision
    }

    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.vars.insert(name.into(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    pub fn clear(&mut self) {
        self.vars.clear();
    }

    pub fn variables(&self) -> Vec<(&str, &Value)> {
        let mut items: Vec<(&str, &Value)> =
            self.vars.iter().map(|(k, v)| (k.as_str(), v)).collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        items
    }

    /// Parses and evaluates one input, returning the value and the name if the
    /// expression was an assignment.
    pub fn eval(&mut self, input: &str) -> Result<Value, EvalError> {
        let (tokens, _) = lexer::tokenize(input)?;
        let expr = parser::parse(tokens)?;
        self.eval_expr(&expr)
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Int { text, radix } => {
                let value = BigInt::parse_bytes(text.as_bytes(), *radix).ok_or_else(|| {
                    EvalError::Syntax(format!("`{text}` is not a valid base-{radix} number"))
                })?;
                Ok(Value::Int(value))
            }
            Expr::Float(text) => {
                let real = crate::number::real_from_decimal_str(text)?;
                Ok(Value::Real(real))
            }
            Expr::Ident(name) => self
                .lookup(name)
                .ok_or_else(|| EvalError::Unknown(format!("`{name}` is not defined"))),
            Expr::Pos(inner) => self.eval_expr(inner),
            Expr::Neg(inner) => Ok(self.eval_expr(inner)?.neg()),
            Expr::Assign { name, value } => {
                let result = self.eval_expr(value)?;
                self.set(name.clone(), result.clone());
                Ok(result)
            }
            Expr::Factorial(inner) => {
                let value = self.eval_expr(inner)?;
                factorial(&value)
            }
            Expr::Bin { op, lhs, rhs } => {
                let a = self.eval_expr(lhs)?;
                let b = self.eval_expr(rhs)?;
                apply(*op, &a, &b)
            }
            Expr::Call { name, args } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval_expr(arg)?);
                }
                call_builtin(name, &values, self.precision)
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.vars.get(name) {
            return Some(value.clone());
        }
        constant(name, self.precision)
    }
}

/// Built-in constants, computed on demand at the requested precision.
pub fn constant(name: &str, precision: u32) -> Option<Value> {
    let value = match name {
        "pi" | "PI" => Value::Real(real_pi(precision)),
        "e" | "E" => Value::Real(real_exp(&Real::from_int(&BigInt::one()), precision)),
        "tau" => {
            Value::Real(real_pi(precision).mul_to(&Real::from_int(&BigInt::from(2)), precision))
        }
        "phi" => {
            // (1 + sqrt(5)) / 2
            let five = Real::from_int(&BigInt::from(5));
            let root = real_sqrt(&five, precision);
            Value::Real(
                Real::from_int(&BigInt::one())
                    .add(&root)
                    .div(&Real::from_int(&BigInt::from(2)), precision)
                    .ok()?,
            )
        }
        _ => return None,
    };
    Some(value)
}

fn apply(op: BinOp, a: &Value, b: &Value) -> Result<Value, EvalError> {
    Ok(match op {
        BinOp::Add => a.add(b)?,
        BinOp::Sub => a.sub(b)?,
        BinOp::Mul => a.mul(b)?,
        BinOp::Div => a.div(b)?,
        BinOp::Rem => a.rem(b)?,
        BinOp::Pow => a.pow(b)?,
    })
}

fn factorial(value: &Value) -> Result<Value, EvalError> {
    let n = value.as_int().ok_or_else(|| {
        EvalError::Math(NumError::Domain(
            "factorial is only defined for non-negative integers".to_string(),
        ))
    })?;
    if n.is_negative() {
        return Err(EvalError::Math(NumError::Domain(format!(
            "factorial of a negative number ({}) is undefined",
            format::render_plain(value)
        ))));
    }
    // Guard against a request that would exhaust memory: 10^8! has far more
    // digits than can be stored, so refuse before allocating.
    if let Some(small) = n.to_u64() {
        if small > 20_000_000 {
            return Err(EvalError::Math(NumError::Overflow(format!(
                "{small}! is too large to compute exactly"
            ))));
        }
    } else {
        return Err(EvalError::Math(NumError::Overflow(
            "factorial argument is too large to compute exactly".to_string(),
        )));
    }

    // Binary splitting: multiplying 2..n sequentially keeps the accumulator
    // huge for most of the loop, so every step is a large-by-small multiply.
    // Splitting the range in half and multiplying the two partial products
    // together balances the operand sizes, which is dramatically faster for
    // large n.
    Ok(Value::Int(product_range(BigInt::from(2), n.clone())))
}

/// Product of every integer in `low..=high`, by recursive halving.
///
/// Returns 1 for an empty range. The recursion splits until the ranges are
/// small enough that a plain loop is cheapest, then combines balanced products.
fn product_range(low: BigInt, high: BigInt) -> BigInt {
    // A small range is not worth splitting; multiplying a handful of machine
    // words is cheaper than the recursion.
    if low > high {
        return BigInt::one();
    }
    if &high - &low <= BigInt::from(8) {
        let mut result = BigInt::one();
        let mut k = low;
        while k <= high {
            result *= &k;
            k += 1;
        }
        return result;
    }
    let middle = (&low + &high) >> 1u32;
    let left = product_range(low, middle.clone());
    let right = product_range(middle + 1, high);
    left * right
}

// ---------------------------------------------------------------------------
// Built-in functions
// ---------------------------------------------------------------------------

fn arity_err(name: &str, expected: &str, got: usize) -> EvalError {
    EvalError::Usage(format!(
        "`{name}` takes {expected} argument(s), but {got} were given"
    ))
}

fn exact_digits(value: &Value, base: u32) -> u32 {
    match value {
        Value::Real(v) => v.scale().max(base),
        Value::Int(v) => {
            // Keep extra precision when the integer itself is huge.
            let digits = format::digit_count(v) as u32;
            digits.max(base)
        }
        _ => base,
    }
}

fn call_builtin(name: &str, args: &[Value], base: u32) -> Result<Value, EvalError> {
    match name {
        "abs" => {
            expect(name, args, 1)?;
            Ok(args[0].abs())
        }
        "neg" => {
            expect(name, args, 1)?;
            Ok(args[0].neg())
        }
        "sign" => {
            expect(name, args, 1)?;
            Ok(Value::Int(BigInt::from(if args[0].is_zero() {
                0
            } else if args[0].is_negative() {
                -1
            } else {
                1
            })))
        }
        "floor" => {
            expect(name, args, 1)?;
            Ok(Value::Int(real_of(&args[0], base).floor()))
        }
        "ceil" => {
            expect(name, args, 1)?;
            let r = real_of(&args[0], base);
            let f = r.floor();
            Ok(Value::Int(if r.is_exact_integer() { f } else { f + 1 }))
        }
        "trunc" => {
            expect(name, args, 1)?;
            Ok(Value::Int(real_of(&args[0], base).truncate()))
        }
        "round" => {
            expect(name, args, 1)?;
            let r = real_of(&args[0], base);
            // Round half away from zero: sign(x) * floor(|x| + 1/2).
            let half = Real::from_scaled(BigInt::from(5), 1);
            let magnitude = r.abs().add(&half).floor();
            let result = if r.is_negative() {
                -magnitude
            } else {
                magnitude
            };
            Ok(Value::Int(result))
        }
        "frac" => {
            expect(name, args, 1)?;
            let r = real_of(&args[0], base);
            Ok(Value::Real(r.sub(&Real::from_int(&r.truncate()))))
        }
        "sqrt" => {
            expect(name, args, 1)?;
            root(&args[0], 2, base)
        }
        "cbrt" => {
            expect(name, args, 1)?;
            root(&args[0], 3, base)
        }
        "nthroot" => {
            expect(name, args, 2)?;
            let degree = args[1].to_u32().ok_or_else(|| {
                EvalError::Usage("the root degree must be a positive integer".into())
            })?;
            if degree == 0 {
                return Err(EvalError::Math(NumError::Domain(
                    "the 0th root is undefined".to_string(),
                )));
            }
            root(&args[0], degree, base)
        }
        "ln" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let value = real_of(&args[0], base);
            Ok(Value::Real(real_ln(&value, precision)?))
        }
        "log10" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let value = real_of(&args[0], base);
            let ln = real_ln(&value, precision + 5)?;
            let ten = real_ln(&Real::from_int(&BigInt::from(10)), precision + 5)?;
            Ok(Value::Real(ln.div(&ten, precision)?))
        }
        "log" => match args.len() {
            1 => call_builtin("log10", args, base),
            2 => {
                let precision = exact_digits(&args[0], base);
                let ln_x = real_ln(&real_of(&args[0], base), precision + 5)?;
                let ln_b = real_ln(&real_of(&args[1], base), precision + 5)?;
                Ok(Value::Real(ln_x.div(&ln_b, precision)?))
            }
            n => Err(arity_err(name, "1 or 2", n)),
        },
        "exp" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            Ok(Value::Real(real_exp(&real_of(&args[0], base), precision)))
        }
        "sin" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            Ok(Value::Real(real_sin(&real_of(&args[0], base), precision)?))
        }
        "cos" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            Ok(Value::Real(real_cos(&real_of(&args[0], base), precision)?))
        }
        "tan" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let value = real_of(&args[0], base);
            let sin = real_sin(&value, precision + 5)?;
            let cos = real_cos(&value, precision + 5)?;
            if cos.is_zero() {
                return Err(EvalError::Math(NumError::Undefined(
                    "tan is undefined where cos is zero".to_string(),
                )));
            }
            Ok(Value::Real(sin.div(&cos, precision)?))
        }
        "atan" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            Ok(Value::Real(real_atan(&real_of(&args[0], base), precision)))
        }
        "asin" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let x = real_of(&args[0], base);
            let one = Real::from_int(&BigInt::one());
            if x.cmp_exact(&one) == std::cmp::Ordering::Greater
                || x.cmp_exact(&one.neg()) == std::cmp::Ordering::Less
            {
                return Err(EvalError::Math(NumError::Domain(
                    "asin is only defined on -1..=1".to_string(),
                )));
            }
            // asin(x) = atan(x / sqrt(1 - x^2))
            let denominator = real_sqrt(&one.sub(&x.mul_to(&x, precision + 5)), precision + 5);
            if denominator.is_zero() {
                let pi = real_pi(precision);
                return Ok(Value::Real(if x.is_negative() {
                    pi.div(&Real::from_int(&BigInt::from(-2)), precision)?
                } else {
                    pi.div(&Real::from_int(&BigInt::from(2)), precision)?
                }));
            }
            Ok(Value::Real(real_atan(
                &x.div(&denominator, precision + 5)?,
                precision,
            )))
        }
        "acos" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let asin = call_builtin("asin", args, base)?;
            let pi = real_pi(precision);
            let half_pi = pi.div(&Real::from_int(&BigInt::from(2)), precision)?;
            Ok(Value::Real(
                half_pi
                    .sub(&real_of(&asin, base))
                    .round_significant(precision as usize, Rounding::HalfAwayFromZero),
            ))
        }
        "sinh" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let x = real_of(&args[0], base);
            let ex = real_exp(&x, precision + 5);
            let enx = real_exp(&x.neg(), precision + 5);
            Ok(Value::Real(
                ex.sub(&enx)
                    .div(&Real::from_int(&BigInt::from(2)), precision)?,
            ))
        }
        "cosh" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            let x = real_of(&args[0], base);
            let ex = real_exp(&x, precision + 5);
            let enx = real_exp(&x.neg(), precision + 5);
            Ok(Value::Real(
                ex.add(&enx)
                    .div(&Real::from_int(&BigInt::from(2)), precision)?,
            ))
        }
        "gcd" => {
            expect(name, args, 2)?;
            let a = args[0]
                .as_int()
                .ok_or_else(|| EvalError::Usage("gcd requires integers".into()))?;
            let b = args[1]
                .as_int()
                .ok_or_else(|| EvalError::Usage("gcd requires integers".into()))?;
            Ok(Value::Int(num_integer::Integer::gcd(a, b)))
        }
        "lcm" => {
            expect(name, args, 2)?;
            let a = args[0]
                .as_int()
                .ok_or_else(|| EvalError::Usage("lcm requires integers".into()))?;
            let b = args[1]
                .as_int()
                .ok_or_else(|| EvalError::Usage("lcm requires integers".into()))?;
            Ok(Value::Int(num_integer::Integer::lcm(a, b)))
        }
        "min" | "max" => {
            expect(name, args, 2)?;
            let ordering = args[0].cmp_value(&args[1]);
            let take_first = if name == "min" {
                ordering != std::cmp::Ordering::Greater
            } else {
                ordering != std::cmp::Ordering::Less
            };
            Ok(if take_first {
                args[0].clone()
            } else {
                args[1].clone()
            })
        }
        "pow" => {
            expect(name, args, 2)?;
            Ok(args[0].pow(&args[1])?)
        }
        "float" => {
            expect(name, args, 1)?;
            let precision = exact_digits(&args[0], base);
            Ok(Value::Real(real_of(&args[0], base).round_significant(
                precision as usize,
                Rounding::HalfAwayFromZero,
            )))
        }
        "digits" => {
            expect(name, args, 1)?;
            let count = match &args[0] {
                Value::Int(v) => format::digit_count(v),
                Value::Rational(v) => format::digit_count(v.numerator()),
                Value::Real(v) => format::digit_count(v.mantissa()),
            };
            Ok(Value::Int(BigInt::from(count)))
        }
        // `size(b, e)` reports how many digits `b^e` would have, without
        // computing it. This is what makes a number like 9999^9999999999
        // answerable at all: the value cannot be held in memory, but its size
        // is known instantly.
        "size" | "numdigits" => {
            expect(name, args, 2)?;
            let digits = args[0].result_digits(
                args[1]
                    .as_int()
                    .ok_or_else(|| EvalError::Usage("the exponent must be an integer".into()))?,
            );
            match digits {
                Some(digits) => Ok(Value::Int(BigInt::from(digits))),
                None => Err(EvalError::Usage(
                    "the result size cannot be estimated for this base".into(),
                )),
            }
        }
        "isprime" => {
            expect(name, args, 1)?;
            let n = args[0]
                .as_int()
                .ok_or_else(|| EvalError::Usage("isprime requires an integer".into()))?;
            Ok(Value::Int(BigInt::from(if is_prime(n) { 1 } else { 0 })))
        }
        _ => Err(EvalError::Unknown(format!("unknown function `{name}`"))),
    }
}

fn expect(name: &str, args: &[Value], count: usize) -> Result<(), EvalError> {
    if args.len() == count {
        Ok(())
    } else {
        Err(arity_err(name, &count.to_string(), args.len()))
    }
}

fn real_of(value: &Value, base: u32) -> Real {
    value.to_real(exact_digits(value, base))
}

/// Integer `n`-th root. Returns an exact integer when the input is a perfect
/// power, and a high-precision real otherwise.
fn root(value: &Value, degree: u32, base: u32) -> Result<Value, EvalError> {
    let negative = value.is_negative();
    if negative && degree % 2 == 0 {
        return Err(EvalError::Math(NumError::NegativeRoot {
            degree: degree.to_string(),
        }));
    }
    if value.is_zero() {
        return Ok(Value::zero());
    }

    let rational = value.to_rational();
    if let Some(exact) = rational.nth_root_exact(&BigInt::from(degree)) {
        return Ok(Value::Rational(exact).simplify());
    }

    let precision = exact_digits(value, base);
    let real = real_of(value, base);
    let magnitude = real.abs();
    if degree == 2 {
        let result = real_sqrt(&magnitude, precision);
        return Ok(Value::Real(if negative { result.neg() } else { result }));
    }
    let result = real_nth_root(&magnitude, degree, precision)?;
    Ok(Value::Real(if negative { result.neg() } else { result }))
}

// ---------------------------------------------------------------------------
// Real-valued primitives
// ---------------------------------------------------------------------------

/// Newton's method for a square root, seeded from an exact integer root.
pub fn real_sqrt(value: &Real, precision: u32) -> Real {
    if value.is_zero() {
        return Real::zero();
    }
    let work = precision + 10;
    // Compute sqrt(value * 10^work) exactly-ish as an integer, which places the
    // root at 10^(work/2) decimal places of the true result. Scaling the value
    // itself (rather than its representation) is what makes the integer root
    // large enough to carry `work / 2` digits.
    let scaled = value.mul(&Real::from_int(&pow10(work)));
    let seed = integer_sqrt(&scaled.truncate());
    let unit = Real::from_int(&pow10(work / 2));
    let mut guess = Real::from_int(&seed)
        .div(&unit, work + 5)
        .unwrap_or_else(|_| Real::from_int(&BigInt::one()));
    let two = Real::from_int(&BigInt::from(2));
    for _ in 0..64 {
        if guess.is_zero() {
            break;
        }
        let quotient = match value.div(&guess, work + 5) {
            Ok(q) => q,
            Err(_) => break,
        };
        let next = guess
            .add(&quotient)
            .div(&two, work + 5)
            .unwrap_or_else(|_| guess.clone());
        let converged = next.cmp_exact(&guess) == std::cmp::Ordering::Equal;
        guess = next;
        if converged {
            break;
        }
    }
    guess.round_significant(precision as usize, Rounding::HalfAwayFromZero)
}

/// `floor(sqrt(value))` for a non-negative integer.
///
/// The seed is a power of two at least as large as the true root, and the
/// iteration is the standard integer Newton step, which decreases strictly
/// until it reaches the floor.
fn integer_sqrt(value: &BigInt) -> BigInt {
    if value <= &BigInt::zero() {
        return BigInt::zero();
    }
    if value.is_one() {
        return BigInt::one();
    }
    // 2^ceil(bits / 2) is >= sqrt(value) because value < 2^bits.
    let bits = value.bits();
    let mut x = BigInt::one() << bits.div_ceil(2);
    loop {
        // next = floor((x + value / x) / 2)
        let quotient = value / &x;
        let next = (&x + quotient) >> 1u32;
        if next >= x {
            break;
        }
        x = next;
    }
    // Guard against the seed landing one below the floor.
    while (&x + 1u32) * (&x + 1u32) <= *value {
        x += 1u32;
    }
    while &x * &x > *value {
        x -= 1u32;
    }
    x
}

fn real_nth_root(value: &Real, degree: u32, precision: u32) -> Result<Real, NumError> {
    let work = precision + 10;
    let n = Real::from_int(&BigInt::from(degree));
    let n_minus_one = Real::from_int(&BigInt::from(degree - 1));
    // Seed with an exponential estimate so Newton converges quickly.
    let mut guess = real_exp(&real_ln(value, work + 5)?.div(&n, work + 5)?, work);
    if guess.is_zero() {
        guess = Real::from_int(&BigInt::one());
    }
    for _ in 0..(work as f64).log2().ceil() as usize + 8 {
        let powered = guess.pow_u32(degree - 1, work + 5);
        let denominator = n.mul_to(&powered, work + 5);
        if denominator.is_zero() {
            break;
        }
        // x_{k+1} = ((n-1) x_k + a / x_k^(n-1)) / n
        let term = value.div(&powered, work + 5)?;
        let next = n_minus_one
            .mul_to(&guess, work + 5)
            .add(&term)
            .div(&n, work + 5)?;
        if next.cmp_exact(&guess) == std::cmp::Ordering::Equal {
            guess = next;
            break;
        }
        guess = next;
    }
    Ok(guess.round_significant(precision as usize, Rounding::HalfAwayFromZero))
}

/// Pi via Machin's formula: pi/4 = 4*atan(1/5) - atan(1/239).
pub fn real_pi(precision: u32) -> Real {
    let work = precision + 10;
    let four = Real::from_int(&BigInt::from(4));
    // atan(1/5) is the expensive series; atan(1/239) converges far faster.
    let a = arctan_reciprocal(5, work).mul_to(&four, work);
    let b = arctan_reciprocal(239, work);
    // pi = 4 * (4*atan(1/5) - atan(1/239)). Multiplying by four is exact, so
    // there is no need to form pi/4 and divide it back out again.
    a.sub(&b)
        .mul_to(&four, work)
        .round_significant(precision as usize, Rounding::HalfAwayFromZero)
}

/// `atan(1/x)` for an integer `x > 1`, summed as an alternating series.
fn arctan_reciprocal(x: i64, precision: u32) -> Real {
    let work = precision + 10;
    let x_squared = Real::from_int(&BigInt::from(x * x));
    let mut term = Real::from_int(&BigInt::one())
        .div(&Real::from_int(&BigInt::from(x)), work)
        .unwrap();
    let mut sum = term.clone();
    let mut k: i64 = 1;
    loop {
        term = term.div(&x_squared, work).unwrap_or_else(|_| Real::zero());
        let divided = match term.div(&Real::from_int(&BigInt::from(2 * k + 1)), work) {
            Ok(v) => v,
            Err(_) => break,
        };
        if divided.is_zero() {
            break;
        }
        if k % 2 == 1 {
            sum = sum.sub(&divided);
        } else {
            sum = sum.add(&divided);
        }
        k += 1;
        // Stop once the term is below the working precision.
        if divided.abs().is_below_epsilon(work + 5) {
            break;
        }
        if k > 100_000 {
            break;
        }
    }
    sum
}

/// Natural logarithm via argument reduction and the atanh series.
pub fn real_ln(value: &Real, precision: u32) -> Result<Real, NumError> {
    if !value.is_positive() {
        return Err(NumError::Domain(format!(
            "ln is only defined for positive numbers (got {})",
            format::render_real(
                value,
                format::Style {
                    digits: 12,
                    ..format::Style::default()
                },
            )
        )));
    }
    let work = precision + 10;
    if value.cmp_exact(&Real::from_int(&BigInt::one())) == std::cmp::Ordering::Equal {
        return Ok(Real::zero());
    }

    // Reduce to [1, 2) by extracting a power of ten, then use ln(x) = 2*atanh((x-1)/(x+1)).
    let one = Real::from_int(&BigInt::one());
    let ten = Real::from_int(&BigInt::from(10));

    let mut x = value.clone();
    let mut exponent: i64 = 0;
    while x.cmp_exact(&ten) != std::cmp::Ordering::Less {
        x = x.div(&ten, work + 5)?;
        exponent += 1;
    }
    while x.cmp_exact(&one) == std::cmp::Ordering::Less {
        x = x.mul_to(&ten, work + 5);
        exponent -= 1;
    }

    // Further reduce toward 1 using sqrt so the series converges quickly.
    let mut corrections = 0u32;
    while x.cmp_exact(&Real::from_scaled(pow10(work) + pow10(work) / 10, work))
        == std::cmp::Ordering::Greater
    {
        x = real_sqrt(&x, work + 5);
        corrections += 1;
        if corrections > 8 {
            break;
        }
    }

    let numerator = x.sub(&one);
    let denominator = x.add(&one);
    let z = numerator.div(&denominator, work + 5)?;
    let z_squared = z.mul_to(&z, work + 5);

    let mut term = z.clone();
    let mut sum = z.clone();
    let mut k: u64 = 1;
    loop {
        term = term.mul_to(&z_squared, work + 5);
        let divided = match term.div(&Real::from_int(&BigInt::from(2 * k + 1)), work + 5) {
            Ok(v) => v,
            Err(_) => break,
        };
        sum = sum.add(&divided);
        // The series terms shrink geometrically; stop when nothing changes.
        if divided.is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    sum = sum.mul_to(&Real::from_int(&BigInt::from(2)), work + 5);
    for _ in 0..corrections {
        sum = sum.mul_to(&Real::from_int(&BigInt::from(2)), work + 5);
    }

    // Add back ln(10) for each extracted power of ten.
    if exponent != 0 {
        let ln10 = ln10(work + 5)?;
        let scaled = ln10.mul_to(&Real::from_int(&BigInt::from(exponent)), work + 5);
        sum = sum.add(&scaled);
    }
    Ok(sum.round_significant(precision as usize, Rounding::HalfAwayFromZero))
}

/// caches ln(10) implicitly by recomputing with the atanh series
fn ln10(precision: u32) -> Result<Real, NumError> {
    // ln(10) = ln(2) + ln(5); computed directly keeps the reduction loop from
    // recursing back into itself.
    let work = precision + 10;
    let ten = Real::from_int(&BigInt::from(10));
    let one = Real::from_int(&BigInt::one());
    // Reduce 10 by repeated square roots (10^(1/2^k)) until close to 1.
    let mut x = ten;
    let mut doublings = 0u32;
    let threshold = Real::from_scaled(pow10(work) + pow10(work) / 5, work);
    while x.cmp_exact(&threshold) == std::cmp::Ordering::Greater {
        x = real_sqrt(&x, work + 5);
        doublings += 1;
        if doublings > 12 {
            break;
        }
    }
    let z = x.sub(&one).div(&x.add(&one), work + 5)?;
    let z_squared = z.mul_to(&z, work + 5);
    let mut term = z.clone();
    let mut sum = z.clone();
    let mut k: u64 = 1;
    loop {
        term = term.mul_to(&z_squared, work + 5);
        let divided = term.div(&Real::from_int(&BigInt::from(2 * k + 1)), work + 5)?;
        sum = sum.add(&divided);
        if divided.is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    sum = sum.mul_to(&Real::from_int(&BigInt::from(2)), work + 5);
    for _ in 0..doublings {
        sum = sum.mul_to(&Real::from_int(&BigInt::from(2)), work + 5);
    }
    Ok(sum)
}

/// Exponential via range reduction and the Taylor series.
pub fn real_exp(value: &Real, precision: u32) -> Real {
    if value.is_zero() {
        return Real::from_int(&BigInt::one());
    }
    let work = precision + 10;
    // exp(x) = exp(x / 2^k)^(2^k), with x/2^k small so the series converges fast.
    let mut doublings = 0u32;
    let mut x = value.clone();
    let one = Real::from_int(&BigInt::one());
    while x.abs().cmp_exact(&one) == std::cmp::Ordering::Greater {
        x = x
            .div(&Real::from_int(&BigInt::from(2)), work + 5)
            .unwrap_or_else(|_| Real::zero());
        doublings += 1;
        if doublings > 4096 {
            break;
        }
    }

    let mut term = one.clone();
    let mut sum = one.clone();
    let mut k: u64 = 1;
    loop {
        term = term
            .mul_to(&x, work + 5)
            .div(&Real::from_int(&BigInt::from(k)), work + 5)
            .unwrap_or_else(|_| Real::zero());
        if term.is_zero() {
            break;
        }
        sum = sum.add(&term);
        if term.abs().is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    for _ in 0..doublings {
        sum = sum.mul_to(&sum.clone(), work + 5);
    }
    sum.round_significant(precision as usize, Rounding::HalfAwayFromZero)
}

/// Reduces `x` modulo 2*pi so that trig series converge quickly even for huge
/// arguments. The reduction is exact in the sense that it uses pi to the
/// working precision plus guard digits.
fn reduce_angle(value: &Real, precision: u32) -> Real {
    let work = precision + 20;
    let pi = real_pi(work);
    let two_pi = pi.mul_to(&Real::from_int(&BigInt::from(2)), work);
    let quotient = match value.div(&two_pi, work) {
        Ok(q) => q,
        Err(_) => return value.clone(),
    };
    let periods = Real::from_int(&quotient.truncate());
    value.sub(&two_pi.mul_to(&periods, work))
}

pub fn real_sin(value: &Real, precision: u32) -> Result<Real, NumError> {
    let work = precision + 15;
    let x = reduce_angle(value, work);
    let x_squared = x.mul_to(&x, work + 5);
    let mut term = x.clone();
    let mut sum = x.clone();
    let mut k: u64 = 1;
    loop {
        // term_{k} = -term_{k-1} * x^2 / ((2k)(2k+1))
        term = term.mul_to(&x_squared, work + 5).div(
            &Real::from_int(&BigInt::from((2 * k) * (2 * k + 1))),
            work + 5,
        )?;
        if term.is_zero() {
            break;
        }
        if k % 2 == 1 {
            sum = sum.sub(&term);
        } else {
            sum = sum.add(&term);
        }
        if term.abs().is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    Ok(sum.round_significant(precision as usize, Rounding::HalfAwayFromZero))
}

pub fn real_cos(value: &Real, precision: u32) -> Result<Real, NumError> {
    let work = precision + 15;
    let x = reduce_angle(value, work);
    let x_squared = x.mul_to(&x, work + 5);
    let one = Real::from_int(&BigInt::one());
    let mut term = one.clone();
    let mut sum = one.clone();
    let mut k: u64 = 1;
    loop {
        term = term.mul_to(&x_squared, work + 5).div(
            &Real::from_int(&BigInt::from((2 * k - 1) * (2 * k))),
            work + 5,
        )?;
        if term.is_zero() {
            break;
        }
        if k % 2 == 1 {
            sum = sum.sub(&term);
        } else {
            sum = sum.add(&term);
        }
        if term.abs().is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    Ok(sum.round_significant(precision as usize, Rounding::HalfAwayFromZero))
}

/// arctangent via argument reduction: atan(x) = 2*atan(x / (1 + sqrt(1+x^2))).
///
/// Each application of the half-angle identity halves the result and shrinks
/// the argument, so the Taylor series is only ever summed for a small `|x|`.
/// Reducing all the way to `|x| <= 1/16` keeps the number of series terms
/// proportional to the requested precision; stopping at `|x| <= 1` would need
/// on the order of `10^precision` terms and silently return a wrong answer.
pub fn real_atan(value: &Real, precision: u32) -> Real {
    let work = precision + 15;
    let one = Real::from_int(&BigInt::one());
    let threshold = Real::from_scaled(BigInt::from(625), 4); // 1/16
    let mut x = value.clone();
    let mut halvings = 0u32;

    while x.abs().cmp_exact(&threshold) == std::cmp::Ordering::Greater {
        let root = real_sqrt(&one.add(&x.mul_to(&x, work + 5)), work + 5);
        x = x
            .div(&one.add(&root), work + 5)
            .unwrap_or_else(|_| Real::zero());
        halvings += 1;
        if x.is_zero() || halvings > 4096 {
            break;
        }
    }

    // Taylor series for atan, valid and fast for the small |x| reached above.
    let x_squared = x.mul_to(&x, work + 5);
    let mut term = x.clone();
    let mut sum = x.clone();
    let mut k: u64 = 1;
    loop {
        term = term.mul_to(&x_squared, work + 5);
        let divided = match term.div(&Real::from_int(&BigInt::from(2 * k + 1)), work + 5) {
            Ok(v) => v,
            Err(_) => break,
        };
        if divided.is_zero() {
            break;
        }
        if k % 2 == 1 {
            sum = sum.sub(&divided);
        } else {
            sum = sum.add(&divided);
        }
        if divided.abs().is_below_epsilon(work + 5) {
            break;
        }
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    for _ in 0..halvings {
        sum = sum.mul_to(&Real::from_int(&BigInt::from(2)), work + 5);
    }
    sum.round_significant(precision as usize, Rounding::HalfAwayFromZero)
}

/// `x^y` for real `x > 0`: exp(y * ln(x)).
pub fn real_pow(base: &Real, exponent: &Real, precision: u32) -> Result<Real, NumError> {
    if exponent.is_zero() {
        return Ok(Real::from_int(&BigInt::one()));
    }
    let work = precision + 10;
    if base.cmp_exact(&Real::from_int(&BigInt::one())) == std::cmp::Ordering::Equal {
        return Ok(Real::from_int(&BigInt::one()));
    }
    let ln = real_ln(base, work + 5)?;
    Ok(real_exp(&ln.mul_to(exponent, work + 5), precision))
}

// ---------------------------------------------------------------------------
// Primality
// ---------------------------------------------------------------------------

/// Deterministic Miller-Rabin for values that fit in `u64`; larger inputs use
/// the first several prime bases, which is not a proof but is decisive in
/// practice. Even numbers and small cases are handled directly.
pub fn is_prime(n: &BigInt) -> bool {
    if n.is_negative() {
        return false;
    }
    if let Some(small) = n.to_u64() {
        return is_prime_u64(small);
    }
    if num_integer::Integer::is_even(n) {
        return false;
    }
    let bases = [
        BigInt::from(2),
        BigInt::from(3),
        BigInt::from(5),
        BigInt::from(7),
        BigInt::from(11),
        BigInt::from(13),
        BigInt::from(17),
        BigInt::from(19),
        BigInt::from(23),
        BigInt::from(29),
        BigInt::from(31),
        BigInt::from(37),
    ];
    bases.iter().all(|base| miller_rabin(n, base))
}

fn is_prime_u64(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for p in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n % p == 0 {
            return n == p;
        }
    }
    // Deterministic set for all u64.
    let bases = [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    let n_minus_one = n - 1;
    let mut d = n_minus_one;
    let mut r = 0u32;
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }
    'outer: for base in bases {
        let mut x = mod_pow_u64(base % n, d, n);
        if x == 1 || x == n_minus_one {
            continue;
        }
        for _ in 0..r - 1 {
            x = mul_mod_u64(x, x, n);
            if x == n_minus_one {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

fn mod_pow_u64(mut base: u64, mut exponent: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    base %= modulus;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = mul_mod_u64(result, base, modulus);
        }
        base = mul_mod_u64(base, base, modulus);
        exponent >>= 1;
    }
    result
}

/// Multiplication modulo `m` without overflowing `u64`.
fn mul_mod_u64(a: u64, b: u64, m: u64) -> u64 {
    (a as u128 * b as u128 % m as u128) as u64
}

fn miller_rabin(n: &BigInt, base: &BigInt) -> bool {
    let one = BigInt::one();
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut r = 0u32;
    while num_integer::Integer::is_even(&d) {
        d /= 2;
        r += 1;
    }
    let mut x = base.modpow(&d, n);
    if x == one || x == n_minus_one {
        return true;
    }
    for _ in 0..r.saturating_sub(1) {
        x = x.modpow(&BigInt::from(2), n);
        if x == n_minus_one {
            return true;
        }
    }
    false
}
