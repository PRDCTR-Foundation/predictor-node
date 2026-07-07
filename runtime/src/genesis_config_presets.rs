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
        Ok(common_primitives::presents::PUBLIC_TESTNET_RUNTIME_PRESET) => public_testnet_genesis(),
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
    vec![
        PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
        PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
        PresetId::from(common_primitives::presents::STAGING_TESTNET_RUNTIME_PRESET),
        PresetId::from(common_primitives::presents::PUBLIC_TESTNET_RUNTIME_PRESET),
    ]
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

fn public_testnet_ethereum_public_keys() -> Vec<EthPublicKey> {
    let ecdsa_pub = ecdsa::Public::from_slice;
    #[rustfmt::skip]
    let public_keys: Vec<EthPublicKey> = vec![
        // 0x073411c96F59ef379DE620fd3226eA3f222af1b9
        ecdsa_pub(&hex!["037abd11467b065e7b12d7b61cf9d73c1c359c5ece1b15a560806a610ccb6d0e09"]).unwrap(),
        // 0xE43ce3aEF589a1c413A4213F9937Ac60D341d214
        ecdsa_pub(&hex!["03fe1be9394842c6a52e9d2acb7826331eb2d4fe3ab488c6f184e97767caa740d8"]).unwrap(),
        // 0xee2238986aE9C2D104cd11a3e2165c4684580eF9
        ecdsa_pub(&hex!["03faee2e31f9f6f256f3fb58971448ae5bd0d353ed3d3a09c7d921b65349a6fb7a"]).unwrap(),
        // 0xF6D4696405B4D6971bb0532cf5e76774259575aA
        ecdsa_pub(&hex!["03006f92d93a4cb34528bd4db425489b4bca783cd29a06a752c0c14e909e9836ae"]).unwrap(),
        // 0xF45337E8A2ffE96809B71a6D6Be186985457f6bB
        ecdsa_pub(&hex!["0340831337647a7e796f4e9feca9a8af14b50e17e1cc7be25fbc9928ac9a2914b5"]).unwrap(),
    ];
    public_keys
}

fn public_testnet_initial_authorities(
) -> Vec<(AccountId, AuraId, GrandpaId, ImOnlineId, AuthorityDiscoveryId, AvnId)> {
    use sp_core::crypto::UncheckedInto;
    #[rustfmt::skip]
    let initial_authorities: Vec<(AccountId, AuraId, GrandpaId, ImOnlineId, AuthorityDiscoveryId, AvnId)> = vec![(
		// Author-1 5F6d5JcHJzd4rW1ALsHvrCTcymLBV4D6ySKNnpQhb5uZ7hJK
		hex!["862b17d13c612140c7bd05592b307ee910b3e535e55c4ceed5498a0e3c9db307"].into(),
		hex!["7e76d552618265b405d8a4563feea6be19127be5cf0584e5b5bf29a015ab072a"].unchecked_into(),
		hex!["890a3d65da72977fe14ae4a9b211c3a89e001873275e5c80abfc36b7d8456c2e"].unchecked_into(),
		hex!["b63442936c2d5b405cce61de680f705fb7b3e812c5ef37dabf5e42284a7d6b71"].unchecked_into(),
		hex!["7eca929ba1c6a01741baad5671753dd9eb0329b7a7df618f25b01b1e21fb5b14"].unchecked_into(),
		hex!["d8bea8fb05df6e07d27574f7e6890ec97182bd187167757f05573a866b4ceb0f"].unchecked_into(),
	),(
		// Author-2 5EcZbnqzHNHkwYfK6VwiV5L4jvFcSfbgbY2Yq8CjTJgdZyKe
		hex!["70c486a5453218fd491f3c1708e42a4f5616ad5a2aecaa8a8f5c22cb71564e0c"].into(),
		hex!["38fddf738d53b0842df80970696b6653108b16351eed19503218c6831b178e09"].unchecked_into(),
		hex!["e3ac2bb0dcdaba222e451efd874ef7d7042f1056c1a5a7a1a1e8cb6bf11eef52"].unchecked_into(),
		hex!["8233420b5d23a57af990199ca72cadfd07b12e922d72ec18077a6d12a01ef529"].unchecked_into(),
		hex!["0883c8ff536dba8faeba50f1f910c02dca257d4947b6256741909cfb662f791c"].unchecked_into(),
		hex!["d64ae61b6621c48bc782b8983d666a30619bc8334a65b4323ba0036fc6e04278"].unchecked_into(),
	),(
		// Author-3 5CVYXQtwdH5HKos4WKzDpRLQYnVMaPnmQojarNEtPQrnQdx8
		hex!["12f1b8865a4ae30b41b37bc32ffdfcb9fb533f6d23bc83df3d46617bd7e5231d"].into(),
		hex!["9434a55bd367bf575115769a8a85b52924ddfe16038439b5cbfd250d7244870d"].unchecked_into(),
		hex!["391c794a46aa06d469c240a3d5296f29492db75697db5453a5d24ffd35ae6432"].unchecked_into(),
		hex!["fa4731ab4e01e9df07d7dca08294c99aa66c4285e614e00770093cfdc003ed05"].unchecked_into(),
		hex!["e45792901c6a021d68e30b2bee47408da36d0cb15463b5a407de6ddf7735da14"].unchecked_into(),
		hex!["1ed9ea9808b2077169fa554f2084582a63a12cf3b7ac51952677ac60f960846f"].unchecked_into(),
	),(
		// Author-4 5DUmdyRMt16kmsEMzieFBPQqt5jM6Lke73x4MBKx5Zi8Rv17
		hex!["3e96d9fe269917236c07346878abc0771903eb067113693f0a5fd55cc4212243"].into(),
		hex!["863098afd0b78ef407f979d0b6fe01bf314c097815f9411b583bef2ec50d8667"].unchecked_into(),
		hex!["512e75bc49cb147f29b2a38c531824a089c82c1a1a8c70b908f2df761ee6cbc3"].unchecked_into(),
		hex!["9648bab6c5d405afad2921244c0f7bbec968139659827b4edb7a0c5646b19511"].unchecked_into(),
		hex!["c4795a8ad28ab18e9b656941dd52dcd9be75526d742ec10f3eb58351b64ad20a"].unchecked_into(),
		hex!["c4a51e4fe4065c6c95891c0fbb33ca1249deb922121a423dffbd7a01b03e7053"].unchecked_into(),
	),(
		// Author-5 5Ey1kbXzRR3zie4UnHJTFqiBxQ6ezWyaCqis51C8eVxHac3Y
		hex!["805d780900f281567adcca5c91f6296fb3ecc693ddc2ea493fd4ce1015e59944"].into(),
		hex!["7c4238de215a3bfa1ac2545ec453f73efcb6e95908c3cc79a85cb182cf36e535"].unchecked_into(),
		hex!["226aa01ed45e3271cb850ff8be966506a9b5661cb1ac9313d609a4c975288ae2"].unchecked_into(),
		hex!["68ea271b4296bba79b42696fd2aba7faa8f5c6c4d909259f034e98b84b843c4d"].unchecked_into(),
		hex!["626fc5477c2173f79e1211dd89d443069dbf16f09c45fd2a50c5f4f147ddb36e"].unchecked_into(),
		hex!["c88510affc6377d814686c2b6f865125205c7108ae7a80bc94d74d99c8f97e39"].unchecked_into(),
	)];
    initial_authorities
}

fn public_testnet_genesis() -> Value {
    use sp_avn_common::eth::EthereumNetwork;
    use sp_runtime::BoundedVec;
    let eth_public_keys = public_testnet_ethereum_public_keys();
    let root: AccountId = sr25519::Public::from_raw(hex!(
        "70ffd9cf70c2388ce9ee611ce0164508022674a592e623abf96d96dc8064882d"
    ))
    .into();
    let initial_authorities = public_testnet_initial_authorities();

    let config = RuntimeGenesisConfig {
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
            lower_schedule_period: 30 * BLOCKS_PER_MINUTE,
            ..Default::default()
        },
        eth_bridge: crate::EthBridgeConfig {
            instance: sp_avn_common::eth::EthBridgeInstance {
                network: EthereumNetwork::Sepolia,
                bridge_contract: H160(hex!("DF1E384d36A6EE55a1b3c89bF6ec720fC5c611EB")),
                name: BoundedVec::truncate_from("PredictorBridge".into()),
                version: BoundedVec::truncate_from("1".into()),
                salt: None,
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let val = serde_json::to_value(config).expect("Could not build genesis config.");
    val
}
