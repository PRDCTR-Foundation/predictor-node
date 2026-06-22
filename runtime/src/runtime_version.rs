use crate::apis;
use sp_runtime::create_runtime_str;
use sp_version::{self, RuntimeVersion};

// To learn more about runtime versioning, see:
// https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name: create_runtime_str!("predictor-runtime"),
    impl_name: create_runtime_str!("predictor-runtime"),
    authoring_version: 1,
    spec_version: 01_00_02_00,
    impl_version: 1,
    apis: apis::RUNTIME_API_VERSIONS,
    transaction_version: 1,
    state_version: 1,
};
