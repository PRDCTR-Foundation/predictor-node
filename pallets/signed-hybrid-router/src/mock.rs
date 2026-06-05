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

use crate as pallet_pm_signed_hybrid_router;
#[cfg(feature = "runtime-benchmarks")]
use alloc::vec::Vec;
use frame_support::{
    derive_impl, parameter_types, storage::PrefixIterator, traits::Everything, weights::Weight,
    PalletId,
};
use frame_system as system;
use orml_traits::MultiCurrency;
use pallet_pm_hybrid_router::weights::WeightInfoZeitgeist;
use pallet_pm_market_commons::MarketCommonsPalletApi;
use parity_scale_codec::Decode;
#[cfg(feature = "runtime-benchmarks")]
use prediction_market_primitives::traits::{CompleteSetOperationsApi, DeployPoolApi};
use prediction_market_primitives::{
    hybrid_router_api_types::{AmmTrade, ApiError, OrderbookTrade},
    orderbook::{Order, OrderId},
    traits::{HybridRouterAmmApi, HybridRouterOrderbookApi},
    types::{Asset, Market, SignatureTest, TestAccountIdPK},
};
use sp_core::{sr25519, Pair};
use sp_runtime::{
    traits::{IdentityLookup, Verify},
    BuildStorage, DispatchError, DispatchResult,
};

pub type AccountId = TestAccountIdPK;
pub type Balance = u128;
pub type MarketId = u128;
pub type BlockNumber = u64;
pub type Moment = u64;
pub type AssetId = Asset<MarketId>;
pub type OrderOf = Order<AccountId, Balance, MarketId>;
pub type Block = frame_system::mocking::MockBlock<Runtime>;

frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system,
        HybridRouter: pallet_pm_hybrid_router,
        SignedHybridRouter: pallet_pm_signed_hybrid_router,
    }
);

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const HybridRouterPalletId: PalletId = PalletId(*b"pm/hybrd");
    pub const MaxOrders: u32 = 10;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig as frame_system::DefaultConfig)]
impl system::Config for Runtime {
    type Nonce = u64;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Block = Block;
    type BlockHashCount = BlockHashCount;
    type BaseCallFilter = Everything;
}

pub struct AssetManager;

impl MultiCurrency<AccountId> for AssetManager {
    type CurrencyId = AssetId;
    type Balance = Balance;

    fn minimum_balance(_currency_id: Self::CurrencyId) -> Self::Balance {
        0
    }

    fn total_issuance(_currency_id: Self::CurrencyId) -> Self::Balance {
        0
    }

    fn total_balance(_currency_id: Self::CurrencyId, _who: &AccountId) -> Self::Balance {
        0
    }

    fn free_balance(_currency_id: Self::CurrencyId, _who: &AccountId) -> Self::Balance {
        0
    }

    fn ensure_can_withdraw(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        Ok(())
    }

    fn transfer(
        _currency_id: Self::CurrencyId,
        _from: &AccountId,
        _to: &AccountId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        Ok(())
    }

    fn deposit(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        Ok(())
    }

    fn withdraw(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        Ok(())
    }

    fn can_slash(_currency_id: Self::CurrencyId, _who: &AccountId, _value: Self::Balance) -> bool {
        false
    }

    fn slash(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        amount: Self::Balance,
    ) -> Self::Balance {
        amount
    }
}

pub struct MarketCommons;

impl MarketCommonsPalletApi for MarketCommons {
    type AccountId = AccountId;
    type BlockNumber = BlockNumber;
    type Balance = Balance;
    type MarketId = MarketId;
    type Moment = Moment;

    fn latest_market_id() -> Result<Self::MarketId, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn market_iter() -> PrefixIterator<(Self::MarketId, MarketOf<Self>)> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn market(_market_id: &Self::MarketId) -> Result<MarketOf<Self>, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn mutate_market<F>(_market_id: &Self::MarketId, _cb: F) -> DispatchResult
    where
        F: FnOnce(&mut MarketOf<Self>) -> DispatchResult,
    {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn push_market(_market: MarketOf<Self>) -> Result<Self::MarketId, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn build_market<U>(
        _market_builder: U,
    ) -> Result<(Self::MarketId, MarketOf<Self>), DispatchError>
    where
        U: prediction_market_primitives::traits::MarketBuilderTrait<
            Self::AccountId,
            Self::Balance,
            Self::BlockNumber,
            Self::Moment,
            Self::MarketId,
        >,
    {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn remove_market(_market_id: &Self::MarketId) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn insert_market_pool(_market_id: Self::MarketId, _pool_id: u128) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn remove_market_pool(_market_id: &Self::MarketId) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn market_pool(_market_id: &Self::MarketId) -> Result<u128, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn now() -> Self::Moment {
        0
    }
}

type MarketOf<T> = Market<
    <T as MarketCommonsPalletApi>::AccountId,
    <T as MarketCommonsPalletApi>::Balance,
    <T as MarketCommonsPalletApi>::BlockNumber,
    <T as MarketCommonsPalletApi>::Moment,
    <T as MarketCommonsPalletApi>::MarketId,
>;

pub struct Amm;

impl HybridRouterAmmApi for Amm {
    type AccountId = AccountId;
    type Asset = AssetId;
    type Balance = Balance;
    type MarketId = MarketId;

    fn pool_exists(_market_id: Self::MarketId) -> bool {
        false
    }

    fn get_spot_price(
        _market_id: Self::MarketId,
        _asset: Self::Asset,
    ) -> Result<Self::Balance, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn calculate_buy_amount_until(
        _market_id: Self::MarketId,
        _asset: Self::Asset,
        _until: Self::Balance,
    ) -> Result<Self::Balance, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn buy(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _asset_out: Self::Asset,
        _amount_in: Self::Balance,
        _min_amount_out: Self::Balance,
    ) -> Result<
        AmmTrade<Self::Balance>,
        ApiError<prediction_market_primitives::hybrid_router_api_types::AmmSoftFail>,
    > {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn calculate_sell_amount_until(
        _market_id: Self::MarketId,
        _asset: Self::Asset,
        _until: Self::Balance,
    ) -> Result<Self::Balance, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn sell(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _asset_in: Self::Asset,
        _amount_in: Self::Balance,
        _min_amount_out: Self::Balance,
    ) -> Result<
        AmmTrade<Self::Balance>,
        ApiError<prediction_market_primitives::hybrid_router_api_types::AmmSoftFail>,
    > {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }
}

pub struct Orderbook;

impl HybridRouterOrderbookApi for Orderbook {
    type AccountId = AccountId;
    type Asset = AssetId;
    type Balance = Balance;
    type MarketId = MarketId;
    type Order = OrderOf;
    type OrderId = OrderId;

    fn order(_order_id: Self::OrderId) -> Result<Self::Order, DispatchError> {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn fill_order(
        _who: Self::AccountId,
        _order_id: Self::OrderId,
        _maker_partial_fill: Option<Self::Balance>,
    ) -> Result<
        OrderbookTrade<Self::AccountId, Self::Balance>,
        ApiError<prediction_market_primitives::hybrid_router_api_types::OrderbookSoftFail>,
    > {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn place_order(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _maker_asset: Self::Asset,
        _maker_amount: Self::Balance,
        _taker_asset: Self::Asset,
        _taker_amount: Self::Balance,
    ) -> Result<
        (),
        ApiError<prediction_market_primitives::hybrid_router_api_types::OrderbookSoftFail>,
    > {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }
}

pub struct HybridRouterWeightInfo;

impl WeightInfoZeitgeist for HybridRouterWeightInfo {
    fn buy(_n: u32, _o: u32) -> Weight {
        Weight::zero()
    }

    fn sell(_n: u32, _o: u32) -> Weight {
        Weight::zero()
    }
}

#[cfg(feature = "runtime-benchmarks")]
pub struct BenchmarkHooks;

#[cfg(feature = "runtime-benchmarks")]
impl DeployPoolApi for BenchmarkHooks {
    type AccountId = AccountId;
    type Balance = Balance;
    type MarketId = MarketId;

    fn deploy_pool(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _amount: Self::Balance,
        _swap_prices: Vec<Self::Balance>,
        _swap_fee: Self::Balance,
    ) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }
}

#[cfg(feature = "runtime-benchmarks")]
impl CompleteSetOperationsApi for BenchmarkHooks {
    type AccountId = AccountId;
    type Balance = Balance;
    type MarketId = MarketId;

    fn buy_complete_set(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }

    fn sell_complete_set(
        _who: Self::AccountId,
        _market_id: Self::MarketId,
        _amount: Self::Balance,
    ) -> DispatchResult {
        unimplemented!("not needed by signed-hybrid-router unit tests")
    }
}

impl pallet_pm_hybrid_router::Config for Runtime {
    type AssetManager = AssetManager;
    #[cfg(feature = "runtime-benchmarks")]
    type AmmPoolDeployer = BenchmarkHooks;
    #[cfg(feature = "runtime-benchmarks")]
    type CompleteSetOperations = BenchmarkHooks;
    type MarketCommons = MarketCommons;
    type Amm = Amm;
    type MaxOrders = MaxOrders;
    type Orderbook = Orderbook;
    type PalletId = HybridRouterPalletId;
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = HybridRouterWeightInfo;
}

pub struct SignedHybridRouterWeightInfo;

impl crate::weights::SignedWeightInfo for SignedHybridRouterWeightInfo {
    fn signed_buy(_n: u32, _o: u32) -> Weight {
        Weight::zero()
    }

    fn signed_sell(_n: u32, _o: u32) -> Weight {
        Weight::zero()
    }
}

impl crate::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type Public = <SignatureTest as Verify>::Signer;
    type Signature = SignatureTest;
    type WeightInfo = SignedHybridRouterWeightInfo;
}

pub fn account(seed: u8) -> AccountId {
    let pair = sr25519::Pair::from_seed(&[seed; 32]);
    AccountId::decode(&mut pair.public().to_vec().as_slice()).unwrap()
}

pub fn key_pair(seed: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[seed; 32])
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    frame_system::GenesisConfig::<Runtime>::default()
        .build_storage()
        .unwrap()
        .into()
}
