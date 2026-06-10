//! Kociemba two-phase fast path for scramble→solved cases.
//!
//! Wraps the `min2phase` crate (cs0x7f's optimized two-phase). Native-only:
//! WASM builds do not include this module — wasm currently falls back to the
//! generic IDA* engine, which is acceptable because heavy bench cases are
//! driven by the Tauri-native side, not the in-renderer search.

use crate::facelet::{Facelet, COLOR_NAMES, FACELET_COUNT};
use crate::moves::parse_move_token;

/// Encode a facelet array as the URFDLB string min2phase expects.
fn facelet_to_min2phase_string(f: &Facelet) -> String {
    let mut s = String::with_capacity(FACELET_COUNT);
    for i in 0..FACELET_COUNT {
        let c = f[i] as usize;
        s.push_str(COLOR_NAMES[c]);
    }
    s
}

/// Try to solve `facelet → solved` using min2phase. Returns the solution as
/// a list of move indices (cube-core encoding), or `None` if min2phase
/// could not find a solution within `max_depth`.
///
/// `max_depth` should typically be 21 or 22. min2phase rejects requests
/// where `max_depth` exceeds its internal cap.
pub fn try_solve_full_state(facelet: &Facelet, max_depth: u8) -> Option<Vec<usize>> {
    let s = facelet_to_min2phase_string(facelet);
    let raw = min2phase::solve(&s, max_depth);
    // min2phase returns an empty string when no solution is found within
    // the depth bound, or when the input is already solved. Distinguish by
    // re-checking solved status.
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        // Either solved or unsolvable. Check if solved (all stickers match
        // their face color).
        if is_already_solved(facelet) {
            return Some(Vec::new());
        }
        return None;
    }
    // Parse moves. min2phase output uses spaces between move tokens.
    let mut moves: Vec<usize> = Vec::new();
    for tok in trimmed.split_whitespace() {
        match parse_move_token(tok) {
            Some(m) => moves.push(m),
            None => {
                // Unknown token from min2phase — bail out and let the
                // caller fall back to the generic engine.
                return None;
            }
        }
    }
    Some(moves)
}

fn is_already_solved(f: &Facelet) -> bool {
    for c in 0..6 {
        for i in 0..9 {
            if f[c * 9 + i] != c as i8 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facelet::create_solved_facelet;
    use crate::moves::{apply_moves, parse_move_token};

    fn scramble_to_moves(s: &str) -> Vec<usize> {
        s.split_whitespace()
            .filter_map(parse_move_token)
            .collect()
    }

    #[test]
    fn solves_simple_scramble() {
        let solved = create_solved_facelet();
        let scramble = scramble_to_moves("R U R' U' F2 R");
        let scrambled = apply_moves(&solved, &scramble);
        let solution = try_solve_full_state(&scrambled, 22).expect("min2phase failed");
        // Applying solution to scrambled should produce solved.
        let result = apply_moves(&scrambled, &solution);
        assert!(is_already_solved(&result));
    }

    #[test]
    fn solved_returns_empty() {
        let solved = create_solved_facelet();
        let solution = try_solve_full_state(&solved, 22).expect("must succeed for solved");
        assert!(solution.is_empty());
    }
}
