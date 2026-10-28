//! In-house snapshot testing helper (dev-dependency only).
//!
//! Status: stub (Phase 1). See ADR 0018 §8 for the planned behavior.

/// The name of this crate, used by the driver for diagnostics about the pipeline.
pub const CRATE_NAME: &str = "loom_snapshot";

#[cfg(test)]
mod tests {
    use super::CRATE_NAME;

    #[test]
    fn crate_name_matches_package() {
        assert_eq!(CRATE_NAME, env!("CARGO_PKG_NAME"));
    }
}
