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
use frame_benchmarking::v2::*;
use frame_support::{
    assert_ok,
    storage::{with_transaction, TransactionOutcome::*},
};
use frame_system::RawOrigin;
use orml_traits::MultiCurrency;
use pallet_pm_hybrid_router::types::Strategy;
use pallet_pm_market_commons::MarketCommonsPalletApi;
use parity_scale_codec::{Decode, Encode};
use prediction_market_primitives::{
    constants::base_multiples::*,
    math::fixed::{BaseProvider, FixedDiv, PredictionMarketBase},
    orderbook::OrderId,
    traits::{CompleteSetOperationsApi, DeployPoolApi, HybridRouterOrderbookApi},
    types::{Asset, Market, MarketCreation, MarketPeriod, MarketStatus, MarketType, ScoringRule},
};
use sp_avn_common::Proof;
use sp_core::{crypto::DEV_PHRASE, ByteArray, H256};
use sp_runtime::{Perbill, RuntimeAppPublic, SaturatedConversion};

type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type BalanceOf<T> = <<T as pallet_pm_hybrid_router::Config>::AssetManager as MultiCurrency<
    AccountIdOf<T>,
>>::Balance;
type MarketIdOf<T> =
    <<T as pallet_pm_hybrid_router::Config>::MarketCommons as MarketCommonsPalletApi>::MarketId;
type AssetOf<T> = Asset<MarketIdOf<T>>;

macro_rules! assert_ok_with_transaction {
    ($expr:expr) => {{
        assert_ok!(with_transaction(|| match $expr {
            Ok(val) => Commit(Ok(val)),
            Err(err) => Rollback(Err(err)),
        }));
    }};
}

fn create_spot_prices<T: Config>(asset_count: u16) -> Vec<BalanceOf<T>> {
    let base = PredictionMarketBase::<u128>::get().unwrap();
    let amount = base / asset_count as u128;
    let remainder = (base % asset_count as u128).saturated_into::<BalanceOf<T>>();

    let mut amounts = vec![amount.saturated_into::<BalanceOf<T>>(); asset_count as usize];
    amounts[0] += remainder;

    amounts
}

fn create_market<T>(
    caller: AccountIdOf<T>,
    base_asset: AssetOf<T>,
    asset_count: u16,
) -> MarketIdOf<T>
where
    T: Config,
{
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

    <T as pallet_pm_hybrid_router::Config>::MarketCommons::push_market(market).unwrap()
}

fn create_market_and_deploy_pool<T: Config>(
    caller: AccountIdOf<T>,
    base_asset: AssetOf<T>,
    asset_count: u16,
    amount: BalanceOf<T>,
) -> MarketIdOf<T> {
    let market_id = create_market::<T>(caller.clone(), base_asset, asset_count);
    let total_cost =
        amount + <T as pallet_pm_hybrid_router::Config>::AssetManager::minimum_balance(base_asset);
    assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
        base_asset, &caller, total_cost
    ));
    assert_ok_with_transaction!(
        <T as pallet_pm_hybrid_router::Config>::CompleteSetOperations::buy_complete_set(
            caller.clone(),
            market_id,
            amount
        )
    );
    assert_ok_with_transaction!(
        <T as pallet_pm_hybrid_router::Config>::AmmPoolDeployer::deploy_pool(
            caller,
            market_id,
            amount,
            create_spot_prices::<T>(asset_count),
            _1_100.saturated_into(),
        )
    );

    market_id
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

#[benchmarks(where T: pallet_avn::Config + frame_system::Config)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn signed_buy(n: Linear<2, 16>, o: Linear<0, 10>) {
        let relayer = get_relayer::<T>();
        let (buyer_key_pair, buyer) = get_user_account::<T>();

        let base_asset = Asset::Prd;
        let asset_count = n.try_into().unwrap();
        let market_id = create_market_and_deploy_pool::<T>(
            buyer.clone(),
            base_asset,
            asset_count,
            _100.saturated_into(),
        );

        let asset = Asset::CategoricalOutcome(market_id, 0u16);
        let amount_in = _1000.saturated_into();
        assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
            base_asset, &buyer, amount_in
        ));

        let spot_prices = create_spot_prices::<T>(asset_count);
        let first_spot_price = spot_prices[0];

        let max_price = _9_10.saturated_into();
        let orders = (0u128..o as u128).collect::<Vec<OrderId>>();
        let maker_asset = asset;
        let maker_amount = _20.saturated_into();
        let taker_asset = base_asset;
        let taker_amount: BalanceOf<T> = _11.saturated_into();
        assert!(taker_amount.bdiv_floor(maker_amount).unwrap() > first_spot_price);
        for (i, order_id) in orders.iter().enumerate() {
            let order_creator: T::AccountId = account("order_creator", *order_id as u32, 0);
            let surplus = ((i + 1) as u128) * _1_2;
            let taker_amount = taker_amount + surplus.saturated_into::<BalanceOf<T>>();
            assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
                maker_asset,
                &order_creator,
                maker_amount
            ));
            assert_ok!(<T as pallet_pm_hybrid_router::Config>::Orderbook::place_order(
                order_creator,
                market_id,
                maker_asset,
                maker_amount,
                taker_asset,
                taker_amount,
            ));
        }

        let strategy = Strategy::LimitOrder;
        let signed_payload = encode_signed_buy_params::<T>(
            &relayer,
            0u64,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &max_price,
            &orders,
            &strategy,
        );
        let signature = buyer_key_pair.sign(&signed_payload).unwrap().encode();
        let proof = get_proof::<T>(buyer.clone(), relayer, &signature);

        #[extrinsic_call]
        signed_buy(
            RawOrigin::Signed(buyer.clone()),
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            max_price,
            orders,
            strategy,
        );

        let buyer_limit_order =
            <T as pallet_pm_hybrid_router::Config>::Orderbook::order(o as u128).unwrap();
        assert_eq!(buyer_limit_order.market_id, market_id);
        assert_eq!(buyer_limit_order.maker, buyer);
        assert_eq!(buyer_limit_order.maker_asset, base_asset);
        assert_eq!(buyer_limit_order.taker_asset, asset);
    }

    #[benchmark]
    fn signed_sell(n: Linear<2, 10>, o: Linear<0, 10>) {
        let relayer = get_relayer::<T>();
        let (seller_key_pair, seller) = get_user_account::<T>();

        let base_asset = Asset::Prd;
        let asset_count = n.try_into().unwrap();
        let market_id = create_market_and_deploy_pool::<T>(
            seller.clone(),
            base_asset,
            asset_count,
            _100.saturated_into(),
        );

        let asset = Asset::CategoricalOutcome(market_id, 0u16);
        let amount_in = (_1000 * 100).saturated_into();
        assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
            asset, &seller, amount_in
        ));
        let min_balance =
            <T as pallet_pm_hybrid_router::Config>::AssetManager::minimum_balance(base_asset);
        assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
            base_asset,
            &seller,
            min_balance
        ));

        let spot_prices = create_spot_prices::<T>(asset_count);
        let first_spot_price = spot_prices[0];

        let min_price = _1_100.saturated_into();
        let orders = (0u128..o as u128).collect::<Vec<OrderId>>();
        let maker_asset: AssetOf<T> = base_asset;
        let maker_amount: BalanceOf<T> = _9.saturated_into();
        let taker_asset = asset;
        let taker_amount = _100.saturated_into();
        assert!(maker_amount.bdiv_floor(taker_amount).unwrap() < first_spot_price);
        for (i, order_id) in orders.iter().enumerate() {
            let order_creator: T::AccountId = account("order_creator", *order_id as u32, 0);
            let surplus = ((i + 1) as u128) * _1_2;
            let taker_amount = taker_amount + surplus.saturated_into::<BalanceOf<T>>();
            assert_ok!(<T as pallet_pm_hybrid_router::Config>::AssetManager::deposit(
                maker_asset,
                &order_creator,
                maker_amount + _100.saturated_into()
            ));
            <T as pallet_pm_hybrid_router::Config>::Orderbook::place_order(
                order_creator,
                market_id,
                maker_asset,
                maker_amount,
                taker_asset,
                taker_amount,
            )
            .unwrap();
        }

        let strategy = Strategy::LimitOrder;
        let signed_payload = encode_signed_sell_params::<T>(
            &relayer,
            0u64,
            &market_id,
            &asset_count,
            &asset,
            &amount_in,
            &min_price,
            &orders,
            &strategy,
        );
        let signature = seller_key_pair.sign(&signed_payload).unwrap().encode();
        let proof = get_proof::<T>(seller.clone(), relayer, &signature);

        #[extrinsic_call]
        signed_sell(
            RawOrigin::Signed(seller.clone()),
            proof,
            market_id,
            asset_count,
            asset,
            amount_in,
            min_price,
            orders,
            strategy,
        );

        let seller_limit_order =
            <T as pallet_pm_hybrid_router::Config>::Orderbook::order(o as u128).unwrap();
        assert_eq!(seller_limit_order.market_id, market_id);
        assert_eq!(seller_limit_order.maker, seller);
        assert_eq!(seller_limit_order.maker_asset, asset);
        assert_eq!(seller_limit_order.taker_asset, base_asset);
    }
}
