//! Pattern IR for the CubeQL DSL.

use std::fmt;

#[derive(Debug, Clone)]
pub enum PatternElement {
    Literal {
        move_idx: usize,
        raw: String,
    },
    Set {
        allowed_bases: Vec<usize>,
        min: usize,
        /// `None` means unbounded (∞).
        max: Option<usize>,
        raw: String,
    },
}

/// Cap on the total occurrences of a move (variant or base-wide).
/// `variant = None` → base-wide (all 3 variants combined).
#[derive(Debug, Clone, Copy)]
pub struct BaseLimit {
    pub base: usize,
    pub variant: Option<usize>,
    pub max: usize,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub elements: Vec<PatternElement>,
    pub source: String,
    pub base_limits: Option<Vec<BaseLimit>>,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct DslError(pub ParseError);

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (line {}, col {})",
            self.0.message, self.0.line, self.0.column
        )
    }
}

impl std::error::Error for DslError {}
