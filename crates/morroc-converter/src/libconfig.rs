//! Legacy libconfig 解析器。
//!
//! 支持解析 `.conf` 数据库文件（item_db.conf、mob_db.conf、skill_db.conf）
//! 和 `npc/scripts.conf` 等清单文件。

use std::collections::HashMap;
use thiserror::Error;

/// 解析错误。
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseError {
    #[error("unexpected token: expected {expected}, found {found}")]
    Unexpected { expected: String, found: String },
    #[error("unexpected end of input")]
    Eof,
    #[error("invalid number: {0}")]
    InvalidNumber(String),
    #[error("unterminated string")]
    UnterminatedString,
    #[error("unknown directive: {0}")]
    UnknownDirective(String),
    #[error("{0}")]
    Other(String),
}

/// libconfig 值。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Array(Vec<Value>),
    Tuple(Vec<Value>),
    Group(HashMap<String, Value>),
    Include(String),
}

/// libconfig 文件解析结果：根级别的命名组映射。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub roots: HashMap<String, Value>,
}

impl Document {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.roots.get(name)
    }
}

/// 解析 libconfig 字符串。
pub fn parse(source: &str) -> Result<Document, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_document()
}

/// 解析 libconfig 文件内容。
pub fn parse_file(source: &str) -> Result<Document, ParseError> {
    parse(source)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    LBrace,   // {
    RBrace,   // }
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    Comma,    // ,
    Colon,    // :
    At,       // @
    Semi,     // ;
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
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
        if self.pos < self.tokens.len() {
            let token = self.tokens[self.pos].clone();
            if token == expected {
                self.pos += 1;
                Ok(token)
            } else {
                Err(ParseError::Unexpected {
                    expected: format!("{:?}", expected),
                    found: format!("{:?}", token),
                })
            }
        } else {
            Err(ParseError::Eof)
        }
    }

    fn parse_document(&mut self) -> Result<Document, ParseError> {
        let mut roots = HashMap::new();
        while self.peek().is_some() {
            let name = match self.advance() {
                Some(Token::Ident(n)) => n,
                Some(token) => {
                    return Err(ParseError::Unexpected {
                        expected: "root identifier".to_string(),
                        found: format!("{:?}", token),
                    })
                }
                None => break,
            };
            self.expect(Token::Colon)?;
            let value = self.parse_value()?;
            if let Value::Include(path) = value {
                // 清单文件（如 scripts.conf）顶层会包含 include 指令；
                // 目前将其作为 Include 节点保留，后续处理。
                roots.insert(name, Value::Include(path));
            } else {
                roots.insert(name, value);
            }
            // 根条目后逗号可选
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        Ok(Document { roots })
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(Token::At) => self.parse_directive(),
            Some(Token::LBrace) => self.parse_group(),
            Some(Token::LParen) => self.parse_list_or_tuple(),
            Some(Token::LBracket) => self.parse_array(),
            Some(Token::String(_)) => self.parse_string_value(),
            Some(Token::Int(_))
            | Some(Token::Float(_))
            | Some(Token::Bool(_))
            | Some(Token::Ident(_)) => self.parse_scalar(),
            Some(token) => Err(ParseError::Unexpected {
                expected: "value".to_string(),
                found: format!("{:?}", token),
            }),
            None => Err(ParseError::Eof),
        }
    }

    fn parse_directive(&mut self) -> Result<Value, ParseError> {
        self.expect(Token::At)?;
        let name = match self.advance() {
            Some(Token::Ident(n)) => n,
            Some(token) => {
                return Err(ParseError::Unexpected {
                    expected: "directive name".to_string(),
                    found: format!("{:?}", token),
                })
            }
            None => return Err(ParseError::Eof),
        };
        match name.as_str() {
            "include" => {
                let path = match self.advance() {
                    Some(Token::String(s)) => s,
                    Some(token) => {
                        return Err(ParseError::Unexpected {
                            expected: "include path string".to_string(),
                            found: format!("{:?}", token),
                        })
                    }
                    None => return Err(ParseError::Eof),
                };
                Ok(Value::Include(path))
            }
            _ => Err(ParseError::UnknownDirective(name)),
        }
    }

    fn parse_group(&mut self) -> Result<Value, ParseError> {
        self.expect(Token::LBrace)?;
        let mut group = HashMap::new();
        while !matches!(self.peek(), Some(Token::RBrace) | None) {
            let key = match self.advance() {
                Some(Token::Ident(k)) => k,
                Some(token) => {
                    return Err(ParseError::Unexpected {
                        expected: "group key".to_string(),
                        found: format!("{:?}", token),
                    })
                }
                None => return Err(ParseError::Eof),
            };
            self.expect(Token::Colon)?;
            let value = self.parse_value()?;
            group.insert(key, value);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Value::Group(group))
    }

    fn parse_list_or_tuple(&mut self) -> Result<Value, ParseError> {
        self.expect(Token::LParen)?;
        let mut values = Vec::new();
        while !matches!(self.peek(), Some(Token::RParen) | None) {
            values.push(self.parse_value()?);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        self.expect(Token::RParen)?;
        // 区分 tuple 与 list：这里统一使用 Tuple，上层按需解释。
        Ok(Value::Tuple(values))
    }

    fn parse_array(&mut self) -> Result<Value, ParseError> {
        self.expect(Token::LBracket)?;
        let mut values = Vec::new();
        while !matches!(self.peek(), Some(Token::RBracket) | None) {
            values.push(self.parse_value()?);
            if matches!(self.peek(), Some(Token::Comma)) {
                self.advance();
            }
        }
        self.expect(Token::RBracket)?;
        Ok(Value::Array(values))
    }

    fn parse_string_value(&mut self) -> Result<Value, ParseError> {
        match self.advance() {
            Some(Token::String(s)) => Ok(Value::String(s)),
            Some(token) => Err(ParseError::Unexpected {
                expected: "string".to_string(),
                found: format!("{:?}", token),
            }),
            None => Err(ParseError::Eof),
        }
    }

    fn parse_scalar(&mut self) -> Result<Value, ParseError> {
        match self.advance() {
            Some(Token::Int(n)) => Ok(Value::Int(n)),
            Some(Token::Float(n)) => Ok(Value::Float(n)),
            Some(Token::Bool(b)) => Ok(Value::Bool(b)),
            Some(Token::String(s)) => Ok(Value::String(s)),
            Some(Token::Ident(s)) => {
                // libconfig 中未引用的标识符视为字符串（常用于枚举值）。
                Ok(Value::String(s))
            }
            Some(token) => Err(ParseError::Unexpected {
                expected: "scalar".to_string(),
                found: format!("{:?}", token),
            }),
            None => Err(ParseError::Eof),
        }
    }
}

fn tokenize(source: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // 跳过空白
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // 行注释
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // 块注释
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        // 符号
        match c {
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
                continue;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
                continue;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
                continue;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
                continue;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
                continue;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
                continue;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
                continue;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
                continue;
            }
            ';' => {
                tokens.push(Token::Semi);
                i += 1;
                continue;
            }
            '@' => {
                tokens.push(Token::At);
                i += 1;
                continue;
            }
            _ => {}
        }

        // 字符串
        if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        other => s.push(other),
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= chars.len() {
                return Err(ParseError::UnterminatedString);
            }
            i += 1; // skip closing quote
            tokens.push(Token::String(s));
            continue;
        }

        // 多行字符串 <" ... ">
        if c == '<' && i + 1 < chars.len() && chars[i + 1] == '"' {
            i += 2;
            let mut s = String::new();
            while i + 1 < chars.len() && !(chars[i] == '"' && chars[i + 1] == '>') {
                s.push(chars[i]);
                i += 1;
            }
            if i + 1 >= chars.len() {
                return Err(ParseError::UnterminatedString);
            }
            i += 2; // skip ">
            tokens.push(Token::String(s));
            continue;
        }

        // 数字或 legacy libconfig 扩展标识符（如 1HSwords）
        if c.is_ascii_digit() || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            let mut is_float = false;
            if c == '-' {
                i += 1;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                if chars[i] == '.' {
                    is_float = true;
                }
                i += 1;
            }
            // legacy libconfig 允许以数字开头的标识符（如 1HSwords、2HSwords）
            if i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                tokens.push(Token::Ident(s));
            } else {
                let s: String = chars[start..i].iter().collect();
                if is_float {
                    let n = s
                        .parse::<f64>()
                        .map_err(|_| ParseError::InvalidNumber(s.clone()))?;
                    tokens.push(Token::Float(n));
                } else {
                    let n = s
                        .parse::<i64>()
                        .map_err(|_| ParseError::InvalidNumber(s.clone()))?;
                    tokens.push(Token::Int(n));
                }
            }
            continue;
        }

        // 标识符 / 关键字 / 布尔值
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if s == "true" {
                tokens.push(Token::Bool(true));
            } else if s == "false" {
                tokens.push(Token::Bool(false));
            } else {
                tokens.push(Token::Ident(s));
            }
            continue;
        }

        // 无法识别的字符
        return Err(ParseError::Unexpected {
            expected: "token".to_string(),
            found: format!("'{}'", c),
        });
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_item_db() {
        let source = r#"item_db: (
        {
            Id: 501
            AegisName: "Red_Potion"
            Name: "Red Potion"
            Buy: 50
            Weight: 70
        }
        )"#;
        let doc = parse(source).unwrap();
        let item_db = doc.get("item_db").unwrap();
        if let Value::Tuple(entries) = item_db {
            assert_eq!(entries.len(), 1);
            if let Value::Group(group) = &entries[0] {
                assert_eq!(group.get("Id").unwrap(), &Value::Int(501));
                assert_eq!(
                    group.get("AegisName").unwrap(),
                    &Value::String("Red_Potion".to_string())
                );
            } else {
                panic!("expected group");
            }
        } else {
            panic!("expected tuple");
        }
    }

    #[test]
    fn parse_multiline_string() {
        let source = r#"item_db: (
        {
            Id: 1
            Script: <"
                itemheal rand(45,65),0;
            ">
        }
        )"#;
        let doc = parse(source).unwrap();
        let group = match doc.get("item_db").unwrap() {
            Value::Tuple(v) => match &v[0] {
                Value::Group(g) => g.clone(),
                _ => panic!("expected group"),
            },
            _ => panic!("expected tuple"),
        };
        let script = group.get("Script").unwrap();
        assert!(matches!(script, Value::String(s) if s.contains("itemheal")));
    }

    #[test]
    fn parse_mob_with_nested_group() {
        let source = r#"mob_db: (
        {
            Id: 1001
            SpriteName: "SCORPION"
            Stats: {
                Str: 12
                Agi: 15
            }
            Drops: {
                Red_Potion: 70
            }
        }
        )"#;
        let doc = parse(source).unwrap();
        let group = match doc.get("mob_db").unwrap() {
            Value::Tuple(v) => match &v[0] {
                Value::Group(g) => g.clone(),
                _ => panic!("expected group"),
            },
            _ => panic!("expected tuple"),
        };
        assert_eq!(group.get("Id").unwrap(), &Value::Int(1001));
        if let Value::Group(stats) = group.get("Stats").unwrap() {
            assert_eq!(stats.get("Str").unwrap(), &Value::Int(12));
        } else {
            panic!("expected Stats group");
        }
    }
}
