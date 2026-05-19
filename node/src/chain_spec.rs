use predictor_runtime::WASM_BINARY;
use sc_service::ChainType;

// The URL for the telemetry server.
// const STAGING_TELEMETRY_URL: &str = "wss://telemetry.polkadot.io/submit/";

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

/// Sets properties for PRD based chain
pub(crate) fn prd_chain_properties() -> sc_chain_spec::Properties {
    // Give your base currency a unit name and decimal places
    let mut properties = sc_chain_spec::Properties::new();
    properties.insert("tokenSymbol".into(), "PRD".into());
    properties.insert("tokenDecimals".into(), 18.into());
    // TODO: Determine which address prefix to use. Using default for now
    properties.insert("ss58Format".into(), 42.into());
    return properties
}

pub fn development_config() -> Result<ChainSpec, String> {
    let properties = prd_chain_properties();

    Ok(ChainSpec::builder(
        WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
        None,
    )
    .with_name("Predictor Development")
    .with_id("dev")
    .with_protocol_id("prd-dev")
    .with_properties(properties)
    .with_chain_type(ChainType::Development)
    .with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
    .build())
}

pub fn local_testnet_config() -> Result<ChainSpec, String> {
    let properties = prd_chain_properties();

    Ok(ChainSpec::builder(WASM_BINARY.ok_or_else(|| "Local wasm not available".to_string())?, None)
        .with_name("Predictor Local Testnet")
        .with_id("local_testnet")
        .with_protocol_id("prd-local-testnet")
        .with_properties(properties)
        .with_chain_type(ChainType::Local)
        .with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
        .build())
}

pub fn staging_testnet_config() -> Result<ChainSpec, String> {
    let properties = prd_chain_properties();

    Ok(ChainSpec::builder(
        WASM_BINARY.ok_or_else(|| "Staging wasm not available".to_string())?,
        None,
    )
    .with_name("Predictor Staging Testnet")
    .with_id("predictor_staging_testnet")
    .with_protocol_id("prd-staging-testnet")
    .with_properties(properties)
    .with_chain_type(ChainType::Live)
    .with_genesis_config_preset_name(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET)
    .build())
}
