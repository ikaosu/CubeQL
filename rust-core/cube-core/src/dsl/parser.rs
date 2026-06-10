//! Recursive-descent parser for CubeQL.

use std::collections::HashSet;

use crate::moves::BASE_NAMES;

use super::lexer::{lex, line_column, Token, TokenKind};
use super::pattern::{BaseLimit, DslError, ParseError, Pattern, PatternElement};

fn make_error(src: &str, message: &str, offset: usize) -> ParseError {
    let (line, column) = line_column(src, offset);
    ParseError {
        message: message.to_string(),
        offset,
        line,
        column,
    }
}

fn require_base(tok: &Token, src: &str) -> Result<usize, DslError> {
    match BASE_NAMES.iter().position(|n| *n == tok.text) {
        Some(idx) => Ok(idx),
        None => Err(DslError(make_error(
            src,
            &format!("unknown move base \"{}\"", tok.text),
            tok.offset,
        ))),
    }
}

pub fn parse_pattern(src: &str) -> Result<Pattern, DslError> {
    let lex_out = lex(src);
    if let Some(err) = lex_out.error {
        return Err(DslError(make_error(src, &err, lex_out.error_offset.unwrap_or(0))));
    }
    let tokens = lex_out.tokens;
    // Slice raw token text from the lexer's normalized form (byte offsets refer
    // to it). Unicode primes have already been collapsed to ASCII so byte
    // indices land on char boundaries.
    let normalized = &lex_out.normalized;
    let mut i = 0usize;
    let mut elements: Vec<PatternElement> = Vec::new();
    let mut base_limits: Option<Vec<BaseLimit>> = None;

    macro_rules! peek {
        () => {
            &tokens[i]
        };
    }
    macro_rules! consume {
        () => {{
            let t = tokens[i].clone();
            i += 1;
            t
        }};
    }

    // Optional [BaseLimits] prefix
    if peek!().kind == TokenKind::Lbracket {
        let open_tok = consume!();
        let mut limits: Vec<BaseLimit> = Vec::new();
        let mut seen_keys: HashSet<u32> = HashSet::new();
        if peek!().kind == TokenKind::Rbracket {
            return Err(DslError(make_error(
                src,
                "empty base-limits block \"[]\"",
                open_tok.offset,
            )));
        }
        loop {
            let base_tok = consume!();
            if base_tok.kind != TokenKind::Ident {
                return Err(DslError(make_error(
                    src,
                    &format!(
                        "expected base identifier in \"[\", got \"{}\"",
                        base_tok.text
                    ),
                    base_tok.offset,
                )));
            }
            let base_idx = require_base(&base_tok, src)?;
            let mut variant: Option<usize> = Some(0);
            match peek!().kind {
                TokenKind::Prime => {
                    variant = Some(1);
                    consume!();
                }
                TokenKind::Two => {
                    variant = Some(2);
                    consume!();
                }
                TokenKind::Star => {
                    variant = None;
                    consume!();
                }
                _ => {}
            }
            let key = (base_idx as u32) * 4
                + match variant {
                    None => 0,
                    Some(v) => 1 + v as u32,
                };
            if seen_keys.contains(&key) {
                let display = format!(
                    "{}{}",
                    base_tok.text,
                    match variant {
                        Some(1) => "'",
                        Some(2) => "2",
                        None => "*",
                        _ => "",
                    }
                );
                return Err(DslError(make_error(
                    src,
                    &format!("duplicate limit \"{}\"", display),
                    base_tok.offset,
                )));
            }
            seen_keys.insert(key);
            let op_tok = consume!();
            if op_tok.kind != TokenKind::Le {
                return Err(DslError(make_error(
                    src,
                    &format!("expected \"<=\" after base name, got \"{}\"", op_tok.text),
                    op_tok.offset,
                )));
            }
            let num_tok = consume!();
            if num_tok.kind != TokenKind::Number && num_tok.kind != TokenKind::Two {
                return Err(DslError(make_error(
                    src,
                    &format!(
                        "expected non-negative integer after \"<=\", got \"{}\"",
                        num_tok.text
                    ),
                    num_tok.offset,
                )));
            }
            let max_n: i64 = num_tok.text.parse().map_err(|_| {
                DslError(make_error(
                    src,
                    &format!(
                        "base limit must be a non-negative integer, got \"{}\"",
                        num_tok.text
                    ),
                    num_tok.offset,
                ))
            })?;
            if max_n < 0 {
                return Err(DslError(make_error(
                    src,
                    &format!(
                        "base limit must be a non-negative integer, got \"{}\"",
                        num_tok.text
                    ),
                    num_tok.offset,
                )));
            }
            limits.push(BaseLimit {
                base: base_idx,
                variant,
                max: max_n as usize,
            });
            match peek!().kind {
                TokenKind::Comma => {
                    consume!();
                    continue;
                }
                TokenKind::Rbracket => {
                    consume!();
                    break;
                }
                _ => {
                    let sep = consume!();
                    return Err(DslError(make_error(
                        src,
                        &format!(
                            "expected \",\" or \"]\" in base-limits, got \"{}\"",
                            sep.text
                        ),
                        sep.offset,
                    )));
                }
            }
        }
        base_limits = Some(limits);
    }

    while peek!().kind != TokenKind::End {
        let tok_kind = peek!().kind;
        if tok_kind == TokenKind::Lt {
            let start_tok = consume!();
            let start = start_tok.offset;
            let mut bases: Vec<usize> = Vec::new();
            let first = consume!();
            if first.kind != TokenKind::Ident {
                return Err(DslError(make_error(
                    src,
                    &format!(
                        "expected face/wide/slice identifier after \"<\", got \"{}\"",
                        first.text
                    ),
                    first.offset,
                )));
            }
            bases.push(require_base(&first, src)?);
            while peek!().kind == TokenKind::Comma {
                consume!();
                let next = consume!();
                if next.kind != TokenKind::Ident {
                    return Err(DslError(make_error(
                        src,
                        &format!("expected identifier after \",\", got \"{}\"", next.text),
                        next.offset,
                    )));
                }
                bases.push(require_base(&next, src)?);
            }
            let close = consume!();
            if close.kind != TokenKind::Gt {
                return Err(DslError(make_error(
                    src,
                    &format!("expected \">\", got \"{}\"", close.text),
                    close.offset,
                )));
            }
            let end = close.offset + 1;
            let mut min = 0usize;
            let mut max: Option<usize> = None;
            let mut end_offset = end;
            if peek!().kind == TokenKind::Lbrace {
                let brace_open = consume!();
                let n1 = consume!();
                if n1.kind != TokenKind::Number && n1.kind != TokenKind::Two {
                    return Err(DslError(make_error(
                        src,
                        &format!("expected number in quantifier, got \"{}\"", n1.text),
                        n1.offset,
                    )));
                }
                let lo: usize = n1.text.parse().unwrap_or(0);
                let next = consume!();
                if next.kind == TokenKind::Rbrace {
                    min = lo;
                    max = Some(lo);
                    end_offset = next.offset + 1;
                } else if next.kind == TokenKind::Comma {
                    let n2 = consume!();
                    if n2.kind == TokenKind::Rbrace {
                        min = lo;
                        max = None;
                        end_offset = n2.offset + 1;
                    } else if n2.kind == TokenKind::Number || n2.kind == TokenKind::Two {
                        let hi: usize = n2.text.parse().unwrap_or(0);
                        let close_brace = consume!();
                        if close_brace.kind != TokenKind::Rbrace {
                            return Err(DslError(make_error(
                                src,
                                &format!("expected \"}}\", got \"{}\"", close_brace.text),
                                close_brace.offset,
                            )));
                        }
                        min = lo;
                        max = Some(hi);
                        end_offset = close_brace.offset + 1;
                        if hi < lo {
                            return Err(DslError(make_error(
                                src,
                                &format!(
                                    "quantifier max ({}) must be >= min ({})",
                                    hi, lo
                                ),
                                brace_open.offset,
                            )));
                        }
                    } else {
                        return Err(DslError(make_error(
                            src,
                            &format!(
                                "expected number or \"}}\" after \",\", got \"{}\"",
                                n2.text
                            ),
                            n2.offset,
                        )));
                    }
                } else {
                    return Err(DslError(make_error(
                        src,
                        &format!(
                            "expected \",\" or \"}}\" in quantifier, got \"{}\"",
                            next.text
                        ),
                        next.offset,
                    )));
                }
            }
            elements.push(PatternElement::Set {
                allowed_bases: bases,
                min,
                max,
                raw: normalized[start..end_offset].to_string(),
            });
        } else if tok_kind == TokenKind::Ident {
            let tok = consume!();
            let start = tok.offset;
            let base_idx = require_base(&tok, src)?;
            let mut variant = 0usize;
            let mut end_offset = start + tok.text.len();
            match peek!().kind {
                TokenKind::Prime => {
                    variant = 1;
                    let p = consume!();
                    end_offset = p.offset + 1;
                }
                TokenKind::Two => {
                    variant = 2;
                    let p = consume!();
                    end_offset = p.offset + 1;
                }
                _ => {}
            }
            elements.push(PatternElement::Literal {
                move_idx: base_idx * 3 + variant,
                raw: normalized[start..end_offset].to_string(),
            });
        } else {
            let tok = consume!();
            return Err(DslError(make_error(
                src,
                &format!("unexpected token \"{}\"", tok.text),
                tok.offset,
            )));
        }
    }

    Ok(Pattern {
        elements,
        source: src.to_string(),
        base_limits,
    })
}

/// Expand a set's allowed bases into all 3 variant moves per base, sorted.
pub fn expand_set_moves(allowed_bases: &[usize]) -> Vec<usize> {
    let mut moves: Vec<usize> = Vec::with_capacity(allowed_bases.len() * 3);
    for &b in allowed_bases {
        moves.push(b * 3);
        moves.push(b * 3 + 1);
        moves.push(b * 3 + 2);
    }
    moves.sort_unstable();
    moves
}
