//! Morroc DSL - a minimal scripting language for the Morroc Ragnarok Online server.
pub mod lexer {
    use logos::Logos;

    #[derive(Logos, Debug, Clone, PartialEq)]
    #[logos(skip r"[ \t\r\n]+")]
    #[logos(skip r"//[^\n]*")]
    pub enum Token {
        #[token("fn")]
        Fn,
        #[token("on")]
        On,
        #[token("let")]
        Let,
        #[token("if")]
        If,
        #[token("else")]
        Else,
        #[token("while")]
        While,
        #[token("return")]
        Return,
        #[token("true")]
        True,
        #[token("false")]
        False,
        #[token("and")]
        And,
        #[token("or")]
        Or,
        #[token("not")]
        Not,

        #[token("+")]
        Plus,
        #[token("-")]
        Minus,
        #[token("*")]
        Star,
        #[token("/")]
        Slash,
        #[token("(")]
        LParen,
        #[token(")")]
        RParen,
        #[token("{")]
        LBrace,
        #[token("}")]
        RBrace,
        #[token(",")]
        Comma,
        #[token(";")]
        Semicolon,
        #[token("=")]
        Eq,
        #[token("==")]
        EqEq,
        #[token("!=")]
        Ne,
        #[token("<")]
        Lt,
        #[token("<=")]
        Le,
        #[token(">")]
        Gt,
        #[token(">=")]
        Ge,

        #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().unwrap())]
        Integer(i64),

        #[regex(r#""[^"]*""#, |lex| lex.slice()[1..lex.slice().len()-1].to_string())]
        String(String),

        #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
        Ident(String),

        Error,
    }

    pub fn lex(source: &str) -> Vec<Token> {
        Token::lexer(source)
            .map(|res| res.unwrap_or(Token::Error))
            .collect()
    }
}

pub mod ast {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Expr {
        Int(i64),
        Bool(bool),
        String(String),
        Ident(String),
        Binary {
            op: BinOp,
            left: Box<Expr>,
            right: Box<Expr>,
        },
        Unary {
            op: UnOp,
            expr: Box<Expr>,
        },
        Call {
            callee: String,
            args: Vec<Expr>,
        },
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BinOp {
        Add,
        Sub,
        Mul,
        Div,
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
        And,
        Or,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UnOp {
        Neg,
        Not,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Stmt {
        Expr(Expr),
        Let {
            name: String,
            value: Expr,
        },
        Assign {
            name: String,
            value: Expr,
        },
        If {
            cond: Expr,
            then_branch: Vec<Stmt>,
            else_branch: Option<Vec<Stmt>>,
        },
        While {
            cond: Expr,
            body: Vec<Stmt>,
        },
        Return(Option<Expr>),
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Item {
        Function {
            name: String,
            params: Vec<String>,
            body: Vec<Stmt>,
        },
        Event {
            name: String,
            params: Vec<String>,
            body: Vec<Stmt>,
        },
    }
}

pub mod parser {
    use super::ast::{BinOp, Expr, Item, Stmt, UnOp};
    use super::lexer::Token;
    use thiserror::Error;

    #[derive(Error, Debug, Clone, PartialEq)]
    pub enum ParseError {
        #[error("unexpected token: expected {expected}, found {found}")]
        Unexpected { expected: String, found: String },
        #[error("unexpected end of input")]
        Eof,
        #[error("invalid integer literal")]
        InvalidInteger,
        #[error("{0}")]
        Other(String),
    }

    pub struct Parser {
        tokens: Vec<Token>,
        pos: usize,
    }

    impl Parser {
        pub fn new(tokens: Vec<Token>) -> Self {
            Parser { tokens, pos: 0 }
        }

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

        fn expect(&mut self, expected: Token) -> Result<Token, ParseError> {
            match self.peek() {
                Some(token) if *token == expected => self.advance().ok_or(ParseError::Eof),
                Some(token) => Err(ParseError::Unexpected {
                    expected: format!("{:?}", expected),
                    found: format!("{:?}", token),
                }),
                None => Err(ParseError::Eof),
            }
        }

        pub fn parse_program(&mut self) -> Result<Vec<Item>, ParseError> {
            let mut items = Vec::new();
            while self.peek().is_some() {
                items.push(self.parse_item()?);
            }
            Ok(items)
        }

        fn parse_item(&mut self) -> Result<Item, ParseError> {
            match self.peek() {
                Some(Token::Fn) => {
                    self.advance();
                    let name = self.expect_ident()?;
                    self.expect(Token::LParen)?;
                    let params = self.parse_params()?;
                    self.expect(Token::RParen)?;
                    let body = self.parse_block()?;
                    Ok(Item::Function { name, params, body })
                }
                Some(Token::On) => {
                    self.advance();
                    let name = self.expect_ident()?;
                    self.expect(Token::LParen)?;
                    let params = self.parse_params()?;
                    self.expect(Token::RParen)?;
                    let body = self.parse_block()?;
                    Ok(Item::Event { name, params, body })
                }
                Some(token) => Err(ParseError::Unexpected {
                    expected: "fn or on".to_string(),
                    found: format!("{:?}", token),
                }),
                None => Err(ParseError::Eof),
            }
        }

        fn expect_ident(&mut self) -> Result<String, ParseError> {
            match self.advance() {
                Some(Token::Ident(name)) => Ok(name),
                Some(token) => Err(ParseError::Unexpected {
                    expected: "identifier".to_string(),
                    found: format!("{:?}", token),
                }),
                None => Err(ParseError::Eof),
            }
        }

        fn parse_params(&mut self) -> Result<Vec<String>, ParseError> {
            let mut params = Vec::new();
            if matches!(self.peek(), Some(Token::Ident(_))) {
                params.push(self.expect_ident()?);
                while matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                    params.push(self.expect_ident()?);
                }
            }
            Ok(params)
        }

        fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
            self.expect(Token::LBrace)?;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Some(Token::RBrace) | None) {
                stmts.push(self.parse_stmt()?);
            }
            self.expect(Token::RBrace)?;
            Ok(stmts)
        }

        fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
            match self.peek() {
                Some(Token::Let) => self.parse_let(),
                Some(Token::If) => self.parse_if(),
                Some(Token::While) => self.parse_while(),
                Some(Token::Return) => self.parse_return(),
                Some(Token::Ident(_)) => {
                    if matches!(self.tokens.get(self.pos + 1), Some(Token::Eq)) {
                        self.parse_assign()
                    } else {
                        let expr = self.parse_expr()?;
                        self.expect(Token::Semicolon)?;
                        Ok(Stmt::Expr(expr))
                    }
                }
                _ => {
                    let expr = self.parse_expr()?;
                    self.expect(Token::Semicolon)?;
                    Ok(Stmt::Expr(expr))
                }
            }
        }

        fn parse_let(&mut self) -> Result<Stmt, ParseError> {
            self.expect(Token::Let)?;
            let name = self.expect_ident()?;
            self.expect(Token::Eq)?;
            let value = self.parse_expr()?;
            self.expect(Token::Semicolon)?;
            Ok(Stmt::Let { name, value })
        }

        fn parse_assign(&mut self) -> Result<Stmt, ParseError> {
            let name = self.expect_ident()?;
            self.expect(Token::Eq)?;
            let value = self.parse_expr()?;
            self.expect(Token::Semicolon)?;
            Ok(Stmt::Assign { name, value })
        }

        fn parse_if(&mut self) -> Result<Stmt, ParseError> {
            self.expect(Token::If)?;
            let cond = self.parse_expr()?;
            let then_branch = self.parse_block()?;
            let else_branch = if matches!(self.peek(), Some(Token::Else)) {
                self.advance();
                Some(self.parse_block()?)
            } else {
                None
            };
            Ok(Stmt::If {
                cond,
                then_branch,
                else_branch,
            })
        }

        fn parse_while(&mut self) -> Result<Stmt, ParseError> {
            self.expect(Token::While)?;
            let cond = self.parse_expr()?;
            let body = self.parse_block()?;
            Ok(Stmt::While { cond, body })
        }

        fn parse_return(&mut self) -> Result<Stmt, ParseError> {
            self.expect(Token::Return)?;
            let value = if !matches!(self.peek(), Some(Token::Semicolon)) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::Semicolon)?;
            Ok(Stmt::Return(value))
        }

        pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
            self.parse_or()
        }

        fn parse_or(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_and()?;
            while matches!(self.peek(), Some(Token::Or)) {
                self.advance();
                let right = self.parse_and()?;
                left = Expr::Binary {
                    op: BinOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_and(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_equality()?;
            while matches!(self.peek(), Some(Token::And)) {
                self.advance();
                let right = self.parse_equality()?;
                left = Expr::Binary {
                    op: BinOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_equality(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_comparison()?;
            while matches!(self.peek(), Some(Token::EqEq) | Some(Token::Ne)) {
                let op = match self.advance().unwrap() {
                    Token::EqEq => BinOp::Eq,
                    Token::Ne => BinOp::Ne,
                    _ => unreachable!(),
                };
                let right = self.parse_comparison()?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_add_sub()?;
            while matches!(
                self.peek(),
                Some(Token::Lt) | Some(Token::Le) | Some(Token::Gt) | Some(Token::Ge)
            ) {
                let op = match self.advance().unwrap() {
                    Token::Lt => BinOp::Lt,
                    Token::Le => BinOp::Le,
                    Token::Gt => BinOp::Gt,
                    Token::Ge => BinOp::Ge,
                    _ => unreachable!(),
                };
                let right = self.parse_add_sub()?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_add_sub(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_mul_div()?;
            while matches!(self.peek(), Some(Token::Plus) | Some(Token::Minus)) {
                let op = match self.advance().unwrap() {
                    Token::Plus => BinOp::Add,
                    Token::Minus => BinOp::Sub,
                    _ => unreachable!(),
                };
                let right = self.parse_mul_div()?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_mul_div(&mut self) -> Result<Expr, ParseError> {
            let mut left = self.parse_unary()?;
            while matches!(self.peek(), Some(Token::Star) | Some(Token::Slash)) {
                let op = match self.advance().unwrap() {
                    Token::Star => BinOp::Mul,
                    Token::Slash => BinOp::Div,
                    _ => unreachable!(),
                };
                let right = self.parse_unary()?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            Ok(left)
        }

        fn parse_unary(&mut self) -> Result<Expr, ParseError> {
            match self.peek() {
                Some(Token::Minus) => {
                    self.advance();
                    let expr = self.parse_unary()?;
                    Ok(Expr::Unary {
                        op: UnOp::Neg,
                        expr: Box::new(expr),
                    })
                }
                Some(Token::Not) => {
                    self.advance();
                    let expr = self.parse_unary()?;
                    Ok(Expr::Unary {
                        op: UnOp::Not,
                        expr: Box::new(expr),
                    })
                }
                _ => self.parse_primary(),
            }
        }

        fn parse_primary(&mut self) -> Result<Expr, ParseError> {
            match self.advance() {
                Some(Token::Integer(n)) => Ok(Expr::Int(n)),
                Some(Token::String(s)) => Ok(Expr::String(s)),
                Some(Token::True) => Ok(Expr::Bool(true)),
                Some(Token::False) => Ok(Expr::Bool(false)),
                Some(Token::Ident(name)) => {
                    if matches!(self.peek(), Some(Token::LParen)) {
                        self.advance();
                        let args = self.parse_args()?;
                        self.expect(Token::RParen)?;
                        Ok(Expr::Call { callee: name, args })
                    } else {
                        Ok(Expr::Ident(name))
                    }
                }
                Some(Token::LParen) => {
                    let expr = self.parse_expr()?;
                    self.expect(Token::RParen)?;
                    Ok(expr)
                }
                Some(token) => Err(ParseError::Unexpected {
                    expected: "expression".to_string(),
                    found: format!("{:?}", token),
                }),
                None => Err(ParseError::Eof),
            }
        }

        fn parse_args(&mut self) -> Result<Vec<Expr>, ParseError> {
            let mut args = Vec::new();
            if !matches!(self.peek(), Some(Token::RParen)) {
                args.push(self.parse_expr()?);
                while matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                    args.push(self.parse_expr()?);
                }
            }
            Ok(args)
        }
    }

    pub fn parse(source: &str) -> Result<Vec<Item>, ParseError> {
        let tokens = super::lexer::lex(source);
        Parser::new(tokens).parse_program()
    }

    pub fn parse_tokens(tokens: Vec<Token>) -> Result<Vec<Item>, ParseError> {
        Parser::new(tokens).parse_program()
    }
}

pub mod compiler {
    use super::ast::{BinOp, Expr, Item, Stmt, UnOp};
    use super::parser::ParseError;
    use std::collections::HashMap;
    use thiserror::Error;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Int(i64),
        Bool(bool),
        String(String),
        Nil,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Op {
        Push(Value),
        Pop,
        Add,
        Sub,
        Mul,
        Div,
        Neg,
        Not,
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
        And,
        Or,
        Jump(usize),
        JumpIfFalse(usize),
        Call { name: String, args: usize },
        Return,
        GetLocal(usize),
        SetLocal(usize),
        GetGlobal(String),
        SetGlobal(String),
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct Chunk {
        pub ops: Vec<Op>,
        pub param_count: usize,
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct Program {
        pub functions: HashMap<String, Chunk>,
        pub events: HashMap<String, Chunk>,
    }

    #[derive(Error, Debug, Clone, PartialEq)]
    pub enum CompileError {
        #[error("unknown variable: {0}")]
        UnknownVariable(String),
        #[error("duplicate function: {0}")]
        DuplicateFunction(String),
        #[error("duplicate event: {0}")]
        DuplicateEvent(String),
        #[error("undefined function: {0}")]
        UndefinedFunction(String),
        #[error("wrong number of arguments: expected {expected}, got {got}")]
        WrongArgCount { expected: usize, got: usize },
        #[error("{0}")]
        Other(String),
    }

    struct Compiler {
        locals: Vec<String>,
        ops: Vec<Op>,
        param_count: usize,
    }

    impl Compiler {
        fn new(params: &[String]) -> Self {
            Compiler {
                locals: params.to_vec(),
                ops: Vec::new(),
                param_count: params.len(),
            }
        }

        fn emit(&mut self, op: Op) -> usize {
            let idx = self.ops.len();
            self.ops.push(op);
            idx
        }

        fn add_local(&mut self, name: String) -> usize {
            let idx = self.locals.len();
            self.locals.push(name);
            idx
        }

        fn resolve_local(&self, name: &str) -> Option<usize> {
            self.locals.iter().rposition(|n| n == name)
        }

        fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
            match expr {
                Expr::Int(n) => {
                    self.emit(Op::Push(Value::Int(*n)));
                }
                Expr::Bool(b) => {
                    self.emit(Op::Push(Value::Bool(*b)));
                }
                Expr::String(s) => {
                    self.emit(Op::Push(Value::String(s.clone())));
                }
                Expr::Ident(name) => {
                    if let Some(idx) = self.resolve_local(name) {
                        self.emit(Op::GetLocal(idx));
                    } else {
                        self.emit(Op::GetGlobal(name.clone()));
                    }
                }
                Expr::Binary { op, left, right } => {
                    self.compile_expr(left)?;
                    self.compile_expr(right)?;
                    let op = match op {
                        BinOp::Add => Op::Add,
                        BinOp::Sub => Op::Sub,
                        BinOp::Mul => Op::Mul,
                        BinOp::Div => Op::Div,
                        BinOp::Eq => Op::Eq,
                        BinOp::Ne => Op::Ne,
                        BinOp::Lt => Op::Lt,
                        BinOp::Le => Op::Le,
                        BinOp::Gt => Op::Gt,
                        BinOp::Ge => Op::Ge,
                        BinOp::And => Op::And,
                        BinOp::Or => Op::Or,
                    };
                    self.emit(op);
                }
                Expr::Unary { op, expr } => {
                    self.compile_expr(expr)?;
                    match op {
                        UnOp::Neg => {
                            self.emit(Op::Neg);
                        }
                        UnOp::Not => {
                            self.emit(Op::Not);
                        }
                    };
                }
                Expr::Call { callee, args } => {
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit(Op::Call {
                        name: callee.clone(),
                        args: args.len(),
                    });
                }
            }
            Ok(())
        }

        fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
            match stmt {
                Stmt::Expr(expr) => {
                    self.compile_expr(expr)?;
                    self.emit(Op::Pop);
                }
                Stmt::Let { name, value } => {
                    self.compile_expr(value)?;
                    self.add_local(name.clone());
                    self.emit(Op::SetLocal(self.locals.len() - 1));
                }
                Stmt::Assign { name, value } => {
                    self.compile_expr(value)?;
                    if let Some(idx) = self.resolve_local(name) {
                        self.emit(Op::SetLocal(idx));
                    } else {
                        self.emit(Op::SetGlobal(name.clone()));
                    }
                }
                Stmt::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    self.compile_expr(cond)?;
                    let then_jump = self.emit(Op::JumpIfFalse(0));
                    for stmt in then_branch {
                        self.compile_stmt(stmt)?;
                    }
                    if else_branch.is_some() {
                        let else_jump = self.emit(Op::Jump(0));
                        self.patch_jump(then_jump);
                        for stmt in else_branch.as_ref().unwrap() {
                            self.compile_stmt(stmt)?;
                        }
                        self.patch_jump(else_jump);
                    } else {
                        self.patch_jump(then_jump);
                    }
                }
                Stmt::While { cond, body } => {
                    let loop_start = self.ops.len();
                    self.compile_expr(cond)?;
                    let exit_jump = self.emit(Op::JumpIfFalse(0));
                    for stmt in body {
                        self.compile_stmt(stmt)?;
                    }
                    let loop_jump = self.emit(Op::Jump(0));
                    self.patch_jump_to(loop_jump, loop_start);
                    self.patch_jump(exit_jump);
                }
                Stmt::Return(expr) => {
                    if let Some(expr) = expr {
                        self.compile_expr(expr)?;
                    } else {
                        self.emit(Op::Push(Value::Nil));
                    }
                    self.emit(Op::Return);
                }
            }
            Ok(())
        }

        fn patch_jump(&mut self, jump_idx: usize) {
            let target = self.ops.len();
            self.patch_jump_to(jump_idx, target);
        }

        fn patch_jump_to(&mut self, jump_idx: usize, target: usize) {
            let offset = target as isize - jump_idx as isize - 1;
            match &mut self.ops[jump_idx] {
                Op::Jump(ref mut o) | Op::JumpIfFalse(ref mut o) => {
                    *o = offset as usize;
                }
                _ => panic!("tried to patch non-jump instruction"),
            }
        }

        fn compile_item(item: &Item) -> Result<(String, Chunk, bool), CompileError> {
            let (name, params, body, is_event) = match item {
                Item::Function { name, params, body } => (name.clone(), params, body, false),
                Item::Event { name, params, body } => (format!("on_{}", name), params, body, true),
            };
            let mut compiler = Compiler::new(params);
            for stmt in body {
                compiler.compile_stmt(stmt)?;
            }
            compiler.emit(Op::Push(Value::Nil));
            compiler.emit(Op::Return);
            Ok((
                name,
                Chunk {
                    ops: compiler.ops,
                    param_count: compiler.param_count,
                },
                is_event,
            ))
        }
    }

    pub fn compile_items(items: &[Item]) -> Result<Program, CompileError> {
        let mut program = Program::default();
        for item in items {
            let (name, chunk, is_event) = Compiler::compile_item(item)?;
            let map = if is_event {
                &mut program.events
            } else {
                &mut program.functions
            };
            if map.contains_key(&name) {
                return Err(if is_event {
                    CompileError::DuplicateEvent(name)
                } else {
                    CompileError::DuplicateFunction(name)
                });
            }
            map.insert(name, chunk);
        }
        Ok(program)
    }

    pub fn compile(source: &str) -> Result<Program, ParseError> {
        let items = super::parser::parse(source)?;
        compile_items(&items).map_err(|e| ParseError::Other(e.to_string()))
    }
}

pub mod vm {
    use super::compiler::{Chunk, Op, Program, Value};
    use std::collections::HashMap;
    use std::sync::Arc;
    use thiserror::Error;

    #[derive(Error, Debug, Clone, PartialEq)]
    pub enum RuntimeError {
        #[error("type error: {0}")]
        TypeError(String),
        #[error("undefined function: {0}")]
        UndefinedFunction(String),
        #[error("missing variable: {0}")]
        MissingVariable(String),
        #[error("wrong argument count: expected {expected}, got {got}")]
        WrongArgCount { expected: usize, got: usize },
        #[error("empty stack")]
        EmptyStack,
        #[error("{0}")]
        Other(String),
    }

    #[allow(clippy::type_complexity)]
    pub type NativeFn = Arc<dyn Fn(&mut Vm, &[Value]) -> Result<Value, RuntimeError> + Send + Sync>;

    pub struct Vm {
        pub globals: HashMap<String, Value>,
        pub stack: Vec<Value>,
        pub natives: HashMap<String, NativeFn>,
        pub program: Program,
    }

    impl std::fmt::Debug for Vm {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Vm")
                .field("globals", &self.globals)
                .field("stack", &self.stack)
                .field("natives", &self.natives.len())
                .field("program", &self.program)
                .finish()
        }
    }

    impl Vm {
        pub fn new() -> Self {
            let mut vm = Vm {
                globals: HashMap::new(),
                stack: Vec::new(),
                natives: HashMap::new(),
                program: Program::default(),
            };
            vm.register_native(
                "print",
                Box::new(|_vm, args| {
                    let msg = args
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::info!("{}", msg);
                    Ok(Value::Nil)
                }),
            );
            vm.register_native(
                "say",
                Box::new(|_vm, args| {
                    let msg = args
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::info!("[say] {}", msg);
                    Ok(Value::Nil)
                }),
            );
            vm
        }

        pub fn with_program(program: Program) -> Self {
            let mut vm = Self::new();
            vm.program = program;
            vm
        }

        #[allow(clippy::type_complexity)]
        pub fn register_native(
            &mut self,
            name: &str,
            f: Box<dyn Fn(&mut Vm, &[Value]) -> Result<Value, RuntimeError> + Send + Sync>,
        ) {
            self.natives.insert(name.to_string(), Arc::from(f));
        }

        pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
            if let Some(native) = self.natives.get(name) {
                let f = native.clone();
                return f(self, args);
            }
            if let Some(chunk) = self.program.functions.get(name).cloned() {
                return self.run_chunk(&chunk, args);
            }
            if let Some(chunk) = self.program.events.get(name).cloned() {
                return self.run_chunk(&chunk, args);
            }
            Err(RuntimeError::UndefinedFunction(name.to_string()))
        }

        fn run_chunk(&mut self, chunk: &Chunk, args: &[Value]) -> Result<Value, RuntimeError> {
            if args.len() != chunk.param_count {
                return Err(RuntimeError::WrongArgCount {
                    expected: chunk.param_count,
                    got: args.len(),
                });
            }
            let mut locals: Vec<Value> = args.to_vec();
            let mut ip: usize = 0;
            while ip < chunk.ops.len() {
                let op = &chunk.ops[ip];
                match op {
                    Op::Push(v) => self.stack.push(v.clone()),
                    Op::Pop => {
                        self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                    }
                    Op::Add => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                        (Value::String(a), Value::String(b)) => {
                            Ok(Value::String(format!("{}{}", a, b)))
                        }
                        _ => Err(RuntimeError::TypeError("add".to_string())),
                    })?,
                    Op::Sub => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                        _ => Err(RuntimeError::TypeError("sub".to_string())),
                    })?,
                    Op::Mul => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                        _ => Err(RuntimeError::TypeError("mul".to_string())),
                    })?,
                    Op::Div => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                        _ => Err(RuntimeError::TypeError("div".to_string())),
                    })?,
                    Op::Neg => {
                        let v = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                        match v {
                            Value::Int(n) => self.stack.push(Value::Int(-n)),
                            _ => return Err(RuntimeError::TypeError("neg".to_string())),
                        }
                    }
                    Op::Not => {
                        let v = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                        match v {
                            Value::Bool(b) => self.stack.push(Value::Bool(!b)),
                            _ => return Err(RuntimeError::TypeError("not".to_string())),
                        }
                    }
                    Op::Eq => self.binary_op(|a, b| Ok(Value::Bool(a == b)))?,
                    Op::Ne => self.binary_op(|a, b| Ok(Value::Bool(a != b)))?,
                    Op::Lt => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                        _ => Err(RuntimeError::TypeError("lt".to_string())),
                    })?,
                    Op::Le => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                        _ => Err(RuntimeError::TypeError("le".to_string())),
                    })?,
                    Op::Gt => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                        _ => Err(RuntimeError::TypeError("gt".to_string())),
                    })?,
                    Op::Ge => self.binary_op(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                        _ => Err(RuntimeError::TypeError("ge".to_string())),
                    })?,
                    Op::And => self.binary_op(|a, b| match (a, b) {
                        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
                        _ => Err(RuntimeError::TypeError("and".to_string())),
                    })?,
                    Op::Or => self.binary_op(|a, b| match (a, b) {
                        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
                        _ => Err(RuntimeError::TypeError("or".to_string())),
                    })?,
                    Op::Jump(offset) => {
                        let signed = *offset as isize;
                        ip = (ip as isize + 1 + signed) as usize;
                        continue;
                    }
                    Op::JumpIfFalse(offset) => {
                        let cond = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                        let truthy = !matches!(cond, Value::Bool(false) | Value::Nil);
                        if !truthy {
                            let signed = *offset as isize;
                            ip = (ip as isize + 1 + signed) as usize;
                            continue;
                        }
                    }
                    Op::Call { name, args } => {
                        let argc = *args;
                        if self.stack.len() < argc {
                            return Err(RuntimeError::EmptyStack);
                        }
                        let start = self.stack.len() - argc;
                        let arg_vals = self.stack.split_off(start);
                        let result = self.call(name, &arg_vals)?;
                        self.stack.push(result);
                    }
                    Op::Return => {
                        return self.stack.pop().ok_or(RuntimeError::EmptyStack);
                    }
                    Op::GetLocal(idx) => {
                        let idx = *idx;
                        if idx >= locals.len() {
                            return Err(RuntimeError::MissingVariable(format!("local {}", idx)));
                        }
                        self.stack.push(locals[idx].clone());
                    }
                    Op::SetLocal(idx) => {
                        let idx = *idx;
                        let value = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                        if idx >= locals.len() {
                            locals.resize(idx + 1, Value::Nil);
                        }
                        locals[idx] = value;
                    }
                    Op::GetGlobal(name) => {
                        let value = self
                            .globals
                            .get(name)
                            .cloned()
                            .ok_or_else(|| RuntimeError::MissingVariable(name.clone()))?;
                        self.stack.push(value);
                    }
                    Op::SetGlobal(name) => {
                        let value = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
                        self.globals.insert(name.clone(), value);
                    }
                }
                ip += 1;
            }
            Ok(Value::Nil)
        }

        fn binary_op<F>(&mut self, op: F) -> Result<(), RuntimeError>
        where
            F: FnOnce(Value, Value) -> Result<Value, RuntimeError>,
        {
            let right = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
            let left = self.stack.pop().ok_or(RuntimeError::EmptyStack)?;
            self.stack.push(op(left, right)?);
            Ok(())
        }
    }

    impl Default for Vm {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Display for Value {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Value::Int(n) => write!(f, "{}", n),
                Value::Bool(b) => write!(f, "{}", b),
                Value::String(s) => write!(f, "{}", s),
                Value::Nil => write!(f, "nil"),
            }
        }
    }
}

pub use compiler::compile;
pub use compiler::{Chunk, Op, Program, Value};
pub use lexer::{lex, Token};
pub use parser::{parse, ParseError, Parser};
pub use vm::{RuntimeError, Vm};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_simple_program() {
        let source = r#"
            fn add(a, b) {
                return a + b; // sum
            }
        "#;
        let tokens = lex(source);
        assert!(tokens.iter().any(|t| matches!(t, Token::Fn)));
        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::Ident(name) if name == "add")));
        assert!(tokens.iter().any(|t| matches!(t, Token::Plus)));
        assert!(!tokens.iter().any(|t| matches!(t, Token::Error)));
    }

    #[test]
    fn parse_arithmetic_and_function() {
        let source = r#"fn add(a, b) { return a + b; }"#;
        let items = parse(source).unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            ast::Item::Function { name, params, body } => {
                assert_eq!(name, "add");
                assert_eq!(params, &["a", "b"]);
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected function"),
        }

        let expr = parser::Parser::new(lex("1 + 2 * 3")).parse_expr().unwrap();
        match expr {
            ast::Expr::Binary {
                op: ast::BinOp::Add,
                left,
                right,
            } => {
                assert!(matches!(left.as_ref(), ast::Expr::Int(1)));
                assert!(matches!(
                    right.as_ref(),
                    ast::Expr::Binary {
                        op: ast::BinOp::Mul,
                        ..
                    }
                ));
            }
            _ => panic!("expected add expression"),
        }
    }

    #[test]
    fn compile_and_run_add() {
        let source = r#"fn add(a, b) { return a + b; }"#;
        let program = compile(source).unwrap();
        let mut vm = Vm::with_program(program);
        let result = vm.call("add", &[Value::Int(2), Value::Int(3)]).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn run_if_else() {
        let source = r#"
            fn abs(n) {
                if n < 0 {
                    return -n;
                } else {
                    return n;
                }
            }
        "#;
        let program = compile(source).unwrap();
        let mut vm = Vm::with_program(program);
        assert_eq!(vm.call("abs", &[Value::Int(-5)]).unwrap(), Value::Int(5));
        assert_eq!(vm.call("abs", &[Value::Int(3)]).unwrap(), Value::Int(3));
    }

    #[test]
    fn run_while_loop() {
        let source = r#"
            fn sum_to(n) {
                let total = 0;
                let i = 1;
                while i <= n {
                    total = total + i;
                    i = i + 1;
                }
                return total;
            }
        "#;
        let program = compile(source).unwrap();
        let mut vm = Vm::with_program(program);
        assert_eq!(vm.call("sum_to", &[Value::Int(5)]).unwrap(), Value::Int(15));
    }

    #[test]
    fn call_event_and_concat_strings() {
        let source = r#"
            on server_init() {
                print("server started");
            }
            fn hello(name) {
                return "hello, " + name;
            }
        "#;
        let program = compile(source).unwrap();
        let mut vm = Vm::with_program(program);
        let result = vm.call("on_server_init", &[]).unwrap();
        assert_eq!(result, Value::Nil);
        let result = vm
            .call("hello", &[Value::String("world".to_string())])
            .unwrap();
        assert_eq!(result, Value::String("hello, world".to_string()));
    }
}
