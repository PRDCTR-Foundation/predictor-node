use crate::apis;
use sp_runtime::create_runtime_str;
use sp_version::{self, RuntimeVersion};

// `runtime/build.rs` fills in `runtime_version.rs.template`'s
// {{SPEC_VERSION}} placeholder from this crate's own Cargo.toml version and
// writes the result to OUT_DIR, so spec_version can never drift from the
// version release-please manages. It has to be spliced in with an
// item-level `include!` (rather than this file referencing a generated
// `const`) because `sp_version::runtime_version` only accepts a literal
// integer for spec_version.
// To learn more about runtime versioning, see:
// https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
include!(concat!(env!("OUT_DIR"), "/runtime_version.rs"));
