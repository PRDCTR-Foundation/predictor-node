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
        account, key_pair, new_test_ext, AssetManager, Runtime, RuntimeCall, RuntimeOrigin,
        SignedPredictionMarkets, FOREIGN_ASSET, TOKEN,
    },
    *,
};
use alloc::vec;
use frame_support::{assert_noop, assert_ok};
use orml_traits::MultiCurrency;
use prediction_market_primitives::types::{
    Asset, Deadlines, MarketDisputeMechanism, MarketPeriod, MarketType, MultiHash, OutcomeReport,
    SignatureTest, TestAccountIdPK,
};
use sp_avn_common::{InnerCallValidator, Proof};
use sp_core::Pair;
use sp_runtime::Perbill;

fn proof_for(
    seed: u8,
    relayer: TestAccountIdPK,
    payload: Vec<u8>,
) -> Proof<SignatureTest, TestAccountIdPK> {
    Proof { signer: account(seed), relayer, signature: key_pair(seed).sign(&payload).into() }
}

fn deadlines() -> Deadlines<u64> {
    Deadlines { grace_period: 1, oracle_duration: 1, dispute_duration: 1 }
}

fn metadata() -> MultiHash {
    MultiHash::Sha3_384([0u8; 50])
}

fn period() -> MarketPeriod<u64, u64> {
    MarketPeriod::Block(1..10)
}

#[test]
fn signed_create_market_and_deploy_pool_call_signature_is_valid_for_current_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let base_asset = Asset::Prd;
        let creator_fee = Perbill::zero();
        let oracle = account(2);
        let period = period();
        let deadlines = deadlines();
        let metadata = metadata();
        let market_type = MarketType::Categorical(2);
        let dispute_mechanism = Some(MarketDisputeMechanism::Court);
        let amount = 100;
        let spot_prices = vec![50, 50];
        let swap_fee = 1;
        let payload = encode_signed_create_market_and_deploy_pool_params::<Runtime>(
            &relayer,
            0,
            &base_asset,
            &creator_fee,
            &oracle,
            &period,
            &deadlines,
            &metadata,
            &market_type,
            &dispute_mechanism,
            &amount,
            &spot_prices,
            &swap_fee,
        );
        let proof = proof_for(0, relayer, payload);

        let call = RuntimeCall::SignedPredictionMarkets(
            crate::Call::<Runtime>::signed_create_market_and_deploy_pool {
                proof,
                base_asset,
                creator_fee,
                oracle,
                period,
                deadlines,
                metadata,
                market_type,
                dispute_mechanism,
                amount,
                spot_prices,
                swap_fee,
            },
        );

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_transfer_asset_call_signature_is_valid_for_current_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let recipient = account(2);
        let amount = 10;
        let payload = encode_signed_transfer_params::<Runtime>(
            &relayer, &0, &TOKEN, &caller, &recipient, &amount,
        );
        let proof = proof_for(0, relayer, payload);

        let call =
            RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_transfer_asset {
                proof,
                token: TOKEN,
                to: recipient,
                amount,
            });

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_transfer_asset_call_signature_is_invalid_for_stale_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let recipient = account(2);
        let amount = 10;
        let payload = encode_signed_transfer_params::<Runtime>(
            &relayer, &0, &TOKEN, &caller, &recipient, &amount,
        );
        let proof = proof_for(0, relayer, payload);
        UserNonces::<Runtime>::insert(caller, 1);

        let call =
            RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_transfer_asset {
                proof,
                token: TOKEN,
                to: recipient,
                amount,
            });

        assert!(!SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_withdraw_tokens_call_signature_is_valid_for_current_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let amount = 10;
        let payload =
            encode_signed_withdraw_params::<Runtime>(&relayer, &0, &TOKEN, &caller, &amount);
        let proof = proof_for(0, relayer, payload);

        let call =
            RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_withdraw_tokens {
                proof,
                token: TOKEN,
                amount,
            });

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_report_call_signature_is_valid_for_current_market_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let outcome = OutcomeReport::Categorical(1);
        let payload = encode_signed_report_params::<Runtime>(&relayer, &0, &market_id, &outcome);
        let proof = proof_for(0, relayer, payload);

        let call = RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_report {
            proof,
            market_id,
            outcome,
        });

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_report_call_signature_is_invalid_for_stale_market_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let market_id = 42;
        let outcome = OutcomeReport::Categorical(1);
        let payload = encode_signed_report_params::<Runtime>(&relayer, &0, &market_id, &outcome);
        let proof = proof_for(0, relayer, payload);
        MarketNonces::<Runtime>::insert(caller, market_id, 1);

        let call = RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_report {
            proof,
            market_id,
            outcome,
        });

        assert!(!SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_redeem_shares_call_signature_is_valid_for_current_market_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let payload = encode_signed_redeem_shares_params::<Runtime>(&relayer, &0, &market_id);
        let proof = proof_for(0, relayer, payload);

        let call =
            RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_redeem_shares {
                proof,
                market_id,
            });

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_buy_complete_set_call_signature_is_valid_for_current_market_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let market_id = 42;
        let amount = 10;
        let payload =
            encode_signed_buy_complete_set_params::<Runtime>(&relayer, &0, &market_id, &amount);
        let proof = proof_for(0, relayer, payload);

        let call =
            RuntimeCall::SignedPredictionMarkets(crate::Call::<Runtime>::signed_buy_complete_set {
                proof,
                market_id,
                amount,
            });

        assert!(SignedPredictionMarkets::signature_is_valid(&Box::new(call)));
    });
}

#[test]
fn signed_transfer_asset_rejects_sender_that_is_not_signer() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let recipient = account(2);
        let amount = 10;
        let payload = encode_signed_transfer_params::<Runtime>(
            &relayer, &0, &TOKEN, &caller, &recipient, &amount,
        );
        let proof = proof_for(0, relayer, payload);

        assert_noop!(
            SignedPredictionMarkets::signed_transfer_asset(
                RuntimeOrigin::signed(account(2)),
                proof,
                TOKEN,
                recipient,
                amount,
            ),
            Error::<Runtime>::SenderIsNotSigner
        );
    });
}

#[test]
fn signed_transfer_asset_rejects_signature_for_different_payload() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let recipient = account(2);
        let amount = 10;
        let payload = encode_signed_transfer_params::<Runtime>(
            &relayer, &0, &TOKEN, &caller, &recipient, &amount,
        );
        let proof = proof_for(0, relayer, payload);

        assert_noop!(
            SignedPredictionMarkets::signed_transfer_asset(
                RuntimeOrigin::signed(caller),
                proof,
                TOKEN,
                recipient,
                amount + 1,
            ),
            Error::<Runtime>::UnauthorizedSignedTransferTransaction
        );
    });
}

#[test]
fn signed_transfer_asset_delegates_transfer_and_increments_nonce() {
    new_test_ext().execute_with(|| {
        let relayer = account(1);
        let caller = account(0);
        let recipient = account(2);
        let amount = 25;
        AssetManager::set_balance(FOREIGN_ASSET, &caller, 100);
        let payload = encode_signed_transfer_params::<Runtime>(
            &relayer, &0, &TOKEN, &caller, &recipient, &amount,
        );
        let proof = proof_for(0, relayer, payload);

        assert_ok!(SignedPredictionMarkets::signed_transfer_asset(
            RuntimeOrigin::signed(caller),
            proof,
            TOKEN,
            recipient,
            amount,
        ));

        assert_eq!(UserNonces::<Runtime>::get(caller), 1);
        assert_eq!(AssetManager::free_balance(FOREIGN_ASSET, &caller), 75);
        assert_eq!(AssetManager::free_balance(FOREIGN_ASSET, &recipient), amount);
    });
}
