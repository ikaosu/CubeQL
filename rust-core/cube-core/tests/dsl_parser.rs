use cube_core::dsl::format::format_pattern;
use cube_core::dsl::parser::parse_pattern;
use cube_core::dsl::pattern::PatternElement;

#[test]
fn literal_moves() {
    let p = parse_pattern("R U R' U'").expect("parse");
    assert_eq!(p.elements.len(), 4);
    if let PatternElement::Literal { move_idx, .. } = &p.elements[0] {
        assert_eq!(*move_idx, 2 * 3); // R
    } else {
        panic!("expected literal");
    }
    if let PatternElement::Literal { move_idx, .. } = &p.elements[1] {
        assert_eq!(*move_idx, 0 * 3); // U
    } else {
        panic!("expected literal");
    }
}

#[test]
fn set_with_range_quantifier() {
    let p = parse_pattern("<R, U>{1,7}").expect("parse");
    match &p.elements[0] {
        PatternElement::Set {
            allowed_bases, min, max, ..
        } => {
            assert_eq!(allowed_bases, &vec![2, 0]); // R, U
            assert_eq!(*min, 1);
            assert_eq!(*max, Some(7));
        }
        _ => panic!("expected set"),
    }
}

#[test]
fn set_with_min_only_quantifier() {
    let p = parse_pattern("<F>{3,}").expect("parse");
    match &p.elements[0] {
        PatternElement::Set { min, max, .. } => {
            assert_eq!(*min, 3);
            assert_eq!(*max, None);
        }
        _ => panic!("expected set"),
    }
}

#[test]
fn rejects_quantifier_with_swapped_bounds() {
    let err = parse_pattern("<R>{5,3}").err().expect("parse error");
    assert!(err.0.message.contains("max"), "msg = {}", err.0.message);
}

#[test]
fn base_limit_cw_only() {
    let p = parse_pattern("[F<=3] <R, U, F>").expect("parse");
    let limits = p.base_limits.expect("limits");
    assert_eq!(limits.len(), 1);
    assert_eq!(limits[0].base, 4); // F
    assert_eq!(limits[0].variant, Some(0)); // CW only
    assert_eq!(limits[0].max, 3);
}

#[test]
fn base_limit_wildcard_star() {
    let p = parse_pattern("[F*<=3] <R, U, F>").expect("parse");
    let limits = p.base_limits.expect("limits");
    assert_eq!(limits[0].variant, None);
}

#[test]
fn base_limit_duplicate_is_error() {
    let err = parse_pattern("[F<=3, F<=2] <R, U, F>").err().expect("parse error");
    assert!(err.0.message.contains("duplicate"), "msg = {}", err.0.message);
}

#[test]
fn base_limit_starwide_and_specific_can_coexist() {
    let p = parse_pattern("[F*<=4, F2<=0] <R, U, F>").expect("parse");
    let limits = p.base_limits.expect("limits");
    assert_eq!(limits.len(), 2);
}

#[test]
fn unknown_base_is_error() {
    let err = parse_pattern("X").err().expect("parse error");
    assert!(err.0.message.contains("unknown move base"), "msg = {}", err.0.message);
}

#[test]
fn format_pattern_roundtrip() {
    let p = parse_pattern("<R, U>{1,7} F'").expect("parse");
    let s = format_pattern(&p);
    // exact spacing matches our formatter
    assert_eq!(s, "<R, U>{1,7} F'");
}

#[test]
fn unicode_prime_normalizes() {
    let p = parse_pattern("R\u{2019}").expect("parse");
    match &p.elements[0] {
        PatternElement::Literal { move_idx, .. } => assert_eq!(*move_idx, 2 * 3 + 1),
        _ => panic!("expected literal"),
    }
}
