# omega

An arbitrary-precision calculator written in Rust. Any number of digits, any
size of integer, and no silent rounding.

```
$ omega "2^128"
340282366920938463463374607431768211456

$ omega "1/3 + 1/6"
1/2

$ omega "0.1 + 0.2"
0.3

$ omega -d 50 "sqrt(2)"
1.414213562373095048801688724209698078569671875377
```

## Why it stays exact

Most calculators round the moment you divide. omega does not. Every value lives
in a small tower of representations, and it only moves up the tower when a
question genuinely has no exact answer:

| Kind | Used for | Example |
| --- | --- | --- |
| Integer | Whole numbers of any size | `2^1000000`, `1000!` |
| Rational | Exact fractions | `1/3`, `10.5 / 3` → `7/2` |
| Real | Irrational results, at a chosen precision | `sqrt(2)`, `pi`, `ln(10)` |

The practical consequences:

* `2^1000000` is computed exactly — all 301,030 digits.
* `1/3` is `1/3`, not `0.3333`.
* `0.1 + 0.2` is exactly `0.3`, because decimal input is exact rather than a
  binary approximation.
* `sqrt(16)` is `4`, not `4.000000000000001`. A perfect power is recognised as
  exact and converted back to an integer.

A rounded real is never passed off as exact: `sqrt(3)*sqrt(3)` reports
`3.0000000000000000000000000000000`, not `3`, because the value came from
rounding and promoting it to an integer would overstate what was computed.

## Install

Requires a Rust toolchain (1.75 or newer).

```
cargo build --release
```

Two programs are produced:

| Binary | What it is |
| --- | --- |
| `target/release/omega` | The command-line calculator |
| `target/release/omega-gui` | The same engine in a native window (Windows) |

Neither has a runtime dependency beyond the system itself.

## The window

`omega-gui` opens a window with an input box and a scrollable result area.
Type an expression and the answer appears as you type; Enter re-evaluates
explicitly.

It is built directly on the Win32 API rather than on a GUI toolkit: it links
`user32` and `gdi32` and nothing else, and the whole program is **358 KB**,
against roughly 8-15 MB for a typical Rust GUI stack. It needs about 3 MB of
private memory.

Correctness is checked without an interactive desktop by `bench/gui-path-check.rs`,
which reproduces the exact logic the window runs between its Win32 calls — the
same engine call, the same render, the same error branch:

```
cargo run --release --bin gui-path-check
```

## Usage

```
omega                     start an interactive session
omega "<expression>"      evaluate one expression
echo "<expr>" | omega     read expressions from standard input
```

Options:

| Flag | Meaning |
| --- | --- |
| `-d <n>`, `--digits <n>` | Correct digits for an inexact result (also sets the display width) |
| `-v`, `--vars` | List variable bindings after evaluating |
| `-e`, `--eval` | Treat the next argument as the expression |
| `-h`, `--help` | Usage |
| `-V`, `--version` | Version |

## No digit is ever withheld

An exact result is printed in full, however long it is. `omega "1000!"` writes
out all 2568 digits; `omega "2^100000"` writes out all 30103. Nothing is
truncated, elided or replaced with a count.

The one exception is forced by mathematics rather than by choice: a value with
no exact answer, such as `sqrt(2)` or `pi`, has no finite decimal expansion to
print. Those are shown to 32 significant digits by default, and `-d <n>` raises
that to as many as you ask for. When a displayed value is shorter than what was
computed, omega says so:

```
$ omega "pi"
3.1415926535897932384626433832795

$ omega -d 100 "pi"
3.141592653589793238462643383279502884197169399375105820974944592307816406286208998628034825342117068
```

## Digit grouping

Numbers are printed as bare digits. Nothing is inserted into the output unless
you ask for it, so a result is always safe to pipe or paste elsewhere:

```
$ omega "2^64"
18446744073709551616
```

When you want separators for readability, `-g` turns them on:

```
$ omega -g "2^64"
18_446_744_073_709_551_616
```

The separator is **only a display aid**. It never becomes part of the value, and
it is ignored if you type or paste it back in, so input and output stay
interchangeable:

```
$ omega "18_446_744_073_709_551_616 - 2^64"
0
```

| Flag | Effect |
| --- | --- |
| *(none)* | Bare digits — the default |
| `-g` | Underscore every three digits from four up |
| `-g ,` | `18,446,744,073,709,551,616` |
| `-g space` | `18 446 744 073 709 551 616` |
| `-g none` | Bare digits |
| `--group-from <n>` | Only group numbers with at least `n` integer digits |

```
$ omega -g --group-from 7 "1000000"
1_000_000

$ omega -g --group-from 7 "100000"
100000
```

With `-g`, grouping starts at four digits, so `1000` becomes `1_000` while
`999` is left alone. Grouping never touches the fractional part of a decimal,
so `1234.5678901` becomes `1_234.5678901` and not `1_234.567_890_1`. A fraction
is left alone entirely, since grouping `1/1000000` would blur the numerator and
denominator together.

## Timing

Every result is followed by how long it took:

```
$ omega "2^64"
18446744073709551616
  time  110.000 us

$ omega "2^1000000"
9900656229295898250697923616301903250733...
  time  184.651 ms
```

The figure is **measured**, not estimated — real elapsed time taken with a
monotonic clock. The unit is chosen from the magnitude so it always carries
significant digits: `ns`, `us`, `ms`, and `s` with milliseconds in brackets.

| Elapsed | Shown as |
| --- | --- |
| under 1 µs | `<1 us` |
| 1.5 µs | `1.500 us` |
| 1.5 ms | `1.500 ms` |
| 1.5 s | `1.500 s (1500.000 ms)` |

### What is being measured

The clock starts when the expression is accepted and stops when the value is
ready to print. That covers parsing, the arithmetic, and rendering the result
to decimal digits.

Rendering is included deliberately, because it is often the expensive part.
Turning a large integer into decimal digits costs far more than computing it:

| Expression | Arithmetic | Total reported |
| --- | --- | --- |
| `2^1000000` | under 1 ms | ~185 ms |

The exponentiation is essentially a shift; nearly all of the 185 ms is the
base-10 conversion of a 301030-digit number. Reporting only the arithmetic
would have shown a misleadingly small figure.

Process startup and writing to the terminal are *not* included, so the wall
clock time of a shell command is higher — roughly 140 ms of it is process
startup on Windows.

### Piping

When stdout is redirected, the value goes to stdout and the timing goes to
stderr, so a pipeline still receives a bare number:

```
$ omega "2^64" 2>/dev/null
18446744073709551616

$ omega "2^64" 2>/dev/null | bc
18446744073709551616
```

In an interactive session both are shown together.

## Operators

| Operator | Meaning |
| --- | --- |
| `+ - * /` | Arithmetic |
| `%` | Remainder, taking the sign of the dividend |
| `^` | Power, right associative |
| `!` | Factorial, postfix |
| `( )` | Grouping |
| `=` | Assignment |

Precedence follows ordinary mathematical convention:

* `^` binds tighter than unary minus, so `-2^2` is `-4`.
* `^` is right associative, so `2^3^2` is `512`, not `64`.
* `-3!` is `-(3!)`.

A negative base with a fractional exponent stays real when the exponent's
denominator is odd, and the numerator decides the sign:

```
$ omega "(-8)^(1/3)"
-2.0000000000000000000000000000000
$ omega "(-8)^(2/3)"
4.0000000000000000000000000000000
```

An even denominator has no real result and is reported as an error.

`×` and `÷` are accepted as synonyms for `*` and `/`, and `−` (U+2212) for `-`.

## Numbers

* Integers: `42`, `1_000_000`
* Decimals and scientific notation: `3.14`, `1e-9`, `2.5E10`
* Other bases: `0xFF`, `0o755`, `0b1011`

## Functions

| Group | Functions |
| --- | --- |
| Roots | `sqrt(x)`, `cbrt(x)`, `nthroot(x, n)` |
| Rounding | `floor(x)`, `ceil(x)`, `trunc(x)`, `round(x)`, `frac(x)` |
| Other | `abs(x)`, `sign(x)`, `min(a,b)`, `max(a,b)`, `pow(a,b)`, `float(x)` |
| Logarithms | `ln(x)`, `log10(x)`, `log(x)`, `log(base, x)`, `exp(x)` |
| Trigonometric | `sin(x)`, `cos(x)`, `tan(x)`, `asin(x)`, `acos(x)`, `atan(x)` |
| Hyperbolic | `sinh(x)`, `cosh(x)` |
| Number theory | `gcd(a,b)`, `lcm(a,b)`, `isprime(n)`, `digits(n)` |

Constants: `pi`, `e`, `tau`, `phi`.

## Precision

Irrational results are computed to 80 digits by default and displayed to 32.
`-d <n>` raises both, so `-d 1000 "pi"` returns 1000 correct digits. A few
guard digits are computed beyond the requested count so the final displayed
digit is correctly rounded rather than merely truncated.

Trigonometric functions reduce their argument modulo `2*pi` at the working
precision, so `sin(10^30)` is meaningful rather than noise.

## Accuracy

The test suite checks omega's output against published expansions of `pi`,
`e`, `sqrt(2)` and `ln(2)` at both 50 and 100 significant digits, compares
rounded digits rather than prefixes, and verifies exact results such as
`2^1000` and `100!` digit for digit.

```
cargo test
```

## Limits

A result is held in memory, so it has to fit. omega computes exact results up
to **one billion digits**; past that the request is refused, and the error
states how large the answer would have been:

```
$ omega "9999^9999999999"
omega: value too large: the exact result would have about 39_999_565_680 digits,
which exceeds the 1_000_000_000 digit limit (roughly 15.5 GiB of memory)
```

That bound is a property of the machine, not a rule of the tool. `2^1000000`,
`1000!` and `5000!` are all far below it and are computed exactly.

### Sizing a number without computing it

When a value is too large to hold, you can still ask how long it would be.
`size(base, exponent)` reports the digit count of `base^exponent` instantly,
using logarithms, at any scale:

```
$ omega "size(9999, 9999999999)"
39999565680

$ omega "size(2, 1000000)"
301030
```

That 301030 agrees with the actual value:

```
$ omega "digits(2^1000000)"
301030
```

`digits(x)` reports the length of a value that already exists; `size(b, e)`
reports the length of one that does not, and need not exist.

## Notes on the implementation

* Bignum arithmetic uses `num-bigint`.
* Reals are fixed-point: `mantissa * 10^-scale`. Decimal scaling keeps printing
  exact and makes the rounding rule (`RoundHalfAwayFromZero`) obvious.
* `pi` comes from Machin's formula, `exp`/`sin`/`cos`/`ln` from range-reduced
  Taylor and atanh series, roots from Newton's method seeded with an exact
  integer root.
* Digit grouping is applied only to the integer part, never to a fractional
  part, and never inside a fraction. See [Digit grouping](#digit-grouping).

## Performance

Measured with the release profile on the development machine:

| Expression | Result size | Time |
| --- | --- | --- |
| `2^1000000` | 301,030 digits | ~0.4–0.6 s |
| `10000!` | 35,660 digits | ~0.18 s |
| `sqrt(2)` to 100 digits | 100 digits | ~0.13 s |
| `sqrt(2)` or `pi` to 1000 digits | 1000 digits | ~0.14 s |

The release profile enables fat LTO and a single codegen unit.
