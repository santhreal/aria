//! Statements and expressions of the language, parsed from tokens.

use super::lex::{Pos, Tok, Token};

/// How deep the syntax tree may grow, counting every nested statement,
/// operator, member, call, and index, so hostile source cannot exhaust the
/// stack while it is bound or evaluated.
const MAX_DEPTH: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Binary {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unary {
    Neg,
    Plus,
    Not,
}

/// Where a property's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PropRef {
    /// The property the driver replaces.
    Own,
    /// A declared input, by index.
    Input(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Array(Vec<Expr>, Pos),
    Name(String, Pos),
    Member(Box<Expr>, String, Pos),
    Index(Box<Expr>, Box<Expr>, Pos),
    Call(Box<Expr>, Vec<Expr>, Pos),
    Unary(Unary, Box<Expr>, Pos),
    Binary(Binary, Box<Expr>, Box<Expr>, Pos),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `name = value`, or `name op= value` with the operator.
    Assign(Target, Option<Binary>, Box<Expr>, Pos),
    /// `++` or `--`, before or after the target.
    Step {
        target: Target,
        delta: f64,
        prefix: bool,
        pos: Pos,
    },
    /// A property reference, made by binding.
    Property(PropRef, Pos),
    /// A clip's name or extent, made by binding: the input clip index into
    /// the program's clip list and the attribute.
    Layer(usize, String, Pos),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Target {
    Name(String),
    /// `name[index]`.
    Index(String, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Stmt {
    Let(String, Option<Expr>),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>, Pos),
    Break(Pos),
    Continue(Pos),
    Block(Vec<Stmt>),
}

pub(super) fn parse(tokens: Vec<Token>) -> Result<Vec<Stmt>, String> {
    let mut parser = Parser {
        tokens,
        at: 0,
        depth: 0,
    };
    let mut body = Vec::new();
    while !parser.is_end() {
        if let Some(stmt) = parser.statement()? {
            body.push(stmt);
        }
    }
    Ok(body)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: u32,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at.min(self.tokens.len() - 1)]
    }

    fn pos(&self) -> Pos {
        self.peek().pos
    }

    fn is_end(&self) -> bool {
        self.peek().tok == Tok::End
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.at < self.tokens.len() - 1 {
            self.at += 1;
        }
        token
    }

    fn is(&self, punct: &str) -> bool {
        matches!(&self.peek().tok, Tok::Punct(p) if *p == punct)
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(name) if name == word)
    }

    fn eat(&mut self, punct: &str) -> bool {
        let found = self.is(punct);
        if found {
            self.advance();
        }
        found
    }

    fn expect(&mut self, punct: &str) -> Result<(), String> {
        if self.eat(punct) {
            return Ok(());
        }
        Err(self.unexpected(&format!("expected {punct}")))
    }

    fn unexpected(&self, wanted: &str) -> String {
        let token = self.peek();
        let found = match &token.tok {
            Tok::Num(value) => format!("the number {value}"),
            Tok::Str(text) => format!("the string {text:?}"),
            Tok::Ident(name) => name.clone(),
            Tok::Punct(punct) => (*punct).to_string(),
            Tok::End => "the end of the expression".into(),
        };
        format!("{}: {wanted}, found {found}", token.pos)
    }

    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!(
                "{}: the expression nests more than {MAX_DEPTH} levels deep",
                self.pos()
            ));
        }
        Ok(())
    }

    fn ident(&mut self) -> Result<String, String> {
        match &self.peek().tok {
            Tok::Ident(name) if !is_keyword(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            _ => Err(self.unexpected("expected a name")),
        }
    }

    /// One statement; `None` for an empty one.
    fn statement(&mut self) -> Result<Option<Stmt>, String> {
        self.deeper()?;
        let stmt = self.statement_inner();
        self.depth -= 1;
        stmt
    }

    fn statement_inner(&mut self) -> Result<Option<Stmt>, String> {
        if self.eat(";") {
            return Ok(None);
        }
        if self.is("{") {
            return Ok(Some(Stmt::Block(self.block()?)));
        }
        let pos = self.pos();
        let Tok::Ident(word) = &self.peek().tok else {
            let expr = self.expression()?;
            self.end_statement()?;
            return Ok(Some(Stmt::Expr(expr)));
        };
        let word = word.clone();
        match word.as_str() {
            "var" | "let" | "const" => {
                self.advance();
                let stmt = self.declaration()?;
                self.end_statement()?;
                Ok(Some(stmt))
            }
            "if" => {
                self.advance();
                self.expect("(")?;
                let cond = self.expression()?;
                self.expect(")")?;
                let then = self.body()?;
                let otherwise = if self.is_word("else") {
                    self.advance();
                    self.body()?
                } else {
                    Vec::new()
                };
                Ok(Some(Stmt::If(cond, then, otherwise)))
            }
            "while" => {
                self.advance();
                self.expect("(")?;
                let cond = self.expression()?;
                self.expect(")")?;
                Ok(Some(Stmt::While(cond, self.body()?)))
            }
            "for" => {
                self.advance();
                self.expect("(")?;
                let init = if self.eat(";") {
                    None
                } else {
                    let init = if self.is_word("var") || self.is_word("let") {
                        self.advance();
                        self.declaration()?
                    } else {
                        Stmt::Expr(self.expression()?)
                    };
                    self.expect(";")?;
                    Some(Box::new(init))
                };
                let cond = if self.is(";") {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.expect(";")?;
                let step = if self.is(")") {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.expect(")")?;
                Ok(Some(Stmt::For {
                    init,
                    cond,
                    step,
                    body: self.body()?,
                }))
            }
            "return" => {
                self.advance();
                let value = if self.is(";") || self.is("}") || self.is_end() {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.end_statement()?;
                Ok(Some(Stmt::Return(value, pos)))
            }
            "break" => {
                self.advance();
                self.end_statement()?;
                Ok(Some(Stmt::Break(pos)))
            }
            "continue" => {
                self.advance();
                self.end_statement()?;
                Ok(Some(Stmt::Continue(pos)))
            }
            "function" | "else" | "do" | "switch" | "class" | "new" | "try" | "throw" => {
                Err(format!("{pos}: {word} is not part of aria.expressions"))
            }
            _ => {
                let expr = self.expression()?;
                self.end_statement()?;
                Ok(Some(Stmt::Expr(expr)))
            }
        }
    }

    fn declaration(&mut self) -> Result<Stmt, String> {
        let name = self.ident()?;
        let value = if self.eat("=") {
            Some(self.expression()?)
        } else {
            None
        };
        Ok(Stmt::Let(name, value))
    }

    /// A statement ends at `;`, before `}`, at the end, or at a line break.
    fn end_statement(&mut self) -> Result<(), String> {
        if self.eat(";") || self.is("}") || self.is_end() {
            return Ok(());
        }
        let previous = self.tokens[self.at.saturating_sub(1)].pos.line;
        if self.pos().line > previous {
            return Ok(());
        }
        Err(self.unexpected("expected ; or a new line"))
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect("{")?;
        let mut body = Vec::new();
        while !self.is("}") {
            if self.is_end() {
                return Err(self.unexpected("expected }"));
            }
            if let Some(stmt) = self.statement()? {
                body.push(stmt);
            }
        }
        self.advance();
        Ok(body)
    }

    /// A braced block or a single statement.
    fn body(&mut self) -> Result<Vec<Stmt>, String> {
        if self.is("{") {
            return self.block();
        }
        Ok(self.statement()?.into_iter().collect())
    }

    fn expression(&mut self) -> Result<Expr, String> {
        self.deeper()?;
        let expr = self.assignment();
        self.depth -= 1;
        expr
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let target = self.conditional()?;
        let op = match &self.peek().tok {
            Tok::Punct("=") => None,
            Tok::Punct("+=") => Some(Binary::Add),
            Tok::Punct("-=") => Some(Binary::Sub),
            Tok::Punct("*=") => Some(Binary::Mul),
            Tok::Punct("/=") => Some(Binary::Div),
            _ => return Ok(target),
        };
        let pos = self.advance().pos;
        let target = assign_target(target, pos)?;
        let value = self.expression()?;
        Ok(Expr::Assign(target, op, Box::new(value), pos))
    }

    fn conditional(&mut self) -> Result<Expr, String> {
        let cond = self.or()?;
        if !self.eat("?") {
            return Ok(cond);
        }
        let then = self.expression()?;
        self.expect(":")?;
        let otherwise = self.expression()?;
        Ok(Expr::Cond(
            Box::new(cond),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        let depth = self.depth;
        while self.eat("||") {
            self.deeper()?;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        self.depth = depth;
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.equality()?;
        let depth = self.depth;
        while self.eat("&&") {
            self.deeper()?;
            left = Expr::And(Box::new(left), Box::new(self.equality()?));
        }
        self.depth = depth;
        Ok(left)
    }

    fn binary_level(
        &mut self,
        ops: &[(&str, Binary)],
        next: fn(&mut Self) -> Result<Expr, String>,
    ) -> Result<Expr, String> {
        let mut left = next(self)?;
        // A chain nests leftward, one level per operator.
        let depth = self.depth;
        loop {
            let Some(&(_, op)) = ops.iter().find(|(punct, _)| self.is(punct)) else {
                self.depth = depth;
                return Ok(left);
            };
            self.deeper()?;
            let pos = self.advance().pos;
            let right = next(self)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), pos);
        }
    }

    fn equality(&mut self) -> Result<Expr, String> {
        self.binary_level(
            &[
                ("===", Binary::Eq),
                ("!==", Binary::NotEq),
                ("==", Binary::Eq),
                ("!=", Binary::NotEq),
            ],
            Self::comparison,
        )
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        self.binary_level(
            &[
                ("<=", Binary::LessEq),
                (">=", Binary::GreaterEq),
                ("<", Binary::Less),
                (">", Binary::Greater),
            ],
            Self::additive,
        )
    }

    fn additive(&mut self) -> Result<Expr, String> {
        self.binary_level(
            &[("+", Binary::Add), ("-", Binary::Sub)],
            Self::multiplicative,
        )
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        self.binary_level(
            &[("*", Binary::Mul), ("/", Binary::Div), ("%", Binary::Rem)],
            Self::unary,
        )
    }

    fn unary(&mut self) -> Result<Expr, String> {
        let pos = self.pos();
        let op = match &self.peek().tok {
            Tok::Punct("-") => Unary::Neg,
            Tok::Punct("+") => Unary::Plus,
            Tok::Punct("!") => Unary::Not,
            Tok::Punct(step @ ("++" | "--")) => {
                let delta = if *step == "++" { 1.0 } else { -1.0 };
                self.advance();
                self.deeper()?;
                let operand = self.unary();
                self.depth -= 1;
                let target = assign_target(operand?, pos)?;
                return Ok(Expr::Step {
                    target,
                    delta,
                    prefix: true,
                    pos,
                });
            }
            _ => return self.postfix(),
        };
        self.advance();
        self.deeper()?;
        let operand = self.unary();
        self.depth -= 1;
        Ok(Expr::Unary(op, Box::new(operand?), pos))
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        let depth = self.depth;
        let result = loop {
            let pos = self.pos();
            if !(self.is(".") || self.is("(") || self.is("[")) {
                if let Tok::Punct(step @ ("++" | "--")) = self.peek().tok {
                    let delta = if step == "++" { 1.0 } else { -1.0 };
                    self.advance();
                    break Ok(Expr::Step {
                        target: assign_target(expr, pos)?,
                        delta,
                        prefix: false,
                        pos,
                    });
                }
                break Ok(expr);
            }
            self.deeper()?;
            if self.eat(".") {
                let Tok::Ident(name) = self.advance().tok else {
                    return Err(format!("{pos}: expected a name after ."));
                };
                expr = Expr::Member(Box::new(expr), name, pos);
            } else if self.eat("(") {
                let mut args = Vec::new();
                while !self.is(")") {
                    args.push(self.expression()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
                expr = Expr::Call(Box::new(expr), args, pos);
            } else if self.eat("[") {
                let index = self.expression()?;
                self.expect("]")?;
                expr = Expr::Index(Box::new(expr), Box::new(index), pos);
            }
        };
        self.depth = depth;
        result
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self.peek().clone();
        let pos = token.pos;
        match token.tok {
            Tok::Num(value) => {
                self.advance();
                Ok(Expr::Num(value))
            }
            Tok::Str(text) => {
                self.advance();
                Ok(Expr::Str(text))
            }
            Tok::Punct("(") => {
                self.advance();
                let expr = self.expression()?;
                self.expect(")")?;
                Ok(expr)
            }
            Tok::Punct("[") => {
                self.advance();
                let mut items = Vec::new();
                while !self.is("]") {
                    items.push(self.expression()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("]")?;
                Ok(Expr::Array(items, pos))
            }
            Tok::Ident(name) => {
                self.advance();
                Ok(match name.as_str() {
                    "true" => Expr::Bool(true),
                    "false" => Expr::Bool(false),
                    "null" | "undefined" => Expr::Null,
                    _ if is_keyword(&name) => {
                        return Err(format!("{pos}: {name} cannot start an expression"));
                    }
                    _ => Expr::Name(name, pos),
                })
            }
            _ => Err(self.unexpected("expected a value")),
        }
    }
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "var"
            | "let"
            | "const"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "return"
            | "break"
            | "continue"
            | "function"
            | "switch"
            | "case"
            | "class"
            | "new"
            | "try"
            | "catch"
            | "throw"
    )
}

fn assign_target(expr: Expr, pos: Pos) -> Result<Target, String> {
    match expr {
        Expr::Name(name, _) => Ok(Target::Name(name)),
        Expr::Index(base, index, _) => match *base {
            Expr::Name(name, _) => Ok(Target::Index(name, index)),
            _ => Err(format!(
                "{pos}: only a variable or one of its items can be assigned"
            )),
        },
        _ => Err(format!(
            "{pos}: only a variable or one of its items can be assigned"
        )),
    }
}
