use crate::{
    configs::genesis_config_helpers::{get_account_id_from_seed, get_validator_keys_from_seed},
    opaque::SessionKeys,
    AccountId, AuraId, AuthorityDiscoveryId, AuthorsManagerConfig, AvnId, BalancesConfig,
    GrandpaId, ImOnlineId, RuntimeGenesisConfig, SessionConfig, SudoConfig, SummaryConfig,
    TokenManagerConfig,
};
use alloc::{vec, vec::Vec};
use frame_support::PalletId;
use hex_literal::hex;
use serde_json::Value;
use sp_core::{ecdsa, sr25519, ByteArray, H160, H256};
use sp_genesis_builder::PresetId;
use sp_runtime::traits::AccountIdConversion;

type EthPublicKey = ecdsa::Public;
use common_primitives::constants::{BLOCKS_PER_DAY, BLOCKS_PER_MINUTE};

#[cfg(feature = "enable-static-presents")]
mod public_testnet;

fn testnet_genesis(
    initial_authorities: Vec<(
        AccountId,
        AuraId,
        GrandpaId,
        ImOnlineId,
        AuthorityDiscoveryId,
        AvnId,
    )>,
    endowed_accounts: Vec<AccountId>,
    root: AccountId,
) -> Value {
    let eth_public_keys = local_ethereum_public_keys();
    // TokenManager treasury account, pre-funded so pallet-node-manager's
    // reward-period rollover transfer into the reward pot succeeds.
    let treasury_account: AccountId = PalletId(*b"Treasury").into_account_truncating();
    let mut balances: Vec<(AccountId, u128)> = endowed_accounts
        .iter()
        .cloned()
        .map(|k| (k, 1u128 << 60))
        .collect();
    balances.push((treasury_account, 1u128 << 60));
    let config = RuntimeGenesisConfig {
        balances: BalancesConfig { balances },
        authors_manager: AuthorsManagerConfig {
            authors: initial_authorities
                .iter()
                .map(|x| x.0.clone())
                .zip(eth_public_keys.iter().map(|pk| pk.clone()))
                .collect::<Vec<_>>(),
        },
        session: SessionConfig {
            keys: initial_authorities
                .iter()
                .map(|(account, aura, grandpa, im_online, authority_discovery, avn)| {
                    (
                        account.clone(),
                        account.clone(),
                        SessionKeys {
                            aura: aura.clone(),
                            grandpa: grandpa.clone(),
                            im_online: im_online.clone(),
                            authority_discovery: authority_discovery.clone(),
                            avn: avn.clone(),
                        },
                    )
                })
                .collect::<Vec<_>>(),
            ..Default::default()
        },
        sudo: SudoConfig { key: Some(root) },
        summary: SummaryConfig {
            schedule_period: 5 * BLOCKS_PER_DAY,
            voting_period: 100,
            ..Default::default()
        },
        token_manager: TokenManagerConfig {
            lower_account_id: H256(hex!(
                "000000000000000000000000000000000000000000000000000000000000dead"
            )),
            avt_token_contract: H160(hex!("DF1E384d36A6EE55a1b3c89bF6ec720fC5c611EB")),
            lower_schedule_period: 5 * BLOCKS_PER_MINUTE,
            ..Default::default()
        },
        ..Default::default()
    };

    serde_json::to_value(config).expect("Could not build genesis config.")
}

fn staging_testnet_genesis() -> Value {
    testnet_genesis(
        // initial validators.
        vec![
            get_authority_keys_from_seed("prd-author-1"),
            get_authority_keys_from_seed("prd-author-2"),
            get_authority_keys_from_seed("prd-author-3"),
            get_authority_keys_from_seed("prd-author-4"),
        ],
        vec![
            get_account_id_from_seed::<sr25519::Public>("Bank"),
            get_account_id_from_seed::<sr25519::Public>("prd-sudo"),
        ],
        get_account_id_from_seed::<sr25519::Public>("prd-sudo"),
    )
}

fn local_testnet_genesis() -> Value {
    testnet_genesis(
        // initial validators.
        vec![get_authority_keys_from_seed("Alice"), get_authority_keys_from_seed("Bob")],
        vec![
            get_account_id_from_seed::<sr25519::Public>("Alice"),
            get_account_id_from_seed::<sr25519::Public>("Bob"),
            get_account_id_from_seed::<sr25519::Public>("Charlie"),
            get_account_id_from_seed::<sr25519::Public>("Dave"),
            get_account_id_from_seed::<sr25519::Public>("Eve"),
            get_account_id_from_seed::<sr25519::Public>("Ferdie"),
            get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
            get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
            get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
            get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
            get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
            get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
        ],
        get_account_id_from_seed::<sr25519::Public>("Alice"),
    )
}

fn development_config_genesis() -> Value {
    testnet_genesis(
        // initial validators.
        vec![get_authority_keys_from_seed("Alice")],
        vec![
            get_account_id_from_seed::<sr25519::Public>("Alice"),
            get_account_id_from_seed::<sr25519::Public>("Bob"),
            get_account_id_from_seed::<sr25519::Public>("Charlie"),
            get_account_id_from_seed::<sr25519::Public>("Dave"),
            get_account_id_from_seed::<sr25519::Public>("Eve"),
            get_account_id_from_seed::<sr25519::Public>("Ferdie"),
            get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
            get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
            get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
            get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
            get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
            get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
        ],
        get_account_id_from_seed::<sr25519::Public>("Alice"),
    )
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<vec::Vec<u8>> {
    let patch = match id.try_into() {
        #[cfg(feature = "enable-static-presents")]
        Ok(common_primitives::presents::PUBLIC_TESTNET_RUNTIME_PRESET) => public_testnet::genesis(),
        Ok(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET) =>
            staging_testnet_genesis(),
        Ok(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET) => local_testnet_genesis(),
        Ok(sp_genesis_builder::DEV_RUNTIME_PRESET) => development_config_genesis(),
        _ => return None,
    };
    Some(
        serde_json::to_string(&patch)
            .expect("serialization to json is expected to work. qed.")
            .into_bytes(),
    )
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
    #[cfg(feature = "enable-static-presents")]
    {
        vec![
            PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
            PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
            PresetId::from(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET),
            PresetId::from(common_primitives::presents::PUBLIC_TESTNET_RUNTIME_PRESET),
        ]
    }

    #[cfg(not(feature = "enable-static-presents"))]
    {
        vec![
            PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
            PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
            PresetId::from(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET),
        ]
    }
}

/// Generate the full set of session keys used by the runtime
/// (aura + grandpa + im-online + authority-discovery + avn).
pub fn get_authority_keys_from_seed(
    s: &str,
) -> (AccountId, AuraId, GrandpaId, ImOnlineId, AuthorityDiscoveryId, AvnId) {
    (
        get_account_id_from_seed::<sr25519::Public>(s),
        get_validator_keys_from_seed::<AuraId>(s),
        get_validator_keys_from_seed::<GrandpaId>(s),
        get_validator_keys_from_seed::<ImOnlineId>(s),
        get_validator_keys_from_seed::<AuthorityDiscoveryId>(s),
        get_validator_keys_from_seed::<AvnId>(s),
    )
}

fn local_ethereum_public_keys() -> Vec<EthPublicKey> {
    /*
        The following test public keys are generated with 12 word mnemonic:
        ship sunset goose humble bicycle alert ten delay tag pig erase health

        Derivation			Address										Public key																Private key
        m/44'/60'/0'/0/0 	0xcc66EC55E0cdF70e1549beBE969e5988603Ef960 	0x0385c59f553aa213cf9ff9e583ee7bd863e8fb6251676686cc58966c71e020c524 	0xa0c25923fd51cfe4984ce6a485e768eb8a5b1e632e9952c4b22af70b54ee6bf2
        m/44'/60'/0'/0/1 	0x890E39BaF40792D0Df2582c7C232CE4a8D5Bf965 	0x02716144732ac662116c9763026a77a93b2f50add8c143f32e7067a60738521e43 	0x50a43e62554a37ad4e55cf0a01062898ee19a2a6fd4ab91a763b3d37a6773d70
        m/44'/60'/0'/0/2 	0x2cC51c7b7b795088Ac10c06cDfc0593a821d3C55 	0x020d91de7d1a039d3f1c66caa6da89ee71f06b79b7cdcf380a72e098d164cd41b0 	0xabd72658e2cc0e6fb78975557db63803197062edb4dff6e5207cca4b4c505e4b
        m/44'/60'/0'/0/3 	0x548e68b384fd8Ac91C88Ad16Cb919b24d7afed52 	0x03b802f4066d418778e8f7f4b1c38b23620ab98f1047304f20a077723e5d51c76b 	0xd52770a47f3ac073d5be73c33d696ff703580e6d0e5a14881cd7afa440f25662
        m/44'/60'/0'/0/4 	0xb9f5946F03c03e3dEB3A8021Bbd2074648fcff20 	0x039b43fbeabee71dd96e9e4be1b0c3f3786de91767d0f4215161c1a996cd03fd5e 	0x38f6727203d81498086412b700dba13d9e09cf98c14aec58c1c5405eed8f36dc
    */
    return vec![
        ecdsa::Public::from_slice(&hex![
            "0385c59f553aa213cf9ff9e583ee7bd863e8fb6251676686cc58966c71e020c524"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "02716144732ac662116c9763026a77a93b2f50add8c143f32e7067a60738521e43"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "020d91de7d1a039d3f1c66caa6da89ee71f06b79b7cdcf380a72e098d164cd41b0"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "03b802f4066d418778e8f7f4b1c38b23620ab98f1047304f20a077723e5d51c76b"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "039b43fbeabee71dd96e9e4be1b0c3f3786de91767d0f4215161c1a996cd03fd5e"
        ])
        .unwrap(),
    ]
}
