use cube_core::presets::mask_from_preset_id;

#[test]
fn cmll_hides_udfb_centers_keeps_rl() {
    let m = mask_from_preset_id("CMLL").expect("CMLL preset");
    // U / D / F / B centers must be don't-care so the cube can be held with
    // any rotation around the L-R axis without breaking the goal match.
    assert_eq!(m[4], 0, "U center should be masked");
    assert_eq!(m[31], 0, "D center should be masked");
    assert_eq!(m[22], 0, "F center should be masked");
    assert_eq!(m[49], 0, "B center should be masked");
    // R / L centers stay visible (they are part of Roux's left/right blocks).
    assert_eq!(m[13], 1, "R center should be visible");
    assert_eq!(m[40], 1, "L center should be visible");

    let faces = [("U", 0..9), ("R", 9..18), ("F", 18..27), ("D", 27..36), ("L", 36..45), ("B", 45..54)];
    for (name, range) in faces {
        let s: String = range.map(|i| if m[i] == 1 { '#' } else { '.' }).collect();
        eprintln!("{}: {}", name, s);
    }
}
