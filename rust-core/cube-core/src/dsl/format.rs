//! Render CubeQL IR back to source form (for previews / round-trip tests).

use crate::moves::{move_base, move_name, move_variant, BASE_NAMES, VARIANT_SUFFIX};

use super::pattern::{Pattern, PatternElement};

pub fn format_element(el: &PatternElement) -> String {
    match el {
        PatternElement::Literal { move_idx, .. } => move_name(*move_idx),
        PatternElement::Set {
            allowed_bases, min, max, ..
        } => {
            let set = format!(
                "<{}>",
                allowed_bases
                    .iter()
                    .map(|&b| BASE_NAMES[b])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let q = match (min, max) {
                (0, None) => String::new(),
                (m, Some(mx)) if m == mx => format!("{{{}}}", m),
                (m, None) => format!("{{{},}}", m),
                (m, Some(mx)) => format!("{{{},{}}}", m, mx),
            };
            format!("{}{}", set, q)
        }
    }
}

pub fn format_pattern(p: &Pattern) -> String {
    p.elements
        .iter()
        .map(format_element)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn format_move_sequence(moves: &[usize]) -> String {
    moves
        .iter()
        .map(|&m| format!("{}{}", BASE_NAMES[move_base(m)], VARIANT_SUFFIX[move_variant(m)]))
        .collect::<Vec<_>>()
        .join(" ")
}
