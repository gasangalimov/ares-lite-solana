// Placeholder constants so the crate builds standalone. Season builds replace
// this file with constants derived from the season challenge
// (`lite/cli/ares_lite.py open` writes <workdir>/season.rs).
pub const HOP_LIMIT: u64 = 3;
pub const RESOURCE_BUDGET: u64 = 24;
pub const REQUIRED_MASK: u64 = 2;
pub const FORBIDDEN_MASK: u64 = 4;
