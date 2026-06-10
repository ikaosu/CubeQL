//! Mathematical properties of all 54 moves (45 face/wide/slice + 9 rotations).

use cube_core::facelet::{create_solved_facelet, facelets_equal, FACELET_COUNT};
use cube_core::moves::{
    apply_move, apply_moves, inverse_move, is_rotation_base, is_rotation_move, move_axis,
    move_base, move_name, move_perm, move_variant, parse_move_token, Base, BASE_NAMES,
    MOVE_COUNT,
};

fn assert_eq_state(a: &[i8; FACELET_COUNT], b: &[i8; FACELET_COUNT]) {
    if !facelets_equal(a, b) {
        for i in 0..FACELET_COUNT {
            if a[i] != b[i] {
                panic!("state diff at {}: {} vs {}", i, a[i], b[i]);
            }
        }
    }
}

#[test]
fn cw_then_ccw_is_identity() {
    let solved = create_solved_facelet();
    for b in 0..18 {
        let cw = b * 3;
        let ccw = b * 3 + 1;
        let after = apply_moves(&solved, &[cw, ccw]);
        assert_eq_state(&solved, &after);
    }
}

#[test]
fn double_180_is_identity() {
    let solved = create_solved_facelet();
    for b in 0..18 {
        let m = b * 3 + 2;
        let after = apply_moves(&solved, &[m, m]);
        assert_eq_state(&solved, &after);
    }
}

#[test]
fn cw_squared_equals_180() {
    let solved = create_solved_facelet();
    for b in 0..18 {
        let cw = b * 3;
        let m180 = b * 3 + 2;
        let a = apply_moves(&solved, &[cw, cw]);
        let b_ = apply_moves(&solved, &[m180]);
        assert_eq_state(&a, &b_);
    }
}

#[test]
fn cw_to_power_4_is_identity() {
    let solved = create_solved_facelet();
    for b in 0..18 {
        let cw = b * 3;
        let after = apply_moves(&solved, &[cw, cw, cw, cw]);
        assert_eq_state(&solved, &after);
    }
}

#[test]
fn move_count_and_base_axis_consistency() {
    assert_eq!(MOVE_COUNT, 54);
    assert_eq!(BASE_NAMES.len(), 18);
    for m in 0..MOVE_COUNT {
        let _ = move_axis(m);
        let _ = move_name(m);
        assert!(move_base(m) < 18);
        assert!(move_variant(m) < 3);
    }
}

#[test]
fn parse_move_token_roundtrip() {
    for m in 0..MOVE_COUNT {
        let s = move_name(m);
        let parsed = parse_move_token(&s);
        assert_eq!(parsed, Some(m), "round-trip failed for {}", s);
    }
}

#[test]
fn inverse_move_is_inverse() {
    let solved = create_solved_facelet();
    for m in 0..MOVE_COUNT {
        let inv = inverse_move(m);
        let after = apply_moves(&solved, &[m, inv]);
        assert_eq_state(&solved, &after);
    }
}

#[test]
fn rw_equals_r_plus_m_inverse() {
    // Rw = R · M' as composed permutations.
    let solved = create_solved_facelet();
    let perms = move_perm();
    // Apply Rw and the composed (R + M') sequence — should produce same facelet.
    let after_rw = apply_move(&solved, 8 * 3); // Rw CW
    let after_r_then_mi = apply_moves(&solved, &[2 * 3, 12 * 3 + 1]); // R then M'
    assert_eq_state(&after_rw, &after_r_then_mi);
    let _ = perms;
}

#[test]
fn x_equals_rw_plus_l_inverse() {
    // x = Rw · L' as composed permutations.
    let solved = create_solved_facelet();
    let after_x = apply_move(&solved, Base::X * 3); // x CW
    let after_rw_then_li = apply_moves(&solved, &[Base::Rw * 3, Base::L * 3 + 1]);
    assert_eq_state(&after_x, &after_rw_then_li);
}

#[test]
fn y_equals_uw_plus_d_inverse() {
    let solved = create_solved_facelet();
    let after_y = apply_move(&solved, Base::Y * 3);
    let after_uw_then_di = apply_moves(&solved, &[Base::Uw * 3, Base::D * 3 + 1]);
    assert_eq_state(&after_y, &after_uw_then_di);
}

#[test]
fn z_equals_fw_plus_b_inverse() {
    let solved = create_solved_facelet();
    let after_z = apply_move(&solved, Base::Z * 3);
    let after_fw_then_bi = apply_moves(&solved, &[Base::Fw * 3, Base::B * 3 + 1]);
    assert_eq_state(&after_z, &after_fw_then_bi);
}

#[test]
fn rotation_classification() {
    for b in 0..15 {
        assert!(!is_rotation_base(b), "base {} should not be rotation", b);
    }
    for b in 15..18 {
        assert!(is_rotation_base(b), "base {} should be rotation", b);
    }
    for m in 0..45 {
        assert!(!is_rotation_move(m));
    }
    for m in 45..54 {
        assert!(is_rotation_move(m));
    }
}

#[test]
fn parse_rotation_tokens() {
    assert_eq!(parse_move_token("x"), Some(Base::X * 3));
    assert_eq!(parse_move_token("x'"), Some(Base::X * 3 + 1));
    assert_eq!(parse_move_token("x2"), Some(Base::X * 3 + 2));
    assert_eq!(parse_move_token("y"), Some(Base::Y * 3));
    assert_eq!(parse_move_token("z"), Some(Base::Z * 3));
    // Unicode prime
    assert_eq!(parse_move_token("x\u{2019}"), Some(Base::X * 3 + 1));
}
