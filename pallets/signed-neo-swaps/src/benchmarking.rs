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

#![allow(
    // Benchmark setup deliberately uses worst-case arithmetic values.
    clippy::arithmetic_side_effects
)]
#![cfg(feature = "runtime-benchmarks")]

use crate::*;
use alloc::{vec, vec::Vec};
use common_primitives::constants::currency::CENT_BASE;
use frame_benchmarking::v2::*;
use frame_support::{
    assert_ok,
    storage::{with_transaction, TransactionOutcome::*},
};
use frame_system::RawOrigin;
use orml_traits::MultiCurrency;
use pallet_pm_market_commons::MarketCommonsPalletApi;
use pallet_pm_neo_swaps::{AssetOf, BalanceOf, MarketIdOf, Pallet as NeoSwaps, MIN_SPOT_PRICE};
use parity_scale_codec::{Decode, Encode};
use prediction_market_primitives::{
    constants::base_multiples::*,
    math::fixed::{BaseProvider, PredictionMarketBase},
    traits::CompleteSetOperationsApi,
    types::{Asset, Market, MarketCreation, MarketPeriod, MarketStatus, MarketType, ScoringRule},
};
use sp_avn_common::Proof;
use sp_core::{crypto::DEV_PHRASE, ByteArray, H256};
use sp_runtime::{traits::SaturatedConversion, Perbill, RuntimeAppPublic};

type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

macro_rules! assert_ok_with_transaction {
    ($expr:expr) => {{
        assert_ok!(with_transaction(|| match $expr {
            Ok(val) => Commit(Ok(val)),
            Err(err) => Rollback(Err(err)),
        }));
    }};
}

fn create_market<T: Config>(
    caller: T::AccountId,
    base_asset: AssetOf<T>,
    asset_count: u16,
) -> MarketIdOf<T> {
    let market = Market {
        market_id: 0u8.into(),
        base_asset,
        creation: MarketCreation::Permissionless,
        creator_fee: Perbill::zero(),
        creator: caller.clone(),
        oracle: caller,
        metadata: vec![0, 50],
        market_type: MarketType::Categorical(asset_count),
        period: MarketPeriod::Block(0u32.into()..1u32.into()),
        deadlines: Default::default(),
        scoring_rule: ScoringRule::AmmCdaHybrid,
        status: MarketStatus::Active,
        report: None,
        resolved_outcome: None,
        dispute_mechanism: None,
        bonds: Default::default(),
        early_close: None,
    };
    <T as pallet_pm_neo_swaps::Config>::MarketCommons::push_market(market).unwrap()
}

fn create_spot_prices<T: Config>(asset_count: u16) -> Vec<BalanceOf<T>> {
    let mut result = vec![MIN_SPOT_PRICE.saturated_into(); (asset_count - 1) as usize];
    let remaining_u128 =
        PredictionMarketBase::<u128>::get().unwrap() - (asset_count - 1) as u128 * MIN_SPOT_PRICE;
    result.push(remaining_u128.saturated_into());
    result
}

fn create_market_and_deploy_pool<T: Config>(
    caller: T::AccountId,
    base_asset: AssetOf<T>,
    asset_count: u16,
    amount: BalanceOf<T>,
) -> MarketIdOf<T> {
    let market_id = create_market::<T>(caller.clone(), base_asset, asset_count);
    let total_cost =
        amount + <T as pallet_pm_neo_swaps::Config>::MultiCurrency::minimum_balance(base_asset);
    assert_ok!(<T as pallet_pm_neo_swaps::Config>::MultiCurrency::deposit(
        base_asset, &caller, total_cost
    ));
    assert_ok_with_transaction!(
        <T as pallet_pm_neo_swaps::Config>::CompleteSetOperations::buy_complete_set(
            caller.clone(),
            market_id,
            amount,
        )
    );
    assert_ok!(NeoSwaps::<T>::deploy_pool(
        RawOrigin::Signed(caller).into(),
        market_id,
        amount,
        create_spot_prices::<T>(asset_count),
        CENT_BASE.saturated_into(),
    ));
    market_id
}

fn set_up_liquidity<T: Config>(
    market_id: MarketIdOf<T>,
    account: AccountIdOf<T>,
    amount: BalanceOf<T>,
) {
    let base_asset = <T as pallet_pm_neo_swaps::Config>::MarketCommons::market(&market_id)
        .expect("benchmark market exists")
        .base_asset;
    assert_ok!(<T as pallet_pm_neo_swaps::Config>::MultiCurrency::deposit(
        base_asset, &account, amount,
    ));
    assert_ok_with_transaction!(
        <T as pallet_pm_neo_swaps::Config>::CompleteSetOperations::buy_complete_set(
            account, market_id, amount,
        )
    );
}

fn accrue_fees<T: Config>(market_id: MarketIdOf<T>, account: AccountIdOf<T>, amount: BalanceOf<T>) {
    let base_asset = <T as pallet_pm_neo_swaps::Config>::MarketCommons::market(&market_id)
        .expect("benchmark market exists")
        .base_asset;
    let assets = NeoSwaps::<T>::assets(market_id).expect("benchmark pool assets exist");
    let asset_count = assets.len().try_into().expect("benchmark asset count fits in u16");

    assert_ok!(<T as pallet_pm_neo_swaps::Config>::MultiCurrency::deposit(
        base_asset, &account, amount
    ));
    assert_ok!(NeoSwaps::<T>::buy(
        RawOrigin::Signed(account).into(),
        market_id,
        asset_count,
        assets[0],
        amount,
        0u8.into(),
    ));
}

fn into_bytes<T: Config>(account: &<T as pallet_avn::Config>::AuthorityId) -> [u8; 32]
where
    T: Config + pallet_avn::Config,
{
    let bytes = account.encode();
    let mut vector: [u8; 32] = Default::default();
    vector.copy_from_slice(&bytes[0..32]);
    vector
}

fn get_user_account<T: Config>() -> (<T as pallet_avn::Config>::AuthorityId, T::AccountId)
where
    T: Config + pallet_avn::Config,
{
    let key_pair =
        <T as pallet_avn::Config>::AuthorityId::generate_pair(Some(DEV_PHRASE.as_bytes().to_vec()));
    let account_bytes = into_bytes::<T>(&key_pair);
    let account_id = T::AccountId::decode(&mut &account_bytes.encode()[..])
        .expect("benchmark account id can be decoded from sr25519 public key");
    (key_pair, account_id)
}

fn get_relayer<T: Config>() -> T::AccountId {
    let relayer_account: H256 = H256::repeat_byte(1);
    T::AccountId::decode(&mut relayer_account.as_bytes()).expect("valid relayer account id")
}

fn get_proof<T: Config>(
    signer: T::AccountId,
    relayer: T::AccountId,
    signature: &[u8],
) -> Proof<T::Signature, T::AccountId> {
    Proof {
        signer,
        relayer,
        signature: sp_core::sr25519::Signature::from_slice(signature).unwrap().into(),
    }
}

#[benchmarks(where T: Config + pallet_avn::Config)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn signed_join(n: Linear<2, 128>) {
        let (signer_key_pair, signer) = get_user_account::<T>();
        let base_asset = Asset::Prd;
        let asset_count = n.try_into().unwrap();
        let market_id = create_market_and_deploy_pool::<T>(
            signer.clone(),
            base_asset,
            asset_count,
            _10.saturated_into(),
        );
        let pool_shares_amount = _1.saturated_into();
        set_up_liquidity::<T>(market_id, signer.clone(), _100.saturated_into());
        let max_amounts_in = vec![u128::MAX.saturated_into(); asset_count as usize];
        let block_number = frame_system::Pallet::<T>::block_number();

        let relayer = get_relayer::<T>();
        let encoded_payload = encode_signed_join_params::<T>(
            &relayer,
            &market_id,
            &pool_shares_amount,
            &max_amounts_in,
            &block_number,
        );
        let signature = signer_key_pair.sign(&encoded_payload).unwrap().encode();
        let proof = get_proof::<T>(signer.clone(), relayer, &signature);

        #[extrinsic_call]
        signed_join(
            RawOrigin::Signed(signer),
            proof,
            market_id,
            pool_shares_amount,
            max_amounts_in,
            block_number,
        );
    }

    #[benchmark]
    fn signed_withdraw_fees() {
        let (signer_key_pair, signer) = get_user_account::<T>();
        let market_id = create_market_and_deploy_pool::<T>(
            signer.clone(),
            Asset::Prd,
            2u16,
            _10.saturated_into(),
        );
        let base_asset = <T as pallet_pm_neo_swaps::Config>::MarketCommons::market(&market_id)
            .expect("benchmark market exists")
            .base_asset;

        let trader: T::AccountId = account("fee_trader", 0, 0);
        accrue_fees::<T>(market_id, trader, _100.saturated_into());

        let block_number = frame_system::Pallet::<T>::block_number();
        let relayer = get_relayer::<T>();
        let encoded_payload =
            encode_signed_withdraw_fees_params::<T>(&relayer, &market_id, &block_number);
        let signature = signer_key_pair.sign(&encoded_payload).unwrap().encode();
        let proof = get_proof::<T>(signer.clone(), relayer, &signature);

        let initial_balance =
            <T as pallet_pm_neo_swaps::Config>::MultiCurrency::free_balance(base_asset, &signer);

        #[extrinsic_call]
        signed_withdraw_fees(RawOrigin::Signed(signer.clone()), proof, market_id, block_number);

        let final_balance =
            <T as pallet_pm_neo_swaps::Config>::MultiCurrency::free_balance(base_asset, &signer);
        assert!(final_balance > initial_balance);
    }

    #[benchmark]
    fn signed_exit(n: Linear<2, 128>) {
        let (signer_key_pair, signer) = get_user_account::<T>();
        let asset_count = n.try_into().unwrap();
        let market_id = create_market_and_deploy_pool::<T>(
            signer.clone(),
            Asset::Prd,
            asset_count,
            _10.saturated_into(),
        );

        let other_account: T::AccountId = account("other_liquidity_provider", 0, 0);
        let assets = NeoSwaps::<T>::assets(market_id).expect("benchmark pool assets exist");
        let complete_set_amount = _1000.saturated_into();

        set_up_liquidity::<T>(market_id, other_account.clone(), complete_set_amount);
        assert_ok!(NeoSwaps::<T>::join(
            RawOrigin::Signed(other_account.clone()).into(),
            market_id,
            _100.saturated_into(),
            vec![u128::MAX.saturated_into(); assets.len()]
        ));

        set_up_liquidity::<T>(market_id, signer.clone(), complete_set_amount);
        assert_ok!(NeoSwaps::<T>::join(
            RawOrigin::Signed(signer.clone()).into(),
            market_id,
            _10.saturated_into(),
            vec![u128::MAX.saturated_into(); assets.len()]
        ));

        let min_amounts_out = vec![0u8.into(); asset_count as usize];
        let block_number = frame_system::Pallet::<T>::block_number();
        let exit_shares_amount = _10.saturated_into();
        let relayer = get_relayer::<T>();
        let encoded_payload = encode_signed_exit_params::<T>(
            &relayer,
            &market_id,
            &exit_shares_amount,
            &min_amounts_out,
            &block_number,
        );
        let signature = signer_key_pair.sign(&encoded_payload).unwrap().encode();
        let proof = get_proof::<T>(signer.clone(), relayer, &signature);

        #[extrinsic_call]
        signed_exit(
            RawOrigin::Signed(signer.clone()),
            proof,
            market_id,
            exit_shares_amount,
            min_amounts_out,
            block_number,
        );
    }
}
