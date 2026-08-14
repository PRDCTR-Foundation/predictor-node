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

use crate as pallet_pm_signed_prediction_markets;
use alloc::{collections::btree_map::BTreeMap, vec, vec::Vec};
use core::cell::RefCell;
use frame_support::{
    derive_impl, ord_parameter_types, parameter_types,
    traits::{tokens::BalanceStatus, ConstU32, Everything, Time},
    weights::Weight,
    PalletId,
};
use frame_system::{self as system, EnsureSignedBy};
use orml_traits::{
    asset_registry::AssetMetadata, MultiCurrency, MultiReservableCurrency,
    NamedMultiReservableCurrency,
};
use pallet_pm_global_disputes::{types::InitialItem, GlobalDisputesPalletApi};
use parity_scale_codec::Decode;
use prediction_market_primitives::{
    traits::{
        DeployPoolApi, DisputeApi, DisputeMaxWeightApi, DistributeFees, InspectEthAsset,
        MarketOfDisputeApi,
    },
    types::{
        Asset, CustomMetadata, EthAddress, MarketId, OutcomeReport, ResultWithWeightInfo,
        SignatureTest, TestAccountIdPK,
    },
};
use sp_core::{sr25519, Pair, H160};
use sp_runtime::{
    traits::{IdentityLookup, Verify},
    BuildStorage, DispatchError, DispatchResult, Perbill,
};

pub type AccountId = TestAccountIdPK;
pub type Balance = u128;
pub type BlockNumber = u64;
pub type Moment = u64;
pub type AssetId = Asset<MarketId>;
pub type Block = frame_system::mocking::MockBlock<Runtime>;

pub const FOREIGN_ASSET: AssetId = Asset::ForeignAsset(100);
pub const TOKEN: EthAddress = H160([1u8; 20]);

thread_local! {
    static ASSET_BALANCES: RefCell<BTreeMap<(AssetId, AccountId), Balance>> =
        const { RefCell::new(BTreeMap::new()) };
}

frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system,
        Balances: pallet_balances,
        MarketCommons: pallet_pm_market_commons,
        PredictionMarkets: pallet_prediction_markets,
        SignedPredictionMarkets: pallet_pm_signed_prediction_markets,
    }
);

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const ExistentialDeposit: Balance = 1;
    pub const MaxLocks: u32 = 50;
    pub const MaxReserves: u32 = 50;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig as frame_system::DefaultConfig)]
impl system::Config for Runtime {
    type AccountData = pallet_balances::AccountData<Balance>;
    type AccountId = AccountId;
    type BaseCallFilter = Everything;
    type Block = Block;
    type BlockHashCount = BlockHashCount;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Nonce = u64;
}

impl pallet_balances::Config for Runtime {
    type AccountStore = System;
    type Balance = Balance;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type FreezeIdentifier = ();
    type MaxFreezes = ();
    type MaxLocks = MaxLocks;
    type MaxReserves = MaxReserves;
    type ReserveIdentifier = [u8; 8];
    type RuntimeEvent = RuntimeEvent;
    type RuntimeFreezeReason = ();
    type RuntimeHoldReason = ();
    type WeightInfo = ();
}

pub struct MockTime;

impl Time for MockTime {
    type Moment = Moment;

    fn now() -> Self::Moment {
        0
    }
}

impl pallet_pm_market_commons::Config for Runtime {
    type Balance = Balance;
    type MarketId = MarketId;
    type Timestamp = MockTime;
}

pub struct AssetManager;

impl AssetManager {
    pub fn set_balance(asset: AssetId, who: &AccountId, amount: Balance) {
        ASSET_BALANCES.with(|balances| {
            balances.borrow_mut().insert((asset, *who), amount);
        });
    }
}

impl MultiCurrency<AccountId> for AssetManager {
    type Balance = Balance;
    type CurrencyId = AssetId;

    fn minimum_balance(_currency_id: Self::CurrencyId) -> Self::Balance {
        0
    }

    fn total_issuance(currency_id: Self::CurrencyId) -> Self::Balance {
        ASSET_BALANCES.with(|balances| {
            balances
                .borrow()
                .iter()
                .filter_map(|((asset, _), balance)| (*asset == currency_id).then_some(*balance))
                .sum()
        })
    }

    fn total_balance(currency_id: Self::CurrencyId, who: &AccountId) -> Self::Balance {
        Self::free_balance(currency_id, who)
    }

    fn free_balance(currency_id: Self::CurrencyId, who: &AccountId) -> Self::Balance {
        ASSET_BALANCES.with(|balances| {
            balances.borrow().get(&(currency_id, *who)).copied().unwrap_or_default()
        })
    }

    fn ensure_can_withdraw(
        currency_id: Self::CurrencyId,
        who: &AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        if Self::free_balance(currency_id, who) >= amount {
            Ok(())
        } else {
            Err(DispatchError::Other("insufficient asset balance"))
        }
    }

    fn transfer(
        currency_id: Self::CurrencyId,
        from: &AccountId,
        to: &AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        Self::withdraw(currency_id, from, amount)?;
        Self::deposit(currency_id, to, amount)
    }

    fn deposit(
        currency_id: Self::CurrencyId,
        who: &AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        ASSET_BALANCES.with(|balances| {
            let mut balances = balances.borrow_mut();
            let balance = balances.entry((currency_id, *who)).or_default();
            *balance = balance.saturating_add(amount);
        });
        Ok(())
    }

    fn withdraw(
        currency_id: Self::CurrencyId,
        who: &AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        Self::ensure_can_withdraw(currency_id, who, amount)?;
        ASSET_BALANCES.with(|balances| {
            let mut balances = balances.borrow_mut();
            let balance = balances.entry((currency_id, *who)).or_default();
            *balance = balance.saturating_sub(amount);
        });
        Ok(())
    }

    fn can_slash(currency_id: Self::CurrencyId, who: &AccountId, value: Self::Balance) -> bool {
        Self::free_balance(currency_id, who) >= value
    }

    fn slash(
        currency_id: Self::CurrencyId,
        who: &AccountId,
        amount: Self::Balance,
    ) -> Self::Balance {
        let slashable = Self::free_balance(currency_id, who).min(amount);
        let _ = Self::withdraw(currency_id, who, slashable);
        amount.saturating_sub(slashable)
    }
}

impl MultiReservableCurrency<AccountId> for AssetManager {
    fn can_reserve(currency_id: Self::CurrencyId, who: &AccountId, value: Self::Balance) -> bool {
        Self::free_balance(currency_id, who) >= value
    }

    fn slash_reserved(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        value: Self::Balance,
    ) -> Self::Balance {
        value
    }

    fn reserved_balance(_currency_id: Self::CurrencyId, _who: &AccountId) -> Self::Balance {
        0
    }

    fn reserve(
        currency_id: Self::CurrencyId,
        who: &AccountId,
        value: Self::Balance,
    ) -> DispatchResult {
        Self::ensure_can_withdraw(currency_id, who, value)
    }

    fn unreserve(
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        value: Self::Balance,
    ) -> Self::Balance {
        value
    }

    fn repatriate_reserved(
        _currency_id: Self::CurrencyId,
        _slashed: &AccountId,
        _beneficiary: &AccountId,
        value: Self::Balance,
        _status: BalanceStatus,
    ) -> Result<Self::Balance, DispatchError> {
        Ok(value)
    }
}

impl NamedMultiReservableCurrency<AccountId> for AssetManager {
    type ReserveIdentifier = [u8; 8];

    fn slash_reserved_named(
        _id: &Self::ReserveIdentifier,
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        value: Self::Balance,
    ) -> Self::Balance {
        value
    }

    fn reserved_balance_named(
        _id: &Self::ReserveIdentifier,
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
    ) -> Self::Balance {
        0
    }

    fn reserve_named(
        _id: &Self::ReserveIdentifier,
        currency_id: Self::CurrencyId,
        who: &AccountId,
        value: Self::Balance,
    ) -> DispatchResult {
        Self::ensure_can_withdraw(currency_id, who, value)
    }

    fn unreserve_named(
        _id: &Self::ReserveIdentifier,
        _currency_id: Self::CurrencyId,
        _who: &AccountId,
        value: Self::Balance,
    ) -> Self::Balance {
        value
    }

    fn repatriate_reserved_named(
        _id: &Self::ReserveIdentifier,
        _currency_id: Self::CurrencyId,
        _slashed: &AccountId,
        _beneficiary: &AccountId,
        value: Self::Balance,
        _status: BalanceStatus,
    ) -> Result<Self::Balance, DispatchError> {
        Ok(value)
    }
}

pub struct AssetRegistry;

impl InspectEthAsset for AssetRegistry {
    type AssetId = AssetId;
    type Balance = Balance;
    type CustomMetadata = CustomMetadata;
    type StringLimit = ConstU32<1024>;

    fn asset_id(eth_address: &EthAddress) -> Option<Self::AssetId> {
        (*eth_address == TOKEN).then_some(FOREIGN_ASSET)
    }

    fn metadata(
        asset_id: &Self::AssetId,
    ) -> Option<AssetMetadata<Self::Balance, Self::CustomMetadata, Self::StringLimit>> {
        (*asset_id == FOREIGN_ASSET).then_some(AssetMetadata {
            decimals: 18,
            name: b"Token".to_vec().try_into().unwrap(),
            symbol: b"TOK".to_vec().try_into().unwrap(),
            existential_deposit: 0,
            location: None,
            additional: CustomMetadata { eth_address: TOKEN, allow_as_base_asset: true },
        })
    }

    fn metadata_by_eth_address(
        eth_address: &EthAddress,
    ) -> Option<AssetMetadata<Self::Balance, Self::CustomMetadata, Self::StringLimit>> {
        Self::asset_id(eth_address).and_then(|asset_id| Self::metadata(&asset_id))
    }

    fn eth_address_by_asset_id(asset_id: &Self::AssetId) -> Option<EthAddress> {
        (*asset_id == FOREIGN_ASSET).then_some(TOKEN)
    }
}

pub struct NoopDispute;

impl DisputeApi for NoopDispute {
    type AccountId = AccountId;
    type Balance = Balance;
    type BlockNumber = BlockNumber;
    type MarketId = MarketId;
    type Moment = Moment;
    type NegativeImbalance =
        <Balances as frame_support::traits::Currency<AccountId>>::NegativeImbalance;
    type Origin = RuntimeOrigin;

    fn on_dispute(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> Result<ResultWithWeightInfo<()>, DispatchError> {
        Ok(ResultWithWeightInfo { result: (), weight: Weight::zero() })
    }

    fn on_resolution(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> Result<ResultWithWeightInfo<Option<OutcomeReport>>, DispatchError> {
        Ok(ResultWithWeightInfo { result: None, weight: Weight::zero() })
    }

    fn exchange(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
        _resolved_outcome: &OutcomeReport,
        amount: Self::NegativeImbalance,
    ) -> Result<ResultWithWeightInfo<Self::NegativeImbalance>, DispatchError> {
        Ok(ResultWithWeightInfo { result: amount, weight: Weight::zero() })
    }

    fn get_auto_resolve(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> ResultWithWeightInfo<Option<Self::BlockNumber>> {
        ResultWithWeightInfo { result: None, weight: Weight::zero() }
    }

    fn has_failed(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> Result<ResultWithWeightInfo<bool>, DispatchError> {
        Ok(ResultWithWeightInfo { result: false, weight: Weight::zero() })
    }

    fn on_global_dispute(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> Result<
        ResultWithWeightInfo<
            Vec<prediction_market_primitives::types::GlobalDisputeItem<AccountId, Balance>>,
        >,
        DispatchError,
    > {
        Ok(ResultWithWeightInfo { result: vec![], weight: Weight::zero() })
    }

    fn clear(
        _market_id: &Self::MarketId,
        _market: &MarketOfDisputeApi<Self>,
    ) -> Result<ResultWithWeightInfo<()>, DispatchError> {
        Ok(ResultWithWeightInfo { result: (), weight: Weight::zero() })
    }
}

impl DisputeMaxWeightApi for NoopDispute {
    fn on_dispute_max_weight() -> Weight {
        Weight::zero()
    }

    fn on_resolution_max_weight() -> Weight {
        Weight::zero()
    }

    fn exchange_max_weight() -> Weight {
        Weight::zero()
    }

    fn get_auto_resolve_max_weight() -> Weight {
        Weight::zero()
    }

    fn has_failed_max_weight() -> Weight {
        Weight::zero()
    }

    fn on_global_dispute_max_weight() -> Weight {
        Weight::zero()
    }

    fn clear_max_weight() -> Weight {
        Weight::zero()
    }
}

impl pallet_pm_authorized::AuthorizedPalletApi for NoopDispute {}
impl pallet_pm_court::CourtPalletApi for NoopDispute {}

pub struct NoopGlobalDisputes;

impl GlobalDisputesPalletApi<MarketId, AccountId, Balance, BlockNumber> for NoopGlobalDisputes {
    fn get_add_outcome_period() -> BlockNumber {
        0
    }

    fn get_vote_period() -> BlockNumber {
        0
    }

    fn start_global_dispute(
        _market_id: &MarketId,
        _initial_items: &[InitialItem<AccountId, Balance>],
    ) -> Result<u32, DispatchError> {
        Ok(0)
    }

    fn determine_voting_winner(_market_id: &MarketId) -> Option<OutcomeReport> {
        None
    }

    fn does_exist(_market_id: &MarketId) -> bool {
        false
    }

    fn is_active(_market_id: &MarketId) -> bool {
        false
    }

    fn destroy_global_dispute(_market_id: &MarketId) -> Result<(), DispatchError> {
        Ok(())
    }
}

pub struct NoopDeployPool;

impl DeployPoolApi for NoopDeployPool {
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
        Ok(())
    }
}

pub struct NoopFees;

impl DistributeFees for NoopFees {
    type AccountId = AccountId;
    type Asset = AssetId;
    type Balance = Balance;
    type MarketId = MarketId;

    fn distribute(
        _market_id: Self::MarketId,
        _asset: Self::Asset,
        _account: &Self::AccountId,
        _amount: Self::Balance,
    ) -> Self::Balance {
        0
    }
}

pub struct NoopTokenInterface;

impl sp_avn_common::event_types::TokenInterface<EthAddress, AccountId> for NoopTokenInterface {
    fn process_lift(_event: &sp_avn_common::event_types::EthEvent) -> DispatchResult {
        Ok(())
    }

    fn deposit_tokens(
        _token_id: EthAddress,
        _recipient_account_id: AccountId,
        _raw_amount: u128,
    ) -> DispatchResult {
        Ok(())
    }
}

ord_parameter_types! {
    pub const AdminOrigin: AccountId = account(99);
}

parameter_types! {
    pub const AdvisoryBond: Balance = 1;
    pub const AdvisoryBondSlashPercentage: sp_arithmetic::per_things::Percent = sp_arithmetic::per_things::Percent::from_percent(10);
    pub const CloseEarlyBlockPeriod: BlockNumber = 1;
    pub const CloseEarlyDisputeBond: Balance = 1;
    pub const CloseEarlyProtectionBlockPeriod: BlockNumber = 1;
    pub const CloseEarlyProtectionTimeFramePeriod: Moment = 1;
    pub const CloseEarlyRequestBond: Balance = 1;
    pub const CloseEarlyTimeFramePeriod: Moment = 1;
    pub const DisputeBond: Balance = 1;
    pub const MaxCategories: u16 = 10;
    pub const MaxCreatorFee: Perbill = Perbill::from_percent(1);
    pub const MaxDisputeDuration: BlockNumber = 10;
    pub const MaxDisputes: u32 = 3;
    pub const MaxEditReasonLen: u32 = 1024;
    pub const MaxGracePeriod: BlockNumber = 10;
    pub const MaxMarketLifetime: BlockNumber = 1_000;
    pub const MaxOracleDuration: BlockNumber = 10;
    pub const MaxRejectReasonLen: u32 = 1024;
    pub const MinCategories: u16 = 2;
    pub const MinDisputeDuration: BlockNumber = 1;
    pub const MinOracleDuration: BlockNumber = 1;
    pub const OracleBond: Balance = 1;
    pub const OutsiderBond: Balance = 1;
    pub const PmPalletId: PalletId = PalletId(*b"pm/preds");
    pub const ValidityBond: Balance = 1;
    pub const WinnerFeePercentage: Perbill = Perbill::zero();
}

impl pallet_prediction_markets::Config for Runtime {
    type AdvisoryBond = AdvisoryBond;
    type AdvisoryBondSlashPercentage = AdvisoryBondSlashPercentage;
    type ApproveOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type AssetManager = AssetManager;
    type AssetRegistry = AssetRegistry;
    type Authorized = NoopDispute;
    type CloseEarlyBlockPeriod = CloseEarlyBlockPeriod;
    type CloseEarlyDisputeBond = CloseEarlyDisputeBond;
    type CloseEarlyProtectionBlockPeriod = CloseEarlyProtectionBlockPeriod;
    type CloseEarlyProtectionTimeFramePeriod = CloseEarlyProtectionTimeFramePeriod;
    type CloseEarlyRequestBond = CloseEarlyRequestBond;
    type CloseEarlyTimeFramePeriod = CloseEarlyTimeFramePeriod;
    type CloseMarketEarlyOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type CloseOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type Court = NoopDispute;
    type Currency = Balances;
    type DeployPool = NoopDeployPool;
    type DisputeBond = DisputeBond;
    type GlobalDisputes = NoopGlobalDisputes;
    type MaxCategories = MaxCategories;
    type MaxCreatorFee = MaxCreatorFee;
    type MaxDisputeDuration = MaxDisputeDuration;
    type MaxDisputes = MaxDisputes;
    type MaxEditReasonLen = MaxEditReasonLen;
    type MaxGracePeriod = MaxGracePeriod;
    type MaxMarketLifetime = MaxMarketLifetime;
    type MaxOracleDuration = MaxOracleDuration;
    type MaxRejectReasonLen = MaxRejectReasonLen;
    type MinCategories = MinCategories;
    type MinDisputeDuration = MinDisputeDuration;
    type MinOracleDuration = MinOracleDuration;
    type OracleBond = OracleBond;
    type OutsiderBond = OutsiderBond;
    type PalletId = PmPalletId;
    type RejectOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type RequestEditOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type ResolveOrigin = EnsureSignedBy<AdminOrigin, AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type Slash = ();
    type TokenInterface = NoopTokenInterface;
    type ValidityBond = ValidityBond;
    type WeightInfo = pallet_prediction_markets::weights::WeightInfo<Runtime>;
    type WinnerFeeHandler = NoopFees;
    type WinnerFeePercentage = WinnerFeePercentage;
}

pub struct SignedPredictionMarketsWeightInfo;

impl crate::weights::SignedWeightInfo for SignedPredictionMarketsWeightInfo {
    fn signed_create_market_and_deploy_pool(_m: u32, _n: u32) -> Weight {
        Weight::zero()
    }

    fn signed_report_market_with_dispute_mechanism(_m: u32) -> Weight {
        Weight::zero()
    }

    fn signed_report_trusted_market() -> Weight {
        Weight::zero()
    }

    fn signed_withdraw_tokens() -> Weight {
        Weight::zero()
    }

    fn signed_transfer_asset() -> Weight {
        Weight::zero()
    }

    fn signed_redeem_shares_categorical() -> Weight {
        Weight::zero()
    }

    fn signed_redeem_shares_scalar() -> Weight {
        Weight::zero()
    }

    fn signed_buy_complete_set(_a: u32) -> Weight {
        Weight::zero()
    }
}

impl crate::Config for Runtime {
    type Public = <SignatureTest as Verify>::Signer;
    type RuntimeCall = RuntimeCall;
    type Signature = SignatureTest;
    type WeightInfo = SignedPredictionMarketsWeightInfo;
}

pub fn account(seed: u8) -> AccountId {
    let pair = sr25519::Pair::from_seed(&[seed; 32]);
    AccountId::decode(&mut pair.public().to_vec().as_slice()).unwrap()
}

pub fn key_pair(seed: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[seed; 32])
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    ASSET_BALANCES.with(|balances| balances.borrow_mut().clear());

    let mut storage = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
    pallet_balances::GenesisConfig::<Runtime> {
        balances: vec![(account(0), 1_000_000), (account(1), 1_000_000), (account(2), 1_000_000)],
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    storage.into()
}
