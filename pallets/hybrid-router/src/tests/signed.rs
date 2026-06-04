// Copyright 2024-2025 Forecasting Technologies LTD.
//
// This file is part of Zeitgeist.
//
// Zeitgeist is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at
// your option) any later version.
//
// Zeitgeist is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Zeitgeist. If not, see <https://www.gnu.org/licenses/>.

use super::*;
use prediction_market_primitives::{
    test_helper::TestAccount,
    types::{SignatureTest, TestAccountIdPK},
};
use sp_avn_common::{InnerCallValidator, Proof};
use sp_core::Pair;
use zeitgeist_hybrid_router::Strategy;

fn proof_for(
    seed: u8,
    relayer: TestAccountIdPK,
    payload: Vec<u8>,
) -> Proof<SignatureTest, TestAccountIdPK> {
    let signer = TestAccount::new([seed; 32]);

    Proof {
        signer: signer.account_id(),
        relayer,
        signature: signer.key_pair().sign(&payload).into(),
    }
}

#[test]
fn signed_buy_call_signature_is_valid_for_current_nonce() {
    ExtBuilder::default().build().execute_with(|| {
        let relayer = bob();
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = _1;
        let max_price = _9_10;
        let orders = vec![];
        let strategy = Strategy::ImmediateOrCancel;

        let payload = encode_signed_buy_params::<Runtime>(
            &relayer,
            0,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &max_price,
            &orders,
            &strategy,
        );
        let proof = proof_for(0, relayer, payload);

        let call = RuntimeCall::HybridRouter(crate::Call::<Runtime>::signed_buy {
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        });

        assert!(HybridRouter::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_buy_call_signature_is_invalid_for_stale_nonce() {
    ExtBuilder::default().build().execute_with(|| {
        let relayer = bob();
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = _1;
        let max_price = _9_10;
        let orders = vec![];
        let strategy = Strategy::ImmediateOrCancel;

        let payload = encode_signed_buy_params::<Runtime>(
            &relayer,
            0,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &max_price,
            &orders,
            &strategy,
        );
        let proof = proof_for(0, relayer, payload);
        MarketNonces::<Runtime>::insert(alice(), market_id, 1);

        let call = RuntimeCall::HybridRouter(crate::Call::<Runtime>::signed_buy {
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        });

        assert!(!HybridRouter::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_buy_rejects_sender_that_is_not_signer() {
    ExtBuilder::default().build().execute_with(|| {
        let relayer = bob();
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = _1;
        let max_price = _9_10;
        let orders = vec![];
        let strategy = Strategy::ImmediateOrCancel;

        let payload = encode_signed_buy_params::<Runtime>(
            &relayer,
            0,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &max_price,
            &orders,
            &strategy,
        );
        let proof = proof_for(0, relayer, payload);

        assert_noop!(
            HybridRouter::signed_buy(
                RuntimeOrigin::signed(charlie()),
                proof,
                market_id,
                asset_count,
                asset,
                amount_in,
                max_price,
                orders,
                strategy,
            ),
            Error::<Runtime>::SenderIsNotSigner
        );
    });
}

#[test]
fn signed_buy_increments_nonce_after_delegated_trade_succeeds() {
    ExtBuilder::default().build().execute_with(|| {
        let market_id = create_market_and_deploy_pool(
            alice(),
            BASE_ASSET,
            MarketType::Categorical(2),
            _10,
            vec![_1_2, _1_2],
            CENT_BASE,
        );
        let relayer = bob();
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = _1;
        let max_price = _9_10;
        let orders = vec![];
        let strategy = Strategy::ImmediateOrCancel;
        assert_ok!(AssetManager::deposit(BASE_ASSET, &alice(), amount_in));

        let payload = encode_signed_buy_params::<Runtime>(
            &relayer,
            0,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &max_price,
            &orders,
            &strategy,
        );
        let proof = proof_for(0, relayer, payload);

        assert_ok!(HybridRouter::signed_buy(
            RuntimeOrigin::signed(alice()),
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        ));
        assert_eq!(HybridRouter::market_nonces(alice(), market_id), 1);
    });
}

#[test]
fn signed_sell_rejects_sender_that_is_not_signer() {
    ExtBuilder::default().build().execute_with(|| {
        let relayer = bob();
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = _1;
        let min_price = _1_10;
        let orders = vec![];
        let strategy = Strategy::ImmediateOrCancel;

        let payload = encode_signed_sell_params::<Runtime>(
            &relayer,
            0,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &min_price,
            &orders,
            &strategy,
        );
        let proof = proof_for(0, relayer, payload);

        assert_noop!(
            HybridRouter::signed_sell(
                RuntimeOrigin::signed(charlie()),
                proof,
                market_id,
                asset_count,
                asset,
                amount_in,
                min_price,
                orders,
                strategy,
            ),
            Error::<Runtime>::SenderIsNotSigner
        );
    });
}
