//! CubeQL — domain-specific language for describing move patterns.
//!
//! Grammar overview (see CubeQL.md for the full reference):
//!   Pattern     = [BaseLimits] Token+
//!   BaseLimits  = "[" BaseLimit ("," BaseLimit)* "]"
//!   BaseLimit   = MoveBase [ "'" | "2" | "*" ] "<=" Number
//!   Token       = LiteralMove | MoveSet [Quantifier]
//!   LiteralMove = MoveBase [ "'" | "2" ]
//!   MoveSet     = "<" MoveBase ("," MoveBase)* ">"
//!   Quantifier  = "{" N "}" | "{" m "," M "}" | "{" m "," "}"

pub mod format;
pub mod lexer;
pub mod parser;
pub mod pattern;

pub use format::{format_element, format_move_sequence, format_pattern};
pub use lexer::{lex, line_column, LexOutcome, Token, TokenKind};
pub use parser::{expand_set_moves, parse_pattern};
pub use pattern::{BaseLimit, DslError, ParseError, Pattern, PatternElement};
