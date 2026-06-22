// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{self as pallet_node_manager, *};
use frame_support::{derive_impl, parameter_types, weights::Weight, PalletId};
use frame_system as system;
use pallet_session as session;
pub use parity_scale_codec::alloc::sync::Arc;
use parity_scale_codec::Decode;
pub use parking_lot::RwLock;
pub use sp_core::{
    offchain::{
        testing::{
            OffchainState, PendingRequest, PoolState, TestOffchainExt, TestTransactionPoolExt,
        },
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    sr25519, Pair,
};
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
pub use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{ConvertInto, IdentityLookup, Verify},
    BuildStorage, Perbill,
};
use sp_state_machine::BasicExternalities;
use std::cell::RefCell;

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;

/// Native token unit used for mock balances. Mirrors `runtime/common::constants::currency::AVT`.
pub const AVT: u128 = 1_000_000_000_000_000_000;

#[derive(Clone)]
pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(seed: [u8; 32]) -> Self {
        TestAccount { seed }
    }

    pub fn account_id(&self) -> AccountId {
        AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        sr25519::Pair::from_seed(&self.seed)
    }
}

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        NodeManager: pallet_node_manager::{Pallet, Call, Storage, Event<T>, Config<T>},
        AVN: pallet_avn::{Pallet, Storage, Event, Config<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
    }
);

parameter_types! {
    pub const RewardPotId: PalletId = PalletId(*b"avtnodes");
    pub TreasurySource: AccountId = treasury_account();
    /// Small interval so tests can observe halving on a tight budget.
    pub const HalvingInterval: u64 = 1_000;
    /// Defaults to OFF; tests flip via set_halving_enabled.
    pub const HalvingEnabledAtGenesis: bool = false;
    pub const MaxNodesPerAggregateHeartbeat: u32 = 1024;
    /// Production value (30k per the hard-fork proposal). Cap tests set
    /// `TotalRegisteredNodes` storage directly instead of mass-registering.
    pub const MaxRegisteredNodes: u32 = 30_000;
}

/// A pseudo-treasury account used as the funding source for the reward pot in
/// the mock. Funded in genesis with a large balance so every reward-period
/// rollover can succeed by default; tests that need an "underfunded" treasury
/// drain it via `Balances::make_free_balance_be`.
pub fn treasury_account() -> AccountId {
    TestAccount::new([23u8; 32]).account_id()
}

/// Default treasury balance available in mock genesis (covers thousands of
/// rollovers at the default reward_amount_per_period).
pub const TREASURY_GENESIS_BALANCE: u128 = 1_000_000 * AVT;

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type SignerId = UintAuthorityId;
    type Public = AccountId;
    type Signature = Signature;
    type RewardPotId = RewardPotId;
    type TreasurySource = TreasurySource;
    type HalvingInterval = HalvingInterval;
    type HalvingEnabledAtGenesis = HalvingEnabledAtGenesis;
    type MaxNodesPerAggregateHeartbeat = MaxNodesPerAggregateHeartbeat;
    type MaxRegisteredNodes = MaxRegisteredNodes;
    type TimeProvider = pallet_timestamp::Pallet<TestRuntime>;
    type SignedTxLifetime = ConstU32<64>;
    type WeightInfo = ();
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
}

pub struct TestSessionManager;
impl session::SessionManager<AccountId> for TestSessionManager {
    fn new_session(_new_index: u32) -> Option<Vec<AccountId>> {
        AUTHORS.with(|l| l.borrow_mut().take())
    }
    fn end_session(_: u32) {}
    fn start_session(_: u32) {}
}

impl session::Config for TestRuntime {
    type SessionManager = TestSessionManager;
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (AVN,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
}

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type OverarchingCall = RuntimeCall;
    type Extrinsic = Extrinsic;
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: Weight = Weight::from_parts(1024 as u64, 0);
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
    pub const ChallengePeriod: u64 = 2;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u128>;
}

impl pallet_avn::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = UintAuthorityId;
    type EthereumPublicKeyChecker = ();
    type NewSessionHandler = ();
    type DisabledValidatorChecker = ();
    type WeightInfo = ();
}

thread_local! {
    static EXISTENTIAL_DEPOSIT: RefCell<u128> = RefCell::new(0);
}

/// Existential deposit is thread-local so individual tests can exercise the
/// `ED > 0` reaping behaviour (e.g. reward-pot reclaim) while the suite default
/// stays zero.
pub struct ExistentialDeposit;
impl frame_support::traits::Get<u128> for ExistentialDeposit {
    fn get() -> u128 {
        EXISTENTIAL_DEPOSIT.with(|v| *v.borrow())
    }
}

pub(crate) fn set_existential_deposit(ed: u128) {
    EXISTENTIAL_DEPOSIT.with(|v| *v.borrow_mut() = ed);
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = frame_support::traits::ConstU64<12000>;
    type WeightInfo = ();
}

pub fn author_id_1() -> AccountId {
    TestAccount::new([17u8; 32]).account_id()
}
pub fn author_id_2() -> AccountId {
    TestAccount::new([19u8; 32]).account_id()
}

thread_local! {
    pub static AUTHORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        author_id_1(),
        author_id_2(),
    ]));
}

pub struct ExtBuilder {
    pub storage: sp_runtime::Storage,
    offchain_state: Option<Arc<RwLock<OffchainState>>>,
    pool_state: Option<Arc<RwLock<PoolState>>>,
    txpool_extension: Option<TestTransactionPoolExt>,
    offchain_extension: Option<TestOffchainExt>,
    offchain_registered: bool,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        // Reset the thread-local existential deposit so a prior test on this
        // thread that raised it cannot leak into the next one.
        set_existential_deposit(0);

        let mut storage: sp_runtime::Storage =
            frame_system::GenesisConfig::<TestRuntime>::default()
                .build_storage()
                .unwrap()
                .into();

        // Pre-fund the treasury source so reward-period rollover transfers
        // succeed by default. Tests can drain or override this balance to
        // exercise the funding-failure path.
        let _ = pallet_balances::GenesisConfig::<TestRuntime> {
            balances: vec![(treasury_account(), TREASURY_GENESIS_BALANCE)],
            ..Default::default()
        }
        .assimilate_storage(&mut storage);

        Self {
            storage,
            pool_state: None,
            offchain_state: None,
            txpool_extension: None,
            offchain_extension: None,
            offchain_registered: false,
        }
    }

    pub fn with_genesis_config(mut self) -> Self {
        let _ = pallet_node_manager::GenesisConfig::<TestRuntime> {
            reward_period: 200u32,
            max_batch_size: 10u32,
            heartbeat_period: 5u32,
            reward_amount_per_period: 20 * AVT,
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    pub fn with_authors(mut self) -> Self {
        let authors: Vec<AccountId> = AUTHORS.with(|l| l.borrow_mut().take().unwrap());

        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &authors {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        let _ = pallet_session::GenesisConfig::<TestRuntime> {
            keys: authors
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, v, UintAuthorityId((i as u32).into())))
                .collect(),
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    pub fn for_offchain_worker(mut self) -> Self {
        assert!(!self.offchain_registered);
        let (offchain, offchain_state) = TestOffchainExt::new();
        let (pool, pool_state) = TestTransactionPoolExt::new();
        self.txpool_extension = Some(pool);
        self.offchain_extension = Some(offchain);
        self.pool_state = Some(pool_state);
        self.offchain_state = Some(offchain_state);
        self.offchain_registered = true;
        self
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let keystore = MemoryKeystore::new();

        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(keystore)));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            Timestamp::set_timestamp(1);
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
            RewardEnabled::<TestRuntime>::put(true);
        });
        ext
    }

    pub fn as_externality_with_state(
        self,
    ) -> (sp_io::TestExternalities, Arc<RwLock<PoolState>>, Arc<RwLock<OffchainState>>) {
        assert!(self.offchain_registered);
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(OffchainDbExt::new(self.offchain_extension.clone().unwrap()));
        ext.register_extension(OffchainWorkerExt::new(self.offchain_extension.unwrap()));
        ext.register_extension(TransactionPoolExt::new(self.txpool_extension.unwrap()));
        assert!(self.pool_state.is_some());
        assert!(self.offchain_state.is_some());
        ext.execute_with(|| {
            Timestamp::set_timestamp(1);
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
            RewardEnabled::<TestRuntime>::put(true);
        });
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

/// Advance the mock clock by whole weeks (the lock-penalty granularity).
pub(crate) fn advance_time_weeks(weeks: u64) {
    let now_ms = Timestamp::get();
    Timestamp::set_timestamp(now_ms + weeks * crate::types::SECONDS_PER_WEEK * 1_000);
}

/// Set the global lock window anchored `weeks_ago` full weeks before the
/// current mock time. With the default 52% week-one penalty, `weeks_ago = 0`
/// is the week-one rate and `weeks_ago >= 52` is fully decayed (expired).
/// The mock clock starts near zero, so the clock is first advanced far enough
/// that the anchor doesn't saturate at zero.
pub(crate) fn set_lock_schedule(weeks_ago: u64, initial_penalty_percent: u32) {
    let needed_ms = weeks_ago * crate::types::SECONDS_PER_WEEK * 1_000;
    if Timestamp::get() < needed_ms {
        Timestamp::set_timestamp(needed_ms);
    }
    let now = NodeManager::time_now_sec();
    let start = now.saturating_sub(weeks_ago * crate::types::SECONDS_PER_WEEK);
    LockSchedule::<TestRuntime>::put(crate::types::LockScheduleInfo::new(
        start,
        initial_penalty_percent,
    ));
}

/// Expired lock window: the penalty has decayed to zero, so reward payouts
/// credit free balance directly (the pre-lock behaviour most existing tests
/// assert).
pub(crate) fn expire_lock_schedule() {
    set_lock_schedule(52, 52);
}

/// Rolls desired block number of times.
pub(crate) fn roll_forward(num_blocks_to_roll: u64) {
    let mut current_block = System::block_number();
    let target_block = current_block + num_blocks_to_roll;
    while current_block < target_block {
        current_block = roll_one_block();
    }
}

pub(crate) fn roll_one_block() -> u64 {
    Balances::on_finalize(System::block_number());
    System::on_finalize(System::block_number());
    System::set_block_number(System::block_number() + 1);
    System::on_initialize(System::block_number());
    Balances::on_initialize(System::block_number());
    NodeManager::on_initialize(System::block_number());
    System::block_number()
}

pub fn mock_get_finalised_block(state: &mut OffchainState, response: &Option<Vec<u8>>) {
    let url = "http://127.0.0.1:2020/latest_finalised_block".to_string();

    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: url.into(),
        response: response.clone(),
        sent: true,
        ..Default::default()
    });
}
