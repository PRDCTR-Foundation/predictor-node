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
use common_primitives::constants::{
    currency::{BASE, CENT_BASE},
    MILLISECS_PER_BLOCK,
};
use frame_benchmarking::{account, benchmarks};
use frame_support::traits::{EnsureOrigin, Get, Time, UnfilteredDispatchable};
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use orml_traits::MultiCurrency;
use pallet_pm_market_commons::MarketCommonsPalletApi;
use pallet_prediction_markets::{
    Call as PredictionMarketsCall, MarketIdsPerCloseTimeFrame, Pallet as PredictionMarkets,
    WhitelistedMarketCreators,
};
use parity_scale_codec::{Decode, Encode};
use prediction_market_primitives::{
    math::fixed::{BaseProvider, PredictionMarketBase},
    traits::InspectEthAsset,
    types::{
        Asset, Deadlines, EthAddress, MarketCreation, MarketDisputeMechanism, MarketPeriod,
        MarketType, MultiHash, OutcomeReport, ScoringRule,
    },
};
use sp_avn_common::Proof;
use sp_core::{crypto::DEV_PHRASE, ByteArray, H160, H256};
use sp_runtime::{
    traits::{SaturatedConversion, Zero},
    DispatchError, Perbill, RuntimeAppPublic,
};

const LIQUIDITY: u128 = 100 * BASE;

type BalanceOf<T> = <T as pallet_pm_market_commons::Config>::Balance;
type MarketIdOf<T> = <T as pallet_pm_market_commons::Config>::MarketId;
type MomentOf<T> = <<T as pallet_pm_market_commons::Config>::Timestamp as Time>::Moment;

fn calculate_time_frame_of_moment<T: Config>(time: MomentOf<T>) -> u64 {
    time.saturated_into::<u64>().saturating_div(MILLISECS_PER_BLOCK.into())
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
    let account_id = T::AccountId::decode(&mut &account_bytes.encode()[..]).unwrap();
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

fn sign_payload<T>(
    signer: T::AccountId,
    relayer: T::AccountId,
    key_pair: &<T as pallet_avn::Config>::AuthorityId,
    payload: &[u8],
) -> Proof<T::Signature, T::AccountId>
where
    T: Config + pallet_avn::Config,
{
    let message = payload.to_vec();
    let signature = key_pair.sign(&message).unwrap().encode();
    get_proof::<T>(signer, relayer, &signature)
}

fn create_spot_prices<T: Config>(asset_count: u16) -> Vec<BalanceOf<T>> {
    let mut result = vec![CENT_BASE.saturated_into(); (asset_count - 1) as usize];
    let remaining =
        PredictionMarketBase::<u128>::get().unwrap() - (asset_count - 1) as u128 * CENT_BASE;
    result.push(remaining.saturated_into());
    result
}

fn create_market_common_parameters<T: Config>(
    is_disputable: bool,
    caller: T::AccountId,
) -> (T::AccountId, Deadlines<BlockNumberFor<T>>, MultiHash) {
    T::AssetManager::deposit(Asset::Prd, &caller, (100_000u128 * BASE).saturated_into()).unwrap();
    let oracle = caller.clone();
    let deadlines = Deadlines::<BlockNumberFor<T>> {
        grace_period: 1_u32.into(),
        oracle_duration: T::MinOracleDuration::get(),
        dispute_duration: if is_disputable { T::MinDisputeDuration::get() } else { Zero::zero() },
    };
    let mut metadata = [0u8; 50];
    metadata[0] = 0x15;
    metadata[1] = 0x30;
    (oracle, deadlines, MultiHash::Sha3_384(metadata))
}

fn create_market_common<T>(
    caller: T::AccountId,
    market_type: MarketType,
    dispute_mechanism: Option<MarketDisputeMechanism>,
) -> Result<MarketIdOf<T>, DispatchError>
where
    T: Config + pallet_timestamp::Config,
{
    pallet_timestamp::Pallet::<T>::set_timestamp(0u32.into());
    let range_start: MomentOf<T> = 100_000u64.saturated_into();
    let range_end: MomentOf<T> = 1_000_000u64.saturated_into();
    let (oracle, deadlines, metadata) =
        create_market_common_parameters::<T>(dispute_mechanism.is_some(), caller.clone());
    WhitelistedMarketCreators::<T>::insert(&caller, ());
    PredictionMarketsCall::<T>::create_market {
        base_asset: Asset::Prd,
        creator_fee: Perbill::zero(),
        oracle,
        period: MarketPeriod::Timestamp(range_start..range_end),
        deadlines,
        metadata,
        creation: MarketCreation::Permissionless,
        market_type,
        dispute_mechanism,
        scoring_rule: ScoringRule::AmmCdaHybrid,
    }
    .dispatch_bypass_filter(RawOrigin::Signed(caller).into())
    .map_err(|e| e.error)?;
    pallet_pm_market_commons::Pallet::<T>::latest_market_id()
}

fn do_report_market_with_dispute_mechanism<T>(
    m: u32,
    caller: T::AccountId,
    dispute_mechanism: Option<MarketDisputeMechanism>,
    expire_reporting_period: bool,
) -> Result<MarketIdOf<T>, DispatchError>
where
    T: Config + pallet_timestamp::Config,
{
    let range_start: MomentOf<T> = pallet_pm_market_commons::Pallet::<T>::now();
    let range_end: MomentOf<T> = 1_000_000u64.saturated_into();
    let (oracle, deadlines, metadata) =
        create_market_common_parameters::<T>(dispute_mechanism.is_some(), caller.clone());
    WhitelistedMarketCreators::<T>::insert(&caller, ());
    PredictionMarketsCall::<T>::create_market {
        base_asset: Asset::Prd,
        creator_fee: Perbill::zero(),
        oracle: oracle.clone(),
        period: MarketPeriod::Timestamp(range_start..range_end),
        deadlines,
        metadata,
        creation: MarketCreation::Permissionless,
        market_type: MarketType::Categorical(T::MaxCategories::get()),
        dispute_mechanism,
        scoring_rule: ScoringRule::AmmCdaHybrid,
    }
    .dispatch_bypass_filter(RawOrigin::Signed(caller.clone()).into())
    .map_err(|e| e.error)?;
    let market_id = pallet_pm_market_commons::Pallet::<T>::latest_market_id()?;

    pallet_pm_market_commons::Pallet::<T>::mutate_market(&market_id, |market| {
        market.oracle = caller.clone();
        Ok(())
    })?;

    let close_origin = T::CloseOrigin::try_successful_origin().unwrap();
    PredictionMarkets::<T>::admin_move_market_to_closed(close_origin, market_id)
        .map_err(|e| e.error)?;
    let market = pallet_pm_market_commons::Pallet::<T>::market(&market_id)?;
    let end: u32 = match market.period {
        MarketPeriod::Timestamp(range) => range.end.saturated_into::<u32>(),
        _ => return Err(DispatchError::Other("timestamp market expected")),
    };
    let mut end_period: u32 =
        (market.deadlines.grace_period.saturated_into::<u32>() + 1) * MILLISECS_PER_BLOCK;
    if expire_reporting_period {
        end_period = end_period.saturating_add(
            market.deadlines.oracle_duration.saturated_into::<u32>() * MILLISECS_PER_BLOCK,
        );
    }
    pallet_timestamp::Pallet::<T>::set_timestamp((end + end_period).into());

    for i in 0..m {
        pallet_prediction_markets::MarketIdsPerReportBlock::<T>::try_mutate(
            frame_system::Pallet::<T>::block_number(),
            |ids| ids.try_push((i + 1).into()),
        )
        .unwrap();
    }

    Ok(market_id)
}

fn do_report_trusted_market<T>(caller: T::AccountId) -> Result<MarketIdOf<T>, DispatchError>
where
    T: Config + pallet_timestamp::Config,
{
    pallet_timestamp::Pallet::<T>::set_timestamp(0u32.into());
    let start: MomentOf<T> = pallet_pm_market_commons::Pallet::<T>::now();
    let end: MomentOf<T> = 1_000_000u64.saturated_into();
    let (_oracle, _deadlines, metadata) =
        create_market_common_parameters::<T>(false, caller.clone());
    WhitelistedMarketCreators::<T>::insert(&caller, ());
    PredictionMarketsCall::<T>::create_market {
        base_asset: Asset::Prd,
        creator_fee: Perbill::zero(),
        oracle: caller.clone(),
        period: MarketPeriod::Timestamp(start..end),
        deadlines: Deadlines::<BlockNumberFor<T>> {
            grace_period: 0u8.into(),
            oracle_duration: T::MinOracleDuration::get(),
            dispute_duration: 0u8.into(),
        },
        metadata,
        creation: MarketCreation::Permissionless,
        market_type: MarketType::Categorical(3),
        dispute_mechanism: None,
        scoring_rule: ScoringRule::AmmCdaHybrid,
    }
    .dispatch_bypass_filter(RawOrigin::Signed(caller).into())
    .map_err(|e| e.error)?;
    let market_id = pallet_pm_market_commons::Pallet::<T>::latest_market_id()?;
    let close_origin = T::CloseOrigin::try_successful_origin().unwrap();
    PredictionMarkets::<T>::admin_move_market_to_closed(close_origin, market_id)
        .map_err(|e| e.error)?;
    Ok(market_id)
}

fn setup_redeem_shares_common<T>(
    caller: T::AccountId,
    market_type: MarketType,
) -> Result<MarketIdOf<T>, DispatchError>
where
    T: Config + pallet_timestamp::Config,
{
    let market_id = create_market_common::<T>(
        caller.clone(),
        market_type.clone(),
        Some(MarketDisputeMechanism::Court),
    )?;
    let outcome = match market_type {
        MarketType::Categorical(categories) =>
            OutcomeReport::Categorical(categories.saturating_sub(1)),
        MarketType::Scalar(range) => OutcomeReport::Scalar(*range.end()),
    };

    PredictionMarketsCall::<T>::buy_complete_set { market_id, amount: LIQUIDITY.saturated_into() }
        .dispatch_bypass_filter(RawOrigin::Signed(caller.clone()).into())
        .map_err(|e| e.error)?;
    PredictionMarketsCall::<T>::admin_move_market_to_closed { market_id }
        .dispatch_bypass_filter(T::CloseOrigin::try_successful_origin().unwrap())
        .map_err(|e| e.error)?;
    let market = pallet_pm_market_commons::Pallet::<T>::market(&market_id)?;
    let end: u32 = match market.period {
        MarketPeriod::Timestamp(range) => range.end.saturated_into::<u32>(),
        _ => return Err(DispatchError::Other("timestamp market expected")),
    };
    let grace_period: u32 =
        (market.deadlines.grace_period.saturated_into::<u32>() + 1) * MILLISECS_PER_BLOCK;
    pallet_timestamp::Pallet::<T>::set_timestamp((end + grace_period).into());
    PredictionMarketsCall::<T>::report { market_id, outcome }
        .dispatch_bypass_filter(RawOrigin::Signed(caller).into())
        .map_err(|e| e.error)?;
    PredictionMarketsCall::<T>::admin_move_market_to_resolved { market_id }
        .dispatch_bypass_filter(T::ResolveOrigin::try_successful_origin().unwrap())
        .map_err(|e| e.error)?;
    Ok(market_id)
}

benchmarks! {
    where_clause {
        where
            T: pallet_avn::Config + pallet_timestamp::Config,
    }

    signed_create_market_and_deploy_pool {
        let m in 0..63;
        let n in 2..T::MaxCategories::get() as u32;

        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let base_asset = Asset::Prd;
        let range_start = (5 * MILLISECS_PER_BLOCK) as u64;
        let range_end = (100 * MILLISECS_PER_BLOCK) as u64;
        let period = MarketPeriod::Timestamp(range_start..range_end);
        let asset_count = n.try_into().unwrap();
        let market_type = MarketType::Categorical(asset_count);
        let (oracle, deadlines, metadata) = create_market_common_parameters::<T>(true, caller.clone());
        let amount = (10u128 * BASE).saturated_into();

        T::AssetManager::deposit(base_asset, &caller, amount)?;
        WhitelistedMarketCreators::<T>::insert(&caller, ());
        for i in 0..m {
            MarketIdsPerCloseTimeFrame::<T>::try_mutate(
                calculate_time_frame_of_moment::<T>(range_end.into()),
                |ids| ids.try_push(i.into()),
            ).unwrap();
        }

        let spot_prices = create_spot_prices::<T>(asset_count);
        let swap_fee: BalanceOf<T> = CENT_BASE.saturated_into();
        let creator_fee = Perbill::zero();
        let dispute_mechanism = Some(MarketDisputeMechanism::Court);
        let signed_payload = encode_signed_create_market_and_deploy_pool_params::<T>(
            &relayer,
            0u64,
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
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: _(RawOrigin::Signed(caller), proof, base_asset, creator_fee, oracle, period, deadlines, metadata, market_type, dispute_mechanism, amount, spot_prices, swap_fee)

    signed_transfer_asset {
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let token: EthAddress = H160::from([1u8; 20]);
        let asset = T::AssetRegistry::asset_id(&token).unwrap();
        T::AssetManager::deposit(asset, &caller, (10_000u128 * LIQUIDITY).saturated_into()).unwrap();
        let recipient: T::AccountId = account("Recipient", 0, 0);
        let amount: BalanceOf<T> = (1_000u128 * LIQUIDITY).saturated_into();
        let signed_payload = encode_signed_transfer_params::<T>(
            &relayer,
            &0u64,
            &token,
            &caller,
            &recipient,
            &amount,
        );
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: _(RawOrigin::Signed(caller.clone()), proof, token, recipient.clone(), amount)
    verify {
        let recipient_balance = T::AssetManager::free_balance(asset, &recipient);
        assert_eq!(recipient_balance, amount);
    }

    signed_withdraw_tokens {
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let token: EthAddress = H160::from([1u8; 20]);
        let asset = T::AssetRegistry::asset_id(&token).unwrap();
        let initial_balance: BalanceOf<T> = (10_000u128 * LIQUIDITY).saturated_into();
        T::AssetManager::deposit(asset, &caller, initial_balance).unwrap();
        let amount: BalanceOf<T> = (1_000u128 * LIQUIDITY).saturated_into();
        let signed_payload =
            encode_signed_withdraw_params::<T>(&relayer, &0u64, &token, &caller, &amount);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: _(RawOrigin::Signed(caller.clone()), proof, token, amount)
    verify {
        let owner_balance = T::AssetManager::free_balance(asset, &caller);
        assert_eq!(owner_balance, initial_balance - amount);
    }

    signed_report_market_with_dispute_mechanism {
        let m in 0..63;
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let outcome = OutcomeReport::Categorical(0);
        let market_id = do_report_market_with_dispute_mechanism::<T>(
            m,
            caller.clone(),
            Some(MarketDisputeMechanism::Court),
            false,
        )?;
        let signed_payload =
            encode_signed_report_params::<T>(&relayer, &0u64, &market_id, &outcome);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: signed_report(RawOrigin::Signed(caller), proof, market_id, outcome)

    signed_report_trusted_market {
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let market_id = do_report_trusted_market::<T>(caller.clone())?;
        let outcome = OutcomeReport::Categorical(0);
        let nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        let signed_payload =
            encode_signed_report_params::<T>(&relayer, &nonce, &market_id, &outcome);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: signed_report(RawOrigin::Signed(caller.clone()), proof, market_id, outcome)
    verify {
        let new_nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        assert_eq!(new_nonce, nonce + 1);
    }

    signed_redeem_shares_categorical {
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();
        let market_id = setup_redeem_shares_common::<T>(
            caller.clone(),
            MarketType::Categorical(T::MaxCategories::get()),
        )?;
        let nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        let signed_payload = encode_signed_redeem_shares_params::<T>(&relayer, &nonce, &market_id);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: signed_redeem_shares(RawOrigin::Signed(caller.clone()), proof, market_id)
    verify {
        let new_nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        assert_eq!(new_nonce, nonce + 1);
    }

    signed_redeem_shares_scalar {
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();
        let market_id = setup_redeem_shares_common::<T>(
            caller.clone(),
            MarketType::Scalar(0u128..=u128::MAX),
        )?;
        let nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        let signed_payload = encode_signed_redeem_shares_params::<T>(&relayer, &nonce, &market_id);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: signed_redeem_shares(RawOrigin::Signed(caller.clone()), proof, market_id)
    verify {
        let new_nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        assert_eq!(new_nonce, nonce + 1);
    }

    signed_buy_complete_set {
        let a in (T::MinCategories::get().into())..T::MaxCategories::get().into();
        let relayer = get_relayer::<T>();
        let (caller_key_pair, caller) = get_user_account::<T>();

        let market_id = create_market_common::<T>(
            caller.clone(),
            MarketType::Categorical(a.saturated_into()),
            Some(MarketDisputeMechanism::Court),
        )?;
        let nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        let amount = (10u128 * BASE).saturated_into();
        let signed_payload =
            encode_signed_buy_complete_set_params::<T>(&relayer, &nonce, &market_id, &amount);
        let proof = sign_payload::<T>(caller.clone(), relayer, &caller_key_pair, &signed_payload);
    }: signed_buy_complete_set(RawOrigin::Signed(caller.clone()), proof, market_id, amount)
    verify {
        let new_nonce = MarketNonces::<T>::get(caller.clone(), market_id);
        assert_eq!(new_nonce, nonce + 1);
    }
}
