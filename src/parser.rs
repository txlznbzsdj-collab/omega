//! Abstract syntax tree and the recursive-descent parser.

use crate::lexer::Token;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int {
        text: String,
        radix: u32,
    },
    Float(String),
    Ident(String),
    Neg(Box<Expr>),
    Pos(Box<Expr>),
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Factorial(Box<Expr>),
    Assign {
        name: String,
        value: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Pow => "^",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

fn err<T>(message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        message: message.into(),
    })
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Parses a complete expression. A trailing `=` is tolerated.
pub fn parse(tokens: Vec<Token>) -> Result<Expr, ParseError> {
    let mut parser = Parser { tokens, pos: 0 };
    if parser.tokens.is_empty() {
        return err("empty expression");
    }
    let expr = parser.expression()?;
    if parser.peek() == Some(&Token::Eq) {
        parser.pos += 1;
    }
    if let Some(token) = parser.peek() {
        return err(format!(
            "unexpected {token} after the end of the expression"
        ));
    }
    Ok(expr)
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// `name = expression`, right associative, lowest precedence.
    fn expression(&mut self) -> Result<Expr, ParseError> {
        if let (Some(Token::Ident(name)), Some(Token::Eq)) =
            (self.peek().cloned(), self.tokens.get(self.pos + 1))
        {
            let name = name.clone();
            self.pos += 2;
            let value = self.expression()?;
            return Ok(Expr::Assign {
                name,
                value: Box::new(value),
            });
        }
        self.additive()
    }

    fn additive(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.multiplicative()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                Some(Token::Percent) => BinOp::Rem,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.unary()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// `-x` binds looser than `^` so that `-2^2` is `-(2^2)`, and looser than
    /// `!` so that `-3!` is `-(3!)`.
    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Token::Plus) => {
                self.pos += 1;
                Ok(Expr::Pos(Box::new(self.unary()?)))
            }
            _ => self.power(),
        }
    }

    /// `^` is right associative: `2^3^2` is `2^(3^2)`.
    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.postfix()?;
        if self.eat(&Token::Caret) {
            let exponent = self.unary()?;
            return Ok(Expr::Bin {
                op: BinOp::Pow,
                lhs: Box::new(base),
                rhs: Box::new(exponent),
            });
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.primary()?;
        while self.eat(&Token::Bang) {
            expr = Expr::Factorial(Box::new(expr));
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let token = match self.advance() {
            Some(token) => token,
            None => return err("expression ended unexpectedly"),
        };
        match token {
            Token::Int(text) => {
                let (radix, digits) = match text.split_once('#') {
                    Some((radix, digits)) => {
                        (radix.parse::<u32>().unwrap_or(10), digits.to_string())
                    }
                    None => (10, text),
                };
                Ok(Expr::Int {
                    text: digits,
                    radix,
                })
            }
            Token::Float(text) => Ok(Expr::Float(text)),
            Token::Ident(name) => {
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&Token::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if self.eat(&Token::Comma) {
                                continue;
                            }
                            if self.eat(&Token::RParen) {
                                break;
                            }
                            return err(format!(
                                "expected `,` or `)` in the argument list of `{name}`"
                            ));
                        }
                    }
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Token::LParen => {
                let inner = self.expression()?;
                if !self.eat(&Token::RParen) {
                    return err("missing `)`");
                }
                Ok(inner)
            }
            other => err(format!("unexpected {other}")),
        }
    }
}
