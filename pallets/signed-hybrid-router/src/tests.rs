// Copyright 2024-2025 Forecasting Technologies LTD.
//
// This file is part of Predictor.
//
// Predictor is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at
// your option) any later version.
//
// Predictor is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Predictor. If not, see <https://www.gnu.org/licenses/>.

#![cfg(test)]

use crate::{
    mock::{
        account, key_pair, new_test_ext, Runtime, RuntimeCall, RuntimeOrigin, SignedHybridRouter,
    },
    *,
};
use alloc::vec;
use frame_support::assert_noop;
use pallet_pm_hybrid_router::types::Strategy;
use prediction_market_primitives::types::{Asset, SignatureTest, TestAccountIdPK};
use sp_avn_common::{InnerCallValidator, Proof};
use sp_core::Pair;

fn proof_for(
    seed: u8,
    relayer: TestAccountIdPK,
    payload: Vec<u8>,
) -> Proof<SignatureTest, TestAccountIdPK> {
    Proof { signer: account(seed), relayer, signature: key_pair(seed).sign(&payload) }
}

#[test]
fn signed_buy_call_signature_is_valid_for_current_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let max_price = 50;
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

        let call = RuntimeCall::SignedHybridRouter(crate::Call::<Runtime>::signed_buy {
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        });

        assert!(SignedHybridRouter::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_buy_call_signature_is_invalid_for_stale_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let max_price = 50;
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
        MarketNonces::<Runtime>::insert(account(0), market_id, 1);

        let call = RuntimeCall::SignedHybridRouter(crate::Call::<Runtime>::signed_buy {
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        });

        assert!(!SignedHybridRouter::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_buy_rejects_sender_that_is_not_signer() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let max_price = 50;
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
            SignedHybridRouter::signed_buy(
                RuntimeOrigin::signed(account(2)),
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
fn signed_buy_rejects_signature_for_different_payload() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let max_price = 50;
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
            SignedHybridRouter::signed_buy(
                RuntimeOrigin::signed(account(0)),
                proof,
                market_id,
                asset_count,
                asset,
                amount_in + 1,
                max_price,
                orders,
                strategy,
            ),
            Error::<Runtime>::UnauthorizedSignedTransaction
        );
    });
}

#[test]
fn signed_sell_call_signature_is_valid_for_current_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let min_price = 5;
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

        let call = RuntimeCall::SignedHybridRouter(crate::Call::<Runtime>::signed_sell {
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            min_price,
            orders,
            strategy,
        });

        assert!(SignedHybridRouter::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_sell_rejects_sender_that_is_not_signer() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let asset_count = 2u16;
        let asset = Asset::CategoricalOutcome(market_id, 0);
        let amount_in = 10;
        let min_price = 5;
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
            SignedHybridRouter::signed_sell(
                RuntimeOrigin::signed(account(2)),
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
