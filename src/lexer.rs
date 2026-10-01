//! Tokenizer for the expression language.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Int(String),
    Float(String),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Bang,
    Tilde,
    LParen,
    RParen,
    Comma,
    Eq,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Int(s) | Token::Float(s) | Token::Ident(s) => write!(f, "`{s}`"),
            Token::Plus => write!(f, "`+`"),
            Token::Minus => write!(f, "`-`"),
            Token::Star => write!(f, "`*`"),
            Token::Slash => write!(f, "`/`"),
            Token::Percent => write!(f, "`%`"),
            Token::Caret => write!(f, "`^`"),
            Token::Bang => write!(f, "`!`"),
            Token::Tilde => write!(f, "`~`"),
            Token::LParen => write!(f, "`(`"),
            Token::RParen => write!(f, "`)`"),
            Token::Comma => write!(f, "`,`"),
            Token::Eq => write!(f, "`=`"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

fn err<T>(message: impl Into<String>) -> Result<T, LexError> {
    Err(LexError {
        message: message.into(),
    })
}

/// Splits `input` into tokens. Bases may be written `0xFF`, `0o755`, `0b1011`
/// and digits may be grouped with `_`. Returns the index of an unmatched `(`
/// alongside the tokens so the CLI can offer to close it.
pub fn tokenize(input: &str) -> Result<(Vec<Token>, Option<usize>), LexError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut open_parens: Vec<usize> = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        match c {
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' | '\u{2212}' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' | '\u{00d7}' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' | '\u{00f7}' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '!' => {
                tokens.push(Token::Bang);
                i += 1;
            }
            '~' => {
                tokens.push(Token::Tilde);
                i += 1;
            }
            '(' | '[' => {
                open_parens.push(i);
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' | ']' => {
                open_parens.pop();
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '=' => {
                tokens.push(Token::Eq);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let (token, next) = number(&chars, i)?;
                tokens.push(token);
                i = next;
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                tokens.push(Token::Ident(chars[start..i].iter().collect()));
            }
            other => {
                return err(format!("unexpected character `{other}`"));
            }
        }
    }

    Ok((tokens, open_parens.first().copied()))
}

fn digits_in_base(c: char) -> Option<u32> {
    c.to_digit(36)
}

/// Reads one numeric literal. Returns the token and the index just past it.
fn number(chars: &[char], start: usize) -> Result<(Token, usize), LexError> {
    let mut i = start;

    if chars[i] == '0' && i + 1 < chars.len() {
        let radix = match chars[i + 1] {
            'x' | 'X' => Some(16),
            'o' | 'O' => Some(8),
            'b' | 'B' => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            i += 2;
            let digits_start = i;
            let mut text = String::new();
            while i < chars.len() {
                let c = chars[i];
                if c == '_' {
                    i += 1;
                    continue;
                }
                match digits_in_base(c) {
                    Some(v) if v < radix => {
                        text.push(c);
                        i += 1;
                    }
                    _ => break,
                }
            }
            if text.is_empty() {
                return err(format!(
                    "base-{radix} literal at position {digits_start} has no digits"
                ));
            }
            return Ok((Token::Int(format!("{radix}#{text}")), i));
        }
    }

    let mut text = String::new();
    let mut seen_dot = false;
    let mut seen_exp = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '_' {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            text.push(c);
            i += 1;
            continue;
        }
        if c == '.' && !seen_dot && !seen_exp {
            // `1.2` is a real; a lone `.5` is accepted as `0.5`.
            seen_dot = true;
            text.push(c);
            i += 1;
            continue;
        }
        if (c == 'e' || c == 'E') && !seen_exp && !text.is_empty() {
            let mut j = i + 1;
            if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                j += 1;
            }
            if j < chars.len() && chars[j].is_ascii_digit() {
                seen_exp = true;
                text.push('e');
                if chars[i + 1] == '+' || chars[i + 1] == '-' {
                    text.push(chars[i + 1]);
                }
                i = j;
                continue;
            }
            break;
        }
        break;
    }

    if text.is_empty() || text == "." {
        return err("malformed number");
    }
    if text.starts_with('.') {
        text.insert(0, '0');
    }
    if text.ends_with('.') {
        text.push('0');
    }

    if seen_dot || seen_exp {
        Ok((Token::Float(text), i))
    } else {
        Ok((Token::Int(text), i))
    }
}
