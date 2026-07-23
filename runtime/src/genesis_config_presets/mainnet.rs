use crate::{
    opaque::SessionKeys, AccountId, AuraId, AuthorityDiscoveryId, AuthorsManagerConfig, AvnId,
    EthBridgeConfig, GrandpaId, ImOnlineId, RuntimeGenesisConfig, SessionConfig, SudoConfig,
    SummaryConfig, TokenManagerConfig,
};
use alloc::{vec, vec::Vec};
use common_primitives::constants::{BLOCKS_PER_DAY, BLOCKS_PER_HOUR};
use hex_literal::hex;
use serde_json::Value;
use sp_avn_common::eth::EthereumNetwork;
use sp_core::{crypto::UncheckedInto, ecdsa, sr25519, ByteArray, H160, H256};
use sp_runtime::BoundedVec;

type EthPublicKey = ecdsa::Public;

fn ethereum_public_keys() -> Vec<EthPublicKey> {
    let ecdsa_pub = ecdsa::Public::from_slice;
    #[rustfmt::skip]
    let public_keys: Vec<EthPublicKey> = vec![
        // author-1 0x041442A53751317182694cCF4DC9997D61E0f173
        ecdsa_pub(&hex!["02be2ddaed0fbbf5c60577f7f65c50bdfd6f1ffc16c469278d40f0949d4aa9ef87"]).unwrap(),
        // author-2 0x8c350e49f2fbFe6Ac8058f85E0057eFf090D1559
        ecdsa_pub(&hex!["02e0fa13fb03c30accc1944a86434f84fe183ed7355fc2240ff7d333b1e83ef711"]).unwrap(),
        // author-3 0x12fD6309bB10da94AE599D4b2640fDD4b9f04e9f
        ecdsa_pub(&hex!["02a679e59b9b472e547c3eef5a9f4ad646cda937014939a298a001b82874107781"]).unwrap(),
        // author-4 0x51b78DfDF4Afd4566E76A84a7cA85861F9140439
        ecdsa_pub(&hex!["024f10471a160990344bb83156a4d239574eff14bfba903a976eb4c85a7827d7d0"]).unwrap(),
        // author-5 0xbc3FD1b1c213D6EB62D2aacC828e3cFc1D424B0B
        ecdsa_pub(&hex!["034b0297f8d8a434e4ce418c4705e27ba6ea75386a494dc78f1e951b5c5ee3226f"]).unwrap(),
    ];
    public_keys
}

fn initial_authorities(
) -> Vec<(AccountId, AuraId, GrandpaId, ImOnlineId, AuthorityDiscoveryId, AvnId)> {
    #[rustfmt::skip]
    let initial_authorities: Vec<(AccountId, AuraId, GrandpaId, ImOnlineId, AuthorityDiscoveryId, AvnId)> = vec![(
		// author-1 5DSExxzGC8zstXV21WPTe2g3BN7S8AqEjSM7JqTxgFjFuD5v
		hex!["3ca91b9516ff1dcae051ff5435a3a3d14c7c2752b8f7572eeb06732dd14c1a6e"].into(),
		hex!["b8b92469dfcc87e79578d1e9a69039fd5bc07564942c3eee0450dcde8e633909"].unchecked_into(),
		hex!["42f4812d14455d1eab076ea4b15a866dad9be1ae64476402396b275a411104e6"].unchecked_into(),
		hex!["7c4cb3a8925f82ee2bf64b1c710790f8f6832318d62eab7523a956d70927e036"].unchecked_into(),
		hex!["d0533416853b2534d8fc90482aed7fa52d929763f375131e12047ed12518d67a"].unchecked_into(),
		hex!["546b4559d91c7c28f96c186f836c0e40a762604817b15a376cfd20b9e9d18436"].unchecked_into(),
	),(
		// author-2 5H6CPFPThugMPRZtFaPS11c3xidJ1q3Lz4CGAACx38S2DGHc
		hex!["de5071a577a2bcc29fc127454417e9c6b8996fb02fe87bf3cddc32c8244e2150"].into(),
		hex!["8a1d64ca5c83914a070e904ed788befef942f2fc86ea768699173fc8f79a0d23"].unchecked_into(),
		hex!["7cba1c8870b5995f667f891a901cacf996ff1d207a02463a50d9323c2a468432"].unchecked_into(),
		hex!["302eab8a48d804615e5f1fedba3de31bccc734dbb75926b2a2b055a64c4a0b3d"].unchecked_into(),
		hex!["f4a9fe2c33b6066d3fe887633cd6468f406767bfeea92cf5f4c0c77e2026967a"].unchecked_into(),
		hex!["4acdb77de3e60c034ff3015809934596ac52ec56fb634a4f1c6bec9db9e9940d"].unchecked_into(),
	),(
		// author-3 5Dvo8zGtuQWUtbrdtB72akfDtSbjzCZZh2oe6axtg9tr9BAj
		hex!["527048483fc7a9284dae652da089c8a5f1f6eb478d78a42c6fc2478fdb75c566"].into(),
		hex!["f06882d017990f8455423c11daa46478f583c8bc5e53135f60ee930b64a3a129"].unchecked_into(),
		hex!["bd182ee161ade4dffe5f798e4b48a43c17e24bb99fbe0cfc745b48f95f63ebb7"].unchecked_into(),
		hex!["ac2c9b5510d5fac5aadd7ba09a29c4cf89eda3e7e747c954bbce9c96d988f349"].unchecked_into(),
		hex!["a294c44fa3e16f8619cbdf364ba9db1dbe4829108595c3f31fb5f8c4563a8427"].unchecked_into(),
		hex!["4ad9a47542c3516c789c65eafa8fab95fc096bfe62b5fa4ed2badaae322f5823"].unchecked_into(),
	),(
		// author-4 5EhQFQjDqWjitY8JDsaMQs4hBJvECX9MnKLkSARW7P6cfi9U
		hex!["747545faa10461734b8c3573be27dfe1b41598df9a4f288d3dba8664f4048705"].into(),
		hex!["ce14e08a89b236f8243828a596f926dff61cb0412cb64d7b932450a4cd571770"].unchecked_into(),
		hex!["196182f3b2b682d5b6ec31b34c9c0ff2eff5de64f2908603bc78a8bcec7cc392"].unchecked_into(),
		hex!["6218d642117f448707bf3aefc678381864bfc481078731db77a3b716a5d6754f"].unchecked_into(),
		hex!["006415599a0a50e861ea53846926ff034efce90d8fbb1b235e65e17d59bdd94a"].unchecked_into(),
		hex!["3c9ded6d4b011e97c5767d7017dd720417682387a8f2510a986acc4cb5902902"].unchecked_into(),
	),(
		// author-5 5D4QDHcBr6GLT83vjMDeiG65HHmGPxGchno2dhZzouPfH2Ts
		hex!["2c00d6344fe21b09f4ef2299c3311531e64e7b341821382a9eb4029ba00daa76"].into(),
		hex!["0ac9db658146d34529c0b8218774ef94885a2189d3a6ae702adb060f57d8c753"].unchecked_into(),
		hex!["6eae535d3ababd922b29db0ede1e8c28434a8aa43a06ebe1a30ead2f78b34162"].unchecked_into(),
		hex!["804d1afc4dcd05728ac3bade922bf79478588a0748eaf129b76c7a364cb5c153"].unchecked_into(),
		hex!["302e7489899c98a061dda9c3661d4b52cdd978a86825d87bed5845297b046b12"].unchecked_into(),
		hex!["e65ba7f6f9f09e582e13f3e566eddff9fffaae7252ce4c4ffa5eeb28d3801d7b"].unchecked_into(),
	)];
    initial_authorities
}

pub(super) fn genesis() -> Value {
    let eth_public_keys = ethereum_public_keys();
    let root: AccountId = sr25519::Public::from_raw(hex!(
        "e4c7f97ea1f29c113b83fa132a9fe6aa6e32e7c3eb3f94801a4b33df6779087c"
    ))
    .into();
    let initial_authorities = initial_authorities();

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
            avt_token_contract: H160(hex!("c84782858B7Bef5d25182Dbac956A6Aa463AeFE5")),
            lower_schedule_period: 12 * BLOCKS_PER_HOUR,
            ..Default::default()
        },
        eth_bridge: EthBridgeConfig {
            instance: sp_avn_common::eth::EthBridgeInstance {
                network: EthereumNetwork::Mainnet,
                // TODO: update me. This is the address of the bridge contract on Mainnet, but it is
                // not yet deployed. bridge_contract:
                // H160(hex!("83478B43A9809Ecfc86cb063C733ECdee074EF72")),
                bridge_contract: H160::zero(),
                name: BoundedVec::truncate_from("PRDCTRBridge".into()),
                version: BoundedVec::truncate_from("1".into()),
                salt: None,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    serde_json::to_value(config).expect("Could not build genesis config.")
}
