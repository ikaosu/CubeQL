//! Pruning-table smoke test. Gated behind GENCUBE_BUILD_PRUNING=1 because the
//! initial build is several minutes (4-table 45-move BFS).
//!
//! Run with:
//!   $env:GENCUBE_BUILD_PRUNING = "1"; cargo test --release --test pruning_smoke -- --nocapture

use cube_core::coord::{
    CANONICAL_SLICE, NUM_CO_COORDS, NUM_CP_COORDS, NUM_EO_COORDS, NUM_SLICE_COORDS,
};
use cube_core::pruning::{get_pruning_tables, get_pruning_tables_with_progress};

#[test]
fn pruning_tables_load_or_build() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipping: set GENCUBE_BUILD_PRUNING=1 to enable");
        return;
    }
    let tables = get_pruning_tables_with_progress(|e| eprintln!("{:?}", e));
    assert_eq!(tables.co_slice.len(), NUM_CO_COORDS * NUM_SLICE_COORDS);
    assert_eq!(tables.eo_slice.len(), NUM_EO_COORDS * NUM_SLICE_COORDS);
    assert_eq!(tables.co_eo.len(), NUM_CO_COORDS * NUM_EO_COORDS);
    assert_eq!(tables.cp_co.len(), NUM_CP_COORDS * NUM_CO_COORDS);
    // Origin distance must be zero for each table.
    assert_eq!(
        tables.co_slice[0 * NUM_SLICE_COORDS + CANONICAL_SLICE as usize],
        0
    );
    assert_eq!(
        tables.eo_slice[0 * NUM_SLICE_COORDS + CANONICAL_SLICE as usize],
        0
    );
    assert_eq!(tables.co_eo[0], 0);
    assert_eq!(tables.cp_co[0], 0);
    // Second call must return cached singleton.
    let _ = get_pruning_tables();
}
