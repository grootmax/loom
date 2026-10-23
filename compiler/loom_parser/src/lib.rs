//! Hand-written recursive-descent parser with error recovery, producing the CST.
//!
//! Status: stub (Phase 0). See `docs/roadmap.md` for when this crate is implemented.

/// The name of this crate, used by the driver for diagnostics about the pipeline.
pub const CRATE_NAME: &str = "loom_parser";

#[cfg(test)]
mod tests {
    use super::CRATE_NAME;

    #[test]
    fn crate_name_matches_package() {
        assert_eq!(CRATE_NAME, env!("CARGO_PKG_NAME"));
    }
}
