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
    properties.insert("tokenDecimals".into(), 10.into());
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
    .with_name("PRDCTR Development")
    .with_id("prdctr-dev")
    .with_protocol_id("prdctr-dev")
    .with_properties(properties)
    .with_chain_type(ChainType::Development)
    .with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
    .build())
}

pub fn local_testnet_config() -> Result<ChainSpec, String> {
    let properties = prd_chain_properties();

    Ok(ChainSpec::builder(WASM_BINARY.ok_or_else(|| "Local wasm not available".to_string())?, None)
        .with_name("PRDCTR Local Testnet")
        .with_id("prdctr_local_testnet")
        .with_protocol_id("prdctr-local-testnet")
        .with_properties(properties)
        .with_chain_type(ChainType::Local)
        .with_genesis_config_preset_name(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET)
        .build())
}

pub fn staging_testnet_config() -> Result<ChainSpec, String> {
    let properties = prd_chain_properties();

    Ok(ChainSpec::builder(
        WASM_BINARY.ok_or_else(|| "Staging wasm not available".to_string())?,
        None,
    )
    .with_name("PRDCTR Staging Testnet")
    .with_id("prdctr_staging_testnet")
    .with_protocol_id("prdctr-staging-testnet")
    .with_properties(properties)
    .with_chain_type(ChainType::Live)
    .with_genesis_config_preset_name(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET)
    .build())
}

pub fn public_testnet_config() -> Result<ChainSpec, String> {
    #[cfg(feature = "enable-static-presents")]
    {
        let properties = prd_chain_properties();

        Ok(ChainSpec::builder(
            WASM_BINARY.ok_or_else(|| "Public testnet wasm not available".to_string())?,
            None,
        )
        .with_name("Cassandra - PRDCTR Public Testnet")
        .with_id("predictor_cassandra_public_testnet_v2")
        .with_protocol_id("prd-public-cassandra-testnet-v2")
        .with_properties(properties)
        .with_chain_type(ChainType::Live)
        .with_boot_nodes(vec![
            "/dns/node-1.testnet.prdctr.io/tcp/30333/p2p/12D3KooWPDtoyeoH9cWr4aVSTwjEBz1BTQGdeF2y2V8G7sAacKJx"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-2.testnet.prdctr.io/tcp/30333/p2p/12D3KooWC4tyhGRVcrL9S1LyKVMnTbi3enavGCpDzkovkCJtpxxY"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-3.testnet.prdctr.io/tcp/30333/p2p/12D3KooW9wJLsmp5ZLv4gkdDmoo2QcG2MZsRGBoYrEkWATrh8Vpu"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-4.testnet.prdctr.io/tcp/30333/p2p/12D3KooWCJvvD2i4i3ZXQxqUY5vr7xD781ATWEzdzMUXPjsqh2r4"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-5.testnet.prdctr.io/tcp/30333/p2p/12D3KooWR9woUYXi5isb3ES46yZRjvmSwNKtz5XLXmKsSM2dGmDD"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
        ])
        .with_genesis_config_preset_name(common_primitives::presents::PUBLIC_TESTNET_RUNTIME_PRESET)
        .build())
    }

    #[cfg(not(feature = "enable-static-presents"))]
    {
        ChainSpec::from_json_bytes(&include_bytes!("./chain-specs/testnet.json")[..])
    }
}

pub fn mainnet_config() -> Result<ChainSpec, String> {
    {
        let properties = prd_chain_properties();

        Ok(ChainSpec::builder(
            WASM_BINARY.ok_or_else(|| "Public mainnet wasm not available".to_string())?,
            None,
        )
        .with_name("PRDCTR")
        .with_id("prdctr_mainnet_v1")
        .with_protocol_id("prdctr-mainnet-v1")
        .with_properties(properties)
        .with_chain_type(ChainType::Live)
        .with_boot_nodes(vec![
            "/dns/node-1.prdctr.io/tcp/30333/p2p/12D3KooWSVi3XCMUDQSJKMCX2zQNXozWem8vygEaQGAzbMkgvUfz"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-2.prdctr.io/tcp/30333/p2p/12D3KooWFp451dBzqPQ6GvLnGnNoqS5dCSG6cWftFnng4m67tAAw"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-3.prdctr.io/tcp/30333/p2p/12D3KooWMUQWvYMAbTCTrfUPGKHqHymiLBnKsice3YSFKnqAGKVv"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-4.prdctr.io/tcp/30333/p2p/12D3KooWQVBzUqBtbzazekYzbrEqqJWiP8MXEnP2ngNFgcGAbzqq"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
            "/dns/node-5.prdctr.io/tcp/30333/p2p/12D3KooWNe1SQkqqNY5kPNvR2rSvXSdtYn8KWFAzwWBrA3iceTJm"
                .parse()
                .map_err(|err| format!("Invalid public mainnet bootnode: {err}"))?,
        ])
        .with_genesis_config_preset_name(common_primitives::presents::MAINNET_RUNTIME_PRESET)
        .build())
    }
}
