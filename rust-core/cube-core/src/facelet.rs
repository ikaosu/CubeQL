//! Facelet representation of a 3x3 Rubik's cube.
//!
//! 54 stickers in Kociemba layout:
//!   U: 0..8    R: 9..17    F: 18..26
//!   D: 27..35  L: 36..44   B: 45..53
//!
//! Colors: 0=U(white), 1=R(red), 2=F(green), 3=D(yellow), 4=L(orange), 5=B(blue)

pub const FACELET_COUNT: usize = 54;

#[allow(non_snake_case)]
pub mod Color {
    pub const U: i8 = 0;
    pub const R: i8 = 1;
    pub const F: i8 = 2;
    pub const D: i8 = 3;
    pub const L: i8 = 4;
    pub const B: i8 = 5;
}

pub const COLOR_NAMES: [&str; 6] = ["U", "R", "F", "D", "L", "B"];

pub const CENTER_FACELETS: [usize; 6] = [4, 13, 22, 31, 40, 49];

/// A facelet state is 54 color codes (0..5).
pub type Facelet = [i8; FACELET_COUNT];

pub fn create_solved_facelet() -> Facelet {
    let mut f = [0i8; FACELET_COUNT];
    for c in 0..6 {
        for i in 0..9 {
            f[c * 9 + i] = c as i8;
        }
    }
    f
}

pub fn clone_facelet(f: &Facelet) -> Facelet {
    *f
}

pub fn facelets_equal(a: &Facelet, b: &Facelet) -> bool {
    a == b
}

/// Validate that a facelet state could be physically possible. If `mask` is
/// provided, only positions where `mask[i] == 1` are checked; masked-off
/// positions are treated as wildcards.
pub fn validate_facelet(f: &Facelet, mask: Option<&[u8; FACELET_COUNT]>) -> Option<String> {
    let mut counts = [0usize; 6];
    let mut visible = 0usize;
    for i in 0..FACELET_COUNT {
        if let Some(m) = mask {
            if m[i] == 0 {
                continue;
            }
        }
        let c = f[i];
        if !(0..=5).contains(&c) {
            return Some(format!("facelet[{}] = {} out of range", i, c));
        }
        counts[c as usize] += 1;
        visible += 1;
    }
    let wildcard_slots = FACELET_COUNT - visible;
    for c in 0..6 {
        if counts[c] > 9 {
            return Some(format!(
                "color {} appears {} times (max 9)",
                COLOR_NAMES[c], counts[c]
            ));
        }
        if counts[c] + wildcard_slots < 9 {
            return Some(format!(
                "color {} appears {} times; cannot reach 9 with {} masked slots",
                COLOR_NAMES[c], counts[c], wildcard_slots
            ));
        }
    }
    for c in 0..6 {
        let ci = CENTER_FACELETS[c];
        if let Some(m) = mask {
            if m[ci] == 0 {
                continue;
            }
        }
        if f[ci] != c as i8 {
            return Some(format!(
                "center {} has color {} (expected {})",
                COLOR_NAMES[c], COLOR_NAMES[f[ci] as usize], COLOR_NAMES[c]
            ));
        }
    }
    None
}
