//! omega — an arbitrary-precision calculator engine.
//!
//! Integers are unbounded, so `2^1000000` is computed exactly. Division that
//! does not divide evenly produces an exact rational instead of silently
//! rounding, and irrational results are produced as high-precision reals whose
//! working precision is raised on demand.

pub mod cli;
pub mod eval;
pub mod format;
pub mod lexer;
pub mod number;
pub mod parser;
pub mod value;

pub use eval::{Engine, EvalError};
pub use number::{Int, Rational, Real};
pub use value::Value;
