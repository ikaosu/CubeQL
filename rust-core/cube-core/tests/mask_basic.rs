use cube_core::facelet::{create_solved_facelet, FACELET_COUNT};
use cube_core::mask::{
    bake_in_mask, create_empty_mask, create_full_mask, default_goal, is_goal, mask_from_indices,
    WILDCARD,
};
use cube_core::moves::{apply_moves, BASE_NAMES};

#[test]
fn empty_mask_makes_all_wildcards() {
    let solved = create_solved_facelet();
    let mask = create_empty_mask();
    let eff = bake_in_mask(&solved, &mask);
    for i in 0..FACELET_COUNT {
        assert_eq!(eff[i], WILDCARD);
    }
}

#[test]
fn full_mask_preserves_state() {
    let solved = create_solved_facelet();
    let mask = create_full_mask();
    let eff = bake_in_mask(&solved, &mask);
    for i in 0..FACELET_COUNT {
        assert_eq!(eff[i], solved[i]);
    }
}

#[test]
fn solved_state_is_goal_under_full_mask() {
    let solved = create_solved_facelet();
    let goal = default_goal();
    let eff = bake_in_mask(&solved, &create_full_mask());
    assert!(is_goal(&eff, &goal));
}

#[test]
fn after_r_solved_no_longer_matches_goal_under_full_mask() {
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &[2 * 3]); // R
    let goal = default_goal();
    let eff = bake_in_mask(&scrambled, &create_full_mask());
    assert!(!is_goal(&eff, &goal));
}

#[test]
fn d_layer_mask_ignores_u_changes() {
    let solved = create_solved_facelet();
    let d_visible: Vec<usize> = (27..54).collect(); // D face + L + B; just for sanity
    let mask = mask_from_indices(&d_visible);
    let after_u = apply_moves(&solved, &[0]); // U CW
    let goal = default_goal();
    let eff = bake_in_mask(&after_u, &mask);
    let _ = &BASE_NAMES;
    // The mask deliberately covers the D face entirely (27..36). U doesn't touch those.
    // Other parts of the mask (36..54) include L/B which DO change under U. We
    // expect is_goal to be false because of those.
    assert!(!is_goal(&eff, &goal));
}
