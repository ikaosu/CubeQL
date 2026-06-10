//! Sanity check that DSL accepts x/y/z (lowercase) bases.

use cube_core::dsl::parser::parse_pattern;

#[test]
fn parses_y_literal() {
    parse_pattern("y").expect("y literal");
}

#[test]
fn parses_y_set() {
    parse_pattern("<y>").expect("<y>");
}

#[test]
fn parses_rotation_set_quantifier() {
    parse_pattern("<R, U, F, x, y, z>{1,4}").expect("mixed rotation set");
}

#[test]
fn parses_all_rotation_literals() {
    for s in ["x", "x'", "x2", "y", "y'", "y2", "z", "z'", "z2"] {
        parse_pattern(s).unwrap_or_else(|e| panic!("{}: {}", s, e));
    }
}

#[test]
fn parses_rotation_then_face_sequence() {
    parse_pattern("y R U R' U' y'").expect("rotation interleaved");
}
