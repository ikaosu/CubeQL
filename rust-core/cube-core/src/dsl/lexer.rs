//! Lexer for CubeQL. Mirrors `core/dsl/lexer.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Lt,
    Gt,
    Comma,
    Lbrace,
    Rbrace,
    Lbracket,
    Rbracket,
    Le,
    Star,
    Number,
    Ident,
    Prime,
    Two,
    End,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub offset: usize,
}

pub struct LexOutcome {
    pub tokens: Vec<Token>,
    /// The input text after unicode-prime normalization. Token `offset`s are
    /// byte indices into this string.
    pub normalized: String,
    pub error: Option<String>,
    pub error_offset: Option<usize>,
}

fn normalize(src: &str) -> String {
    src.chars()
        .map(|c| match c {
            '\u{2019}' | '\u{02BC}' | '\u{02B9}' | '\u{2032}' => '\'',
            other => other,
        })
        .collect()
}

fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

pub fn lex(src: &str) -> LexOutcome {
    let text = normalize(src);
    let bytes = text.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            i += 1;
            continue;
        }
        if c == '<' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                tokens.push(Token { kind: TokenKind::Le, text: "<=".to_string(), offset: i });
                i += 2;
                continue;
            }
            tokens.push(Token { kind: TokenKind::Lt, text: "<".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '>' {
            tokens.push(Token { kind: TokenKind::Gt, text: ">".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '[' {
            tokens.push(Token { kind: TokenKind::Lbracket, text: "[".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == ']' {
            tokens.push(Token { kind: TokenKind::Rbracket, text: "]".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '*' {
            tokens.push(Token { kind: TokenKind::Star, text: "*".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == ',' {
            tokens.push(Token { kind: TokenKind::Comma, text: ",".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '{' {
            tokens.push(Token { kind: TokenKind::Lbrace, text: "{".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '}' {
            tokens.push(Token { kind: TokenKind::Rbrace, text: "}".to_string(), offset: i });
            i += 1;
            continue;
        }
        if c == '\'' {
            tokens.push(Token { kind: TokenKind::Prime, text: "'".to_string(), offset: i });
            i += 1;
            continue;
        }
        if is_digit(c) {
            let start = i;
            while i < bytes.len() && is_digit(bytes[i] as char) {
                i += 1;
            }
            let num = &text[start..i];
            if num == "2" {
                tokens.push(Token { kind: TokenKind::Two, text: "2".to_string(), offset: start });
            } else {
                tokens.push(Token {
                    kind: TokenKind::Number,
                    text: num.to_string(),
                    offset: start,
                });
            }
            continue;
        }
        if is_letter(c) {
            let start = i;
            let base_len = if i + 1 < bytes.len() && bytes[i + 1] == b'w' { 2 } else { 1 };
            i += base_len;
            tokens.push(Token {
                kind: TokenKind::Ident,
                text: text[start..i].to_string(),
                offset: start,
            });
            continue;
        }
        return LexOutcome {
            tokens,
            normalized: text,
            error: Some(format!("unexpected character \"{}\"", c)),
            error_offset: Some(i),
        };
    }
    tokens.push(Token { kind: TokenKind::End, text: String::new(), offset: bytes.len() });
    LexOutcome { tokens, normalized: text, error: None, error_offset: None }
}

pub fn line_column(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    let bytes = src.as_bytes();
    let lim = offset.min(bytes.len());
    for i in 0..lim {
        if bytes[i] == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}
