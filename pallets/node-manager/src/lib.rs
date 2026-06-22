// Copyright 2026 Aventus DAO Ltd

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use frame_support::{
    dispatch::DispatchResult,
    pallet_prelude::*,
    storage::{generator::StorageDoubleMap as StorageDoubleMapTrait, PrefixIterator},
    traits::{Currency, ExistenceRequirement, IsSubType, StorageVersion, UnixTime},
    PalletId,
};
use frame_system::{
    offchain::{SendTransactionTypes, SubmitTransaction},
    pallet_prelude::*,
};
use pallet_avn::{self as avn};
use parity_scale_codec::{Decode, Encode, FullCodec};
use sp_application_crypto::RuntimeAppPublic;
use sp_core::MaxEncodedLen;
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    scale_info::TypeInfo,
    traits::{AccountIdConversion, Dispatchable, IdentifyAccount, Verify, Zero},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError, Perbill, Perquintill, RuntimeDebug, Saturating,
};

pub mod offchain;
pub mod reward;
pub mod types;
use crate::types::*;
pub mod default_weights;
pub use default_weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/test_admin.rs"]
mod test_admin;
#[cfg(test)]
#[path = "tests/test_delegated_heartbeat.rs"]
mod test_delegated_heartbeat;
#[cfg(test)]
#[path = "tests/test_heartbeat.rs"]
mod test_heartbeat;
#[cfg(test)]
#[path = "tests/test_node_deregistration.rs"]
mod test_node_deregistration;
#[cfg(test)]
#[path = "tests/test_node_registration.rs"]
mod test_node_registration;
#[cfg(test)]
#[path = "tests/test_on_idle_drain.rs"]
mod test_on_idle_drain;
#[cfg(test)]
#[path = "tests/test_reward_halving.rs"]
mod test_reward_halving;
#[cfg(test)]
#[path = "tests/test_reward_lock.rs"]
mod test_reward_lock;
#[cfg(test)]
#[path = "tests/test_top_up_reward_pot.rs"]
mod test_top_up_reward_pot;

// Definition of the crypto to use for signing
pub mod sr25519 {
    pub mod app_sr25519 {
        use sp_application_crypto::{app_crypto, sr25519, KeyTypeId};
        app_crypto!(sr25519, KeyTypeId(*b"nodk"));
    }

    pub type AuthorityId = app_sr25519::Public;
}

#[cfg(not(feature = "std"))]
use sp_std::prelude::*;

const HEARTBEAT_CONTEXT: &'static [u8] = b"NodeManager_heartbeat";
const MAX_BATCH_SIZE: u32 = 1_000;
pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);
pub const SIGNED_REGISTER_NODE_CONTEXT: &[u8] = b"register_node";
pub const SIGNED_DEREGISTER_NODE_CONTEXT: &[u8] = b"deregister_node";
pub const AGGREGATE_HEARTBEAT_CONTEXT: &[u8] = b"aggregate_heartbeat";
pub const MAX_NODES_TO_DEREGISTER: u32 = 64;

/// Offchain-worker storage key under which the local node's registered AccountId is persisted.
pub const REGISTERED_NODE_KEY: &'static [u8; 26] = b"ocw_pallet_registered_node";

// Error codes returned by validate unsigned methods
/// Invalid signature for `heartbeat` transaction
pub const ERROR_CODE_INVALID_HEARTBEAT_SIGNATURE: u8 = 2;
/// Node not found
pub const ERROR_CODE_INVALID_NODE: u8 = 3;
/// Rewards are disabled
pub const ERROR_CODE_REWARD_DISABLED: u8 = 4;
/// Invalid heartbeat submission
pub const ERROR_CODE_INVALID_HEARTBEAT: u8 = 5;

pub type AVN<T> = avn::Pallet<T>;
pub use pallet::*;

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
/// Node account ID
pub(crate) type NodeId<T> = <T as frame_system::Config>::AccountId;
/// Max nodes per deregistration call
pub type MaxNodesToDeregister = ConstU32<MAX_NODES_TO_DEREGISTER>;

#[frame_support::pallet]
pub mod pallet {
    use sp_avn_common::{verify_signature, InnerCallValidator, Proof};

    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// Registered nodes
    #[pallet::storage]
    pub type NodeRegistry<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        NodeId<T>,
        NodeInfo<T::SignerId, T::AccountId>,
        OptionQuery,
    >;

    /// Signing key to node ID
    #[pallet::storage]
    pub type SigningKeyToNodeId<T: Config> =
        StorageMap<_, Blake2_128Concat, T::SignerId, NodeId<T>, OptionQuery>;

    /// Total registered nodes
    #[pallet::storage]
    pub type TotalRegisteredNodes<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Owner to node mapping
    #[pallet::storage]
    pub type OwnedNodes<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId, // OwnerAddress
        Blake2_128Concat,
        NodeId<T>,
        (),
        OptionQuery,
    >;

    /// Number of nodes owned by each account
    #[pallet::storage]
    pub type OwnedNodesCount<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// Account allowed to register nodes
    #[pallet::storage]
    pub type NodeRegistrar<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// Max nodes paid per batch
    #[pallet::storage]
    pub type MaxBatchSize<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Heartbeat period in blocks for the next reward period
    #[pallet::storage]
    pub type NextHeartbeatPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Length of the next reward period in blocks
    #[pallet::storage]
    pub type NextRewardPeriodLength<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Reward amount for the next reward period
    #[pallet::storage]
    pub type NextRewardAmountPerPeriod<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// Reward snapshots by period
    #[pallet::storage]
    pub(super) type RewardPot<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        RewardPotInfo<BalanceOf<T>>,
        OptionQuery,
    >;

    /// Total rewards still to be paid
    #[pallet::storage]
    pub type OutstandingRewardToPay<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// Current reward period
    #[pallet::storage]
    #[pallet::getter(fn current_reward_period)]
    pub(super) type RewardPeriod<T: Config> =
        StorageValue<_, RewardPeriodInfo<BlockNumberFor<T>, BalanceOf<T>>, ValueQuery>;

    /// Oldest unpaid reward period
    #[pallet::storage]
    #[pallet::getter(fn oldest_unpaid_period)]
    pub(super) type OldestUnpaidRewardPeriodIndex<T: Config> =
        StorageValue<_, RewardPeriodIndex, ValueQuery>;

    /// Last paid node pointer
    #[pallet::storage]
    #[pallet::getter(fn last_paid_pointer)]
    pub(super) type LastPaidPointer<T: Config> =
        StorageValue<_, PaymentPointer<T::AccountId>, OptionQuery>;

    /// Node uptime by reward period
    #[pallet::storage]
    #[pallet::getter(fn node_uptime)]
    pub(super) type NodeUptime<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        Blake2_128Concat,
        NodeId<T>,
        UptimeInfo<BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Total uptime by reward period
    #[pallet::storage]
    pub(super) type TotalUptime<T: Config> =
        StorageMap<_, Blake2_128Concat, RewardPeriodIndex, TotalUptimeInfo, ValueQuery>;

    /// Whether rewards are enabled
    #[pallet::storage]
    pub(super) type RewardEnabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Minimum uptime threshold
    #[pallet::storage]
    pub type MinUptimeThreshold<T: Config> = StorageValue<_, Perbill, OptionQuery>;

    /// Next node serial number
    #[pallet::storage]
    pub type NextNodeSerialNumber<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Cumulative count of reward-amount halvings applied since genesis.
    /// Updated by `apply_halving_if_due` at most once per `HalvingInterval`
    /// boundary. Idempotent within the same block.
    #[pallet::storage]
    pub type RewardAmountHalvingsApplied<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Whether automatic reward-amount halving is enabled. Defaults to
    /// `HalvingEnabledAtGenesis` at genesis; flipped at runtime via the
    /// root-only `set_halving_enabled` extrinsic.
    #[pallet::storage]
    pub type HalvingEnabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Rewards earned by an owner while the global lock window is active.
    /// The funds themselves stay in the reward-pot account; this map records
    /// each owner's claim, realised via `withdraw_rewards`.
    #[pallet::storage]
    pub type LockedRewards<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, BalanceOf<T>, ValueQuery>;

    /// Sum of all `LockedRewards` entries - the pot's outstanding locked
    /// liability. The reward-pot balance must always cover this plus any
    /// undrained period pots.
    #[pallet::storage]
    pub type TotalLockedRewards<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// The single global reward-lock window (one-off, anchored to the
    /// migration Global Start Date). While unset, payouts are locked
    /// defensively and withdrawal is blocked - root must configure the
    /// window before any locked reward can be released.
    #[pallet::storage]
    pub type LockSchedule<T: Config> = StorageValue<_, LockScheduleInfo, OptionQuery>;

    /// Destination for forfeited (early-withdrawn) reward amounts - the
    /// foundation forfeiture-liquidity wallet on mainnet. Falls back to
    /// `T::TreasurySource` while unset, which keeps forfeits in-system.
    #[pallet::storage]
    pub type ForfeitureDestination<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub max_batch_size: u32,
        pub reward_period: u32,
        pub heartbeat_period: u32,
        pub reward_amount_per_period: BalanceOf<T>,
        /// Unix-seconds anchor of the global reward-lock window. `None`
        /// leaves the schedule unset (payouts lock, withdrawals blocked,
        /// until root configures it).
        pub lock_schedule_start: Option<Duration>,
        /// Week-one forfeiture percentage of the lock window.
        pub lock_initial_penalty_percent: u32,
        /// Destination for forfeited amounts; `None` falls back to
        /// `T::TreasurySource` at withdrawal time.
        pub forfeiture_destination: Option<T::AccountId>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                max_batch_size: 1,
                reward_period: 2,
                heartbeat_period: 1,
                reward_amount_per_period: Default::default(),
                lock_schedule_start: None,
                lock_initial_penalty_percent: 52,
                forfeiture_destination: None,
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            // The reward pot is a pallet-derived account that only ever
            // receives funds at runtime. On a chain with a zero existential
            // deposit, a credit to a provider-less account does not persist
            // (the transfer succeeds but the funds are lost), so the pot
            // gets its provider reference at genesis - the same pattern
            // pallet-treasury uses for its pot account.
            frame_system::Pallet::<T>::inc_providers(&Pallet::<T>::compute_reward_account_id());

            assert!(self.reward_period > self.heartbeat_period);
            let default_threshold = Pallet::<T>::get_default_threshold();

            NextRewardPeriodLength::<T>::set(self.reward_period);
            NextRewardAmountPerPeriod::<T>::set(self.reward_amount_per_period);
            MaxBatchSize::<T>::set(self.max_batch_size);
            NextHeartbeatPeriod::<T>::set(self.heartbeat_period);
            MinUptimeThreshold::<T>::set(Some(default_threshold));

            let uptime_threshold =
                Pallet::<T>::calculate_uptime_threshold(self.reward_period, self.heartbeat_period);
            let reward_period: RewardPeriodInfo<BlockNumberFor<T>, BalanceOf<T>> =
                RewardPeriodInfo::new(
                    0u64,
                    0u32.into(),
                    self.reward_period,
                    self.heartbeat_period,
                    uptime_threshold,
                    self.reward_amount_per_period,
                );

            <RewardPeriod<T>>::put(reward_period);
            OutstandingRewardToPay::<T>::put(BalanceOf::<T>::zero());
            HalvingEnabled::<T>::put(T::HalvingEnabledAtGenesis::get());

            assert!(self.lock_initial_penalty_percent <= 100, "lock penalty must be a percentage");
            if let Some(start) = self.lock_schedule_start {
                LockSchedule::<T>::put(LockScheduleInfo::new(
                    start,
                    self.lock_initial_penalty_percent,
                ));
            }
            if let Some(ref destination) = self.forfeiture_destination {
                ForfeitureDestination::<T>::put(destination.clone());
            }
        }
    }

    // Pallet Events
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Node registered
        NodeRegistered { owner: T::AccountId, node: NodeId<T> },
        /// Reward period length set
        RewardPeriodLengthSet {
            period_index: u64,
            old_reward_period_length: u32,
            new_reward_period_length: u32,
        },
        /// New reward period started
        NewRewardPeriodStarted {
            reward_period_index: RewardPeriodIndex,
            reward_period_length: u32,
            uptime_threshold: u32,
            previous_period_reward: BalanceOf<T>,
        },
        /// Reward payout completed
        RewardPayoutCompleted { reward_period_index: RewardPeriodIndex },
        /// Reward paid
        RewardPaid {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// Reward payment failed
        ErrorPayingReward {
            reward_period: RewardPeriodIndex,
            node: NodeId<T>,
            error: DispatchError,
        },
        /// Node registrar set
        NodeRegistrarSet { new_registrar: T::AccountId },
        /// Batch size set
        BatchSizeSet { new_size: u32 },
        /// Heartbeat period set
        NextHeartbeatPeriodSet { new_heartbeat_period: u32 },
        /// Heartbeat received
        HeartbeatReceived { reward_period_index: RewardPeriodIndex, node: NodeId<T> },
        /// Reward amount per period set
        NextRewardAmountPerPeriodSet { new_amount: BalanceOf<T> },
        /// Reward payment toggled
        RewardEnabledSet { enabled: bool },
        /// Min uptime threshold set
        MinUptimeThresholdSet { threshold: Perbill },
        /// Node deregistered
        NodeDeregistered { owner: T::AccountId, node: NodeId<T> },
        /// Signing key updated
        SigningKeyUpdated { owner: T::AccountId, node: NodeId<T> },
        /// Reward pot funded for a period (treasury transfer succeeded)
        RewardPotFunded { period: RewardPeriodIndex, amount: BalanceOf<T> },
        /// Reward pot funding failed at rollover; period is recoverable via top_up_reward_pot
        RewardPotFundingFailed {
            period: RewardPeriodIndex,
            requested_amount: BalanceOf<T>,
            reason: DispatchError,
        },
        /// Halving applied to `NextRewardAmountPerPeriod`. `total_halvings` is
        /// the cumulative count since genesis; `new_amount` is the post-halving
        /// value (post all pending halvings if more than one boundary was
        /// crossed between calls).
        RewardHalvingApplied {
            period_index: RewardPeriodIndex,
            new_amount: BalanceOf<T>,
            total_halvings: u32,
        },
        /// `HalvingEnabled` toggled by root
        HalvingEnabledSet { enabled: bool },
        /// A reward accrued into `LockedRewards` instead of free balance
        /// (the global lock window is active or not yet configured).
        RewardLocked {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// Locked rewards withdrawn. `net` went to the owner, `forfeited`
        /// (`penalty` of `gross`) to the forfeiture destination.
        RewardWithdrawn {
            owner: T::AccountId,
            gross: BalanceOf<T>,
            net: BalanceOf<T>,
            forfeited: BalanceOf<T>,
            penalty: Perbill,
        },
        /// A non-distributable period's funded reward was returned from the
        /// pot to the treasury (the period had no reportable uptime).
        UndistributedRewardReclaimed { reward_period: RewardPeriodIndex, amount: BalanceOf<T> },
        /// Reclaim of a non-distributable period's reward failed; the funds
        /// remain in the pot to be recovered by a later admin action.
        UndistributedRewardReclaimFailed { reward_period: RewardPeriodIndex, amount: BalanceOf<T> },
        /// The global lock window set by root
        LockScheduleSet { start: Duration, initial_penalty_percent: u32 },
        /// Forfeiture destination set by root
        ForfeitureDestinationSet { destination: T::AccountId },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Invalid node registrar
        OriginNotRegistrar,
        /// Invalid last paid node
        InvalidNodePointer,
        /// Invalid last paid period
        InvalidPeriodPointer,
        /// Node registrar not set
        RegistrarNotSet,
        /// Node already registered
        DuplicateNode,
        /// Invalid signing key
        InvalidSigningKey,
        /// Signing key already in use
        SigningKeyAlreadyInUse,
        /// Invalid reward period
        RewardPeriodInvalid,
        /// Invalid batch size
        BatchSizeInvalid,
        /// Invalid heartbeat period
        NextHeartbeatPeriodInvalid,
        /// Heartbeat period is zero
        NextHeartbeatPeriodZero,
        /// Reward pot has insufficient balance
        InsufficientBalanceForReward,
        /// Total uptime not found
        TotalUptimeNotFound,
        /// Node uptime not found
        NodeUptimeNotFound,
        /// Invalid reward payment request
        InvalidRewardPaymentRequest,
        /// Duplicate heartbeat
        DuplicateHeartbeat,
        /// Invalid heartbeat
        InvalidHeartbeat,
        /// Node not registered
        NodeNotRegistered,
        /// Failed to acquire OCW DB lock
        FailedToAcquireOcwDbLock,
        /// Reward amount is zero
        RewardAmountZero,
        /// Sender is not the signer
        SenderIsNotSigner,
        /// Unauthorized signed transaction
        UnauthorizedSignedTransaction,
        /// Signed transaction expired
        SignedTransactionExpired,
        /// Heartbeat threshold reached
        HeartbeatThresholdReached,
        /// Uptime threshold is zero
        UptimeThresholdZero,
        /// Unauthorized signing key update
        UnauthorizedSigningKeyUpdate,
        /// Signing key must be different
        SigningKeyMustBeDifferent,
        /// Amount must be greater than zero
        ZeroAmount,
        /// Node not found
        NodeNotFound,
        /// Balance overflow
        BalanceOverflow,
        /// Balance underflow
        BalanceUnderflow,
        /// Reward pot snapshot not found
        RewardPotNotFound,
        /// Reward amount per period must be greater than zero
        NextRewardAmountPerPeriodZero,
        /// Reward pot already funded for the given period
        RewardPotAlreadyFunded,
        /// Treasury could not supply the requested amount
        TreasuryUnderfunded,
        /// `proof.signer` does not resolve to a registered node
        ProverNotRegistered,
        /// A node in the batch is not registered or not owned by the prover
        NodeNotOwnedByProver,
        /// The caller has no locked rewards to withdraw
        NoLockedRewards,
        /// The global lock window has not been configured yet
        LockScheduleNotSet,
        /// Requested withdrawal exceeds the caller's locked balance
        WithdrawAmountExceedsLocked,
        /// Lock schedule parameters are invalid (penalty must be <= 100%)
        InvalidLockSchedule,
        /// The network-wide registered-node cap has been reached
        MaxNodesReached,
    }

    #[pallet::config]
    pub trait Config:
        frame_system::Config + avn::Config + SendTransactionTypes<Call<Self>>
    {
        /// Runtime event type
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// Runtime call type
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;
        /// Currency used by this pallet
        type Currency: Currency<Self::AccountId>;
        // The identifier type for an offchain transaction signer.
        type SignerId: Member
            + Parameter
            + RuntimeAppPublic
            + Ord
            + MaybeSerializeDeserialize
            + MaxEncodedLen;
        /// Account type used for signature verification
        type Public: IdentifyAccount<AccountId = Self::AccountId>;
        /// Time provider
        type TimeProvider: UnixTime;
        /// Signature type
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;
        /// Reward pot ID
        #[pallet::constant]
        type RewardPotId: Get<PalletId>;
        /// Source account from which the reward pot is funded at each period rollover
        type TreasurySource: Get<Self::AccountId>;
        /// Number of blocks between halving applications. Setting this to
        /// `BLOCKS_PER_YEAR` (predictor's annual cadence) is the production
        /// default. Setting it small in tests makes halving observable on a
        /// budget.
        #[pallet::constant]
        type HalvingInterval: Get<BlockNumberFor<Self>>;
        /// Whether `HalvingEnabled` defaults to `true` at genesis. Runtime
        /// flips it via `set_halving_enabled`.
        #[pallet::constant]
        type HalvingEnabledAtGenesis: Get<bool>;
        /// Maximum number of nodes covered by a single
        /// `heartbeat_for_owned_nodes` call. Bounds extrinsic weight and
        /// validation work.
        #[pallet::constant]
        type MaxNodesPerAggregateHeartbeat: Get<u32>;
        /// Hard cap on concurrently registered nodes. The hard-fork proposal
        /// fixes the Predictor network at 30,000 nodes; registration fails
        /// once `TotalRegisteredNodes` reaches this value, and deregistering
        /// frees capacity.
        #[pallet::constant]
        type MaxRegisteredNodes: Get<u32>;
        /// Signed transaction lifetime in blocks
        #[pallet::constant]
        type SignedTxLifetime: Get<u32>;
        /// Extrinsic weight provider
        type WeightInfo: WeightInfo;
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a new node
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::register_node())]
        pub fn register_node(
            origin: OriginFor<T>,
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(who == registrar, Error::<T>::OriginNotRegistrar);

            Self::do_register_node(node, owner, signing_key)?;
            Ok(())
        }

        /// Set admin configurations
        #[pallet::call_index(1)]
        #[pallet::weight(
            <T as Config>::WeightInfo::register_node()
            .max(<T as Config>::WeightInfo::set_admin_config_registrar())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_period())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_batch_size())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_heartbeat())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_amount())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_enabled())
            .max(<T as Config>::WeightInfo::set_admin_config_min_threshold())
            .max(<T as Config>::WeightInfo::set_admin_config_lock_schedule())
            .max(<T as Config>::WeightInfo::set_admin_config_forfeiture_destination())
        )]
        pub fn set_admin_config(
            origin: OriginFor<T>,
            config: AdminConfig<T::AccountId, BalanceOf<T>>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            match config {
                AdminConfig::NodeRegistrar(registrar) => {
                    <NodeRegistrar<T>>::mutate(|maybe_registrar| {
                        *maybe_registrar = Some(registrar.clone())
                    });
                    Self::deposit_event(Event::NodeRegistrarSet { new_registrar: registrar });
                    return Ok(Some(<T as Config>::WeightInfo::set_admin_config_registrar()).into())
                },
                AdminConfig::NextRewardPeriodLength(period) => {
                    let heartbeat = <NextHeartbeatPeriod<T>>::get();
                    ensure!(period > heartbeat, Error::<T>::RewardPeriodInvalid);

                    let period_index = RewardPeriod::<T>::get().current;
                    let old_period = NextRewardPeriodLength::<T>::get();

                    NextRewardPeriodLength::<T>::put(period);

                    Self::deposit_event(Event::RewardPeriodLengthSet {
                        period_index,
                        old_reward_period_length: old_period,
                        new_reward_period_length: period,
                    });

                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_period()).into())
                },
                AdminConfig::BatchSize(size) => {
                    ensure!(size > 0 && size <= MAX_BATCH_SIZE, Error::<T>::BatchSizeInvalid);
                    <MaxBatchSize<T>>::mutate(|s| *s = size);
                    Self::deposit_event(Event::BatchSizeSet { new_size: size });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_batch_size()).into())
                },
                AdminConfig::NextHeartbeatPeriod(period) => {
                    let next_reward_period_length = NextRewardPeriodLength::<T>::get();
                    ensure!(period > 0, Error::<T>::NextHeartbeatPeriodZero);
                    ensure!(
                        period < next_reward_period_length,
                        Error::<T>::NextHeartbeatPeriodInvalid
                    );

                    <NextHeartbeatPeriod<T>>::put(period);

                    Self::deposit_event(Event::NextHeartbeatPeriodSet {
                        new_heartbeat_period: period,
                    });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_heartbeat()).into())
                },
                AdminConfig::NextRewardAmountPerPeriod(amount) => {
                    ensure!(
                        amount > BalanceOf::<T>::zero(),
                        Error::<T>::NextRewardAmountPerPeriodZero
                    );
                    <NextRewardAmountPerPeriod<T>>::put(amount);
                    Self::deposit_event(Event::NextRewardAmountPerPeriodSet { new_amount: amount });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_amount()).into())
                },
                AdminConfig::RewardEnabled(enabled) => {
                    <RewardEnabled<T>>::put(enabled);
                    Self::deposit_event(Event::RewardEnabledSet { enabled });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_enabled()).into())
                },
                AdminConfig::MinUptimeThreshold(threshold) => {
                    ensure!(threshold > Perbill::zero(), Error::<T>::UptimeThresholdZero);
                    <MinUptimeThreshold<T>>::put(threshold);

                    Self::deposit_event(Event::MinUptimeThresholdSet { threshold });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_min_threshold()).into())
                },
                AdminConfig::LockSchedule(schedule) => {
                    ensure!(
                        schedule.initial_penalty_percent <= 100,
                        Error::<T>::InvalidLockSchedule
                    );
                    <LockSchedule<T>>::put(schedule);
                    Self::deposit_event(Event::LockScheduleSet {
                        start: schedule.start,
                        initial_penalty_percent: schedule.initial_penalty_percent,
                    });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_lock_schedule()).into())
                },
                AdminConfig::ForfeitureDestination(destination) => {
                    <ForfeitureDestination<T>>::put(destination.clone());
                    Self::deposit_event(Event::ForfeitureDestinationSet { destination });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_forfeiture_destination())
                        .into())
                },
            }
        }

        /// Offchain call: Submit heartbeat to show node is still alive
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::offchain_submit_heartbeat())]
        pub fn offchain_submit_heartbeat(
            origin: OriginFor<T>,
            node: NodeId<T>,
            reward_period_index: RewardPeriodIndex,
            // This helps prevent signature re-use
            heartbeat_count: u64,
            _signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;

            Self::validate_heartbeats(node.clone(), reward_period_index, heartbeat_count)?;

            let current_reward_period = RewardPeriod::<T>::get().current;
            // if we pass validation we have a registered node but double check
            let node_info = NodeRegistry::<T>::get(&node).ok_or(Error::<T>::NodeNotRegistered)?;
            let now = frame_system::Pallet::<T>::block_number();

            let weight = <NodeUptime<T>>::mutate(&current_reward_period, &node, |maybe_info| {
                let info = maybe_info.get_or_insert_with(|| UptimeInfo {
                    count: 0,
                    last_reported: now,
                    weight: 0,
                });

                // Stake removed: every heartbeat counts as one base unit.
                let _ = node_info;
                let node_weight = HEARTBEAT_BASE_WEIGHT;

                info.count = info.count.saturating_add(1);
                info.last_reported = now;
                info.weight = info.weight.saturating_add(node_weight);

                // the total uptime for the period
                node_weight
            });

            <TotalUptime<T>>::mutate(&current_reward_period, |total| {
                total.total_heartbeats = total.total_heartbeats.saturating_add(1);
                total.total_weight = total.total_weight.saturating_add(weight);
            });

            Self::deposit_event(Event::HeartbeatReceived {
                reward_period_index: current_reward_period,
                node,
            });

            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::signed_register_node())]
        pub fn signed_register_node(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = encode_signed_register_node_params::<T>(
                &proof.relayer,
                &node,
                &owner,
                &signing_key,
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_register_node(node, owner, signing_key)?;

            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::deregister_nodes(nodes_to_deregister.len() as u32))]
        pub fn deregister_nodes(
            origin: OriginFor<T>,
            owner: T::AccountId,
            nodes_to_deregister: BoundedVec<NodeId<T>, MaxNodesToDeregister>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);

            Self::do_deregister_nodes(&owner, &nodes_to_deregister)?;

            Ok(())
        }

        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::signed_deregister_nodes(nodes_to_deregister.len() as u32))]
        pub fn signed_deregister_nodes(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            owner: T::AccountId,
            nodes_to_deregister: BoundedVec<NodeId<T>, MaxNodesToDeregister>,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = encode_signed_deregister_node_params::<T>(
                &proof.relayer,
                &owner,
                &nodes_to_deregister,
                &(nodes_to_deregister.len() as u32),
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_deregister_nodes(&owner, &nodes_to_deregister)?;

            Ok(())
        }

        /// Update signing key for a registered node
        #[pallet::call_index(7)]
        #[pallet::weight(<T as Config>::WeightInfo::update_signing_key())]
        pub fn update_signing_key(
            origin: OriginFor<T>,
            node: NodeId<T>,
            new_signing_key: T::SignerId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            let current_info =
                NodeRegistry::<T>::get(&node).ok_or(Error::<T>::NodeNotRegistered)?;
            let owner = current_info.owner;

            ensure!(who == registrar || who == owner, Error::<T>::UnauthorizedSigningKeyUpdate);
            // We could remove this and use the check below to catch all cases but this is more user
            // friendly
            ensure!(
                current_info.signing_key != new_signing_key,
                Error::<T>::SigningKeyMustBeDifferent
            );
            ensure!(
                !SigningKeyToNodeId::<T>::contains_key(&new_signing_key),
                Error::<T>::SigningKeyAlreadyInUse
            );

            <NodeRegistry<T>>::mutate(&node, |maybe_info| {
                if let Some(info) = maybe_info.as_mut() {
                    info.signing_key = new_signing_key.clone();
                }
            });

            Self::rotate_signing_key_index(&node, &current_info.signing_key, &new_signing_key)?;
            Self::deposit_event(Event::SigningKeyUpdated { owner, node });

            Ok(())
        }

        /// Root: top up the reward pot for a period whose rollover funding
        /// failed. The period's `RewardPotInfo` must exist with
        /// `total_reward == 0` (i.e. produced by a failed rollover transfer).
        /// Pulls `amount` from `TreasurySource` into the reward pot account
        /// and bumps `OutstandingRewardToPay`.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::top_up_reward_pot())]
        pub fn top_up_reward_pot(
            origin: OriginFor<T>,
            period: RewardPeriodIndex,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);

            let mut pot_info = RewardPot::<T>::get(period).ok_or(Error::<T>::RewardPotNotFound)?;
            ensure!(pot_info.total_reward.is_zero(), Error::<T>::RewardPotAlreadyFunded);

            let treasury = T::TreasurySource::get();
            let pot = Self::compute_reward_account_id();
            T::Currency::transfer(&treasury, &pot, amount, ExistenceRequirement::KeepAlive)
                .map_err(|_| Error::<T>::TreasuryUnderfunded)?;

            pot_info.total_reward = amount;
            pot_info.funding_failed = false;
            RewardPot::<T>::insert(period, pot_info);
            OutstandingRewardToPay::<T>::mutate(|outstanding| {
                *outstanding = outstanding.saturating_add(amount);
            });

            Self::deposit_event(Event::RewardPotFunded { period, amount });
            Ok(())
        }

        /// Root: toggle automatic reward-amount halving. When enabled the
        /// pallet halves `NextRewardAmountPerPeriod` at every `HalvingInterval`
        /// block boundary (idempotent per block, catch-up across multiple
        /// boundaries if disabled and re-enabled later).
        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::set_halving_enabled())]
        pub fn set_halving_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            HalvingEnabled::<T>::put(enabled);
            Self::deposit_event(Event::HalvingEnabledSet { enabled });
            Ok(())
        }

        /// Aggregate heartbeat: a single prover node signs a batch of node
        /// ids it owns. Mirrors `signed_register_node`'s avn-proxy shape:
        /// `proof.signer` is the prover's NodeId (==AccountId), sender must
        /// equal proof.signer. The pallet validates every node in `nodes`
        /// is registered to the prover's owner; on any failure the whole
        /// call rolls back (all-or-nothing). ("Prover" rather than "anchor"
        /// is used here to avoid conflating with the chain's separate
        /// anchoring mechanism.)
        ///
        /// Successful dispatch records one heartbeat per node into
        /// `NodeUptime` for the current reward period and emits one
        /// `HeartbeatReceived` per node. Duplicate entries in `nodes` are
        /// silently deduped via a BTreeSet so callers don't accidentally
        /// double-count.
        #[pallet::call_index(12)]
        #[pallet::weight(<T as Config>::WeightInfo::heartbeat_for_owned_nodes(nodes.len() as u32))]
        pub fn heartbeat_for_owned_nodes(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nodes: BoundedVec<NodeId<T>, T::MaxNodesPerAggregateHeartbeat>,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            let signed_payload = encode_aggregate_heartbeat_params::<T>(
                &proof.relayer,
                &nodes,
                &(nodes.len() as u32),
                &block_number,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            // The prover must be a registered node; its owner is the
            // asserted owner of every node in the batch.
            let prover_info =
                NodeRegistry::<T>::get(&proof.signer).ok_or(Error::<T>::ProverNotRegistered)?;
            let asserted_owner = prover_info.owner.clone();

            let reward_period = RewardPeriod::<T>::get();
            let current_period = reward_period.current;
            let now = frame_system::Pallet::<T>::block_number();

            // Pre-flight validation: every node must be registered to the
            // asserted owner - a false ownership claim is fatal and rejects the
            // whole batch. Rate-limiting mirrors the OCW path
            // (`validate_heartbeats`) but is per-node isolated: a node already at
            // the uptime threshold or still inside the spacing window is skipped,
            // not fatal, so a single maxed/recent node cannot block heartbeat
            // progress for the rest of the batch. Dedup is via BTreeSet so the
            // loop below stays O(N log N) and silently drops duplicate entries
            // within a single call. A second heartbeat for the same node in the
            // same block across distinct outer nonces is barred instead by the
            // spacing check below (`now < last_reported + heartbeat_period`),
            // not by this in-call set.
            use sp_std::collections::btree_set::BTreeSet;
            let mut unique: BTreeSet<NodeId<T>> = BTreeSet::new();
            for node in nodes.iter() {
                let info = NodeRegistry::<T>::get(node).ok_or(Error::<T>::NodeNotOwnedByProver)?;
                ensure!(info.owner == asserted_owner, Error::<T>::NodeNotOwnedByProver);

                if let Some(uptime_info) = NodeUptime::<T>::get(current_period, node) {
                    if uptime_info.count >= reward_period.uptime_threshold as u64 {
                        continue
                    }
                    let expected_submission = uptime_info.last_reported +
                        BlockNumberFor::<T>::from(reward_period.heartbeat_period);
                    if now < expected_submission {
                        continue
                    }
                }

                unique.insert(node.clone());
            }

            // All validations passed; record heartbeats. From this point on
            // we mutate state and cannot fail (modulo storage I/O).
            let mut total_new_weight: u128 = 0;
            let mut new_heartbeat_count: u64 = 0;

            for node in unique {
                NodeUptime::<T>::mutate(&current_period, &node, |maybe_info| {
                    let info = maybe_info.get_or_insert_with(|| UptimeInfo {
                        count: 0,
                        last_reported: now,
                        weight: 0,
                    });
                    info.count = info.count.saturating_add(1);
                    info.last_reported = now;
                    info.weight = info.weight.saturating_add(HEARTBEAT_BASE_WEIGHT);
                });
                total_new_weight = total_new_weight.saturating_add(HEARTBEAT_BASE_WEIGHT);
                new_heartbeat_count = new_heartbeat_count.saturating_add(1);
                Self::deposit_event(Event::HeartbeatReceived {
                    reward_period_index: current_period,
                    node,
                });
            }

            TotalUptime::<T>::mutate(&current_period, |total| {
                total.total_heartbeats = total.total_heartbeats.saturating_add(new_heartbeat_count);
                total.total_weight = total.total_weight.saturating_add(total_new_weight);
            });

            Ok(())
        }

        /// Withdraw locked rewards. `amount = None` withdraws the caller's
        /// full locked balance; `Some(limit)` withdraws exactly `limit`.
        /// The forfeiture rate of the global lock window at the time of the
        /// call applies to the withdrawn slice: `forfeited = penalty x gross`
        /// goes to the forfeiture destination, the remainder to the caller's
        /// free balance. From week 53 of the window onward the penalty is
        /// zero and a withdrawal returns the full amount.
        ///
        /// Fails while the global lock window is unconfigured: rewards
        /// accrued before the window is set stay locked until root sets it.
        #[pallet::call_index(13)]
        #[pallet::weight(<T as Config>::WeightInfo::withdraw_rewards())]
        pub fn withdraw_rewards(
            origin: OriginFor<T>,
            amount: Option<BalanceOf<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let locked = LockedRewards::<T>::get(&who);
            ensure!(!locked.is_zero(), Error::<T>::NoLockedRewards);
            let schedule = LockSchedule::<T>::get().ok_or(Error::<T>::LockScheduleNotSet)?;

            let gross = amount.unwrap_or(locked);
            ensure!(!gross.is_zero(), Error::<T>::ZeroAmount);
            ensure!(gross <= locked, Error::<T>::WithdrawAmountExceedsLocked);

            let penalty = schedule.penalty_at(Self::time_now_sec());
            let forfeited = penalty.mul_floor(gross);
            let net = gross.saturating_sub(forfeited);

            // Both transfers come out of the reward-pot account, where locked
            // rewards physically live. Extrinsic transactionality reverts the
            // first transfer if the second fails.
            let reward_pot = Self::compute_reward_account_id();
            if !net.is_zero() {
                T::Currency::transfer(&reward_pot, &who, net, ExistenceRequirement::AllowDeath)?;
            }
            if !forfeited.is_zero() {
                let destination =
                    ForfeitureDestination::<T>::get().unwrap_or_else(T::TreasurySource::get);
                T::Currency::transfer(
                    &reward_pot,
                    &destination,
                    forfeited,
                    ExistenceRequirement::AllowDeath,
                )?;
            }

            let remaining = locked.saturating_sub(gross);
            if remaining.is_zero() {
                LockedRewards::<T>::remove(&who);
            } else {
                LockedRewards::<T>::insert(&who, remaining);
            }
            TotalLockedRewards::<T>::mutate(|total| *total = total.saturating_sub(gross));

            Self::deposit_event(Event::RewardWithdrawn {
                owner: who,
                gross,
                net,
                forfeited,
                penalty,
            });

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        // Keep this logic light and bounded
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            // Halving runs unconditionally before the rollover guard so that a
            // halving + rollover that fall on the same block use the post-
            // halved amount.
            Self::apply_halving_if_due(n);

            if !RewardEnabled::<T>::get() {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
            }

            let reward_period = RewardPeriod::<T>::get();
            if !reward_period.should_update(n) {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
            }

            let previous_index = reward_period.current;
            let previous_uptime_threshold = reward_period.uptime_threshold;
            let reward_amount = reward_period.reward_amount;

            // We want to avoid unnecessary reads, so we perform this check and exit early
            let next_reward_period_length = NextRewardPeriodLength::<T>::get();
            let next_heartbeat_period = NextHeartbeatPeriod::<T>::get();
            if next_reward_period_length == 0 || next_heartbeat_period == 0 {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
                    .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(2))
            }

            let next_reward_amount = NextRewardAmountPerPeriod::<T>::get();
            let next_uptime_threshold =
                Self::calculate_uptime_threshold(next_reward_period_length, next_heartbeat_period);

            let next_reward_period = reward_period.update(
                n,
                next_reward_period_length,
                next_heartbeat_period,
                next_uptime_threshold,
                next_reward_amount,
            );
            RewardPeriod::<T>::put(&next_reward_period);

            // Fund the previous period's reward pot synchronously from the
            // treasury source. The pot account is recorded either way so the
            // period is recoverable via `top_up_reward_pot` if the transfer
            // fails (treasury underfunded, ED violation, etc.). If the period
            // turns out to have no distributable uptime, the funds are
            // reclaimed back to the treasury when the drain skips it (see
            // `drain_outstanding_payouts`), so they are never stranded.
            let treasury = T::TreasurySource::get();
            let pot = Self::compute_reward_account_id();
            let (funded_amount, funding_failed) = match T::Currency::transfer(
                &treasury,
                &pot,
                reward_amount,
                ExistenceRequirement::KeepAlive,
            ) {
                Ok(()) => {
                    OutstandingRewardToPay::<T>::mutate(|outstanding| {
                        *outstanding = outstanding.saturating_add(reward_amount);
                    });
                    Self::deposit_event(Event::RewardPotFunded {
                        period: previous_index,
                        amount: reward_amount,
                    });
                    (reward_amount, false)
                },
                Err(reason) => {
                    Self::deposit_event(Event::RewardPotFundingFailed {
                        period: previous_index,
                        requested_amount: reward_amount,
                        reason,
                    });
                    // A genuine funding failure only happens when a non-zero
                    // reward could not be transferred. A zero `reward_amount`
                    // transfers trivially via the `Ok` arm, so reaching here
                    // with a non-zero requested amount marks the period as
                    // awaiting recovery.
                    (BalanceOf::<T>::zero(), !reward_amount.is_zero())
                },
            };

            // Always record the period snapshot. On funding failure the entry
            // carries `total_reward = 0` and `funding_failed = true`, so the
            // drain leaves it recoverable and `top_up_reward_pot` can later
            // replace it with the actual funded amount.
            <RewardPot<T>>::insert(
                previous_index,
                RewardPotInfo::<BalanceOf<T>>::new(
                    funded_amount,
                    previous_uptime_threshold,
                    Self::time_now_sec(),
                    funding_failed,
                ),
            );

            Self::deposit_event(Event::NewRewardPeriodStarted {
                reward_period_index: next_reward_period.current,
                reward_period_length: next_reward_period.length,
                uptime_threshold: next_reward_period.uptime_threshold,
                previous_period_reward: reward_amount,
            });

            <T as Config>::WeightInfo::on_initialise_with_new_reward_period()
        }

        /// `on_idle` drain: when block production has remaining weight, walk
        /// the oldest unpaid reward period and pay nodes one at a time until
        /// (a) the weight budget is exhausted, (b) the per-block batch cap is
        /// hit, or (c) the period is fully paid (in which case
        /// `complete_reward_payout` advances `OldestUnpaidRewardPeriodIndex`
        /// and we may roll into the next period if weight remains).
        fn on_idle(_n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            if !RewardEnabled::<T>::get() {
                return Weight::zero()
            }
            Self::drain_outstanding_payouts(remaining_weight)
        }

        fn offchain_worker(n: BlockNumberFor<T>) {
            log::info!("🛠️  OCW for node manager");

            if <RewardEnabled<T>>::get() == false {
                log::warn!("🛠️  OCW - rewards are disabled, skipping");
                return
            }

            Self::send_heartbeat_if_required(n);
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if <RewardEnabled<T>>::get() == false {
                return InvalidTransaction::Custom(ERROR_CODE_REWARD_DISABLED).into()
            }

            let reduce_priority: TransactionPriority = TransactionPriority::from(1000u64);
            match call {
                Call::offchain_submit_heartbeat {
                    node,
                    reward_period_index,
                    heartbeat_count,
                    signature,
                } => {
                    let node_info = NodeRegistry::<T>::get(&node);
                    match node_info {
                        Some(info) => {
                            if Self::validate_heartbeats(
                                node.clone(),
                                *reward_period_index,
                                *heartbeat_count,
                            )
                            .is_err()
                            {
                                return InvalidTransaction::Custom(ERROR_CODE_INVALID_HEARTBEAT)
                                    .into()
                            }

                            if !Self::offchain_signature_is_valid(
                                &(HEARTBEAT_CONTEXT, heartbeat_count, reward_period_index),
                                &info.signing_key,
                                signature,
                            ) {
                                return InvalidTransaction::Custom(
                                    ERROR_CODE_INVALID_HEARTBEAT_SIGNATURE,
                                )
                                .into()
                            }

                            return ValidTransaction::with_tag_prefix("NodeManagerHeartbeat")
                                .and_provides((
                                    HEARTBEAT_CONTEXT,
                                    node,
                                    reward_period_index,
                                    heartbeat_count,
                                ))
                                .priority(TransactionPriority::max_value() - reduce_priority)
                                .longevity(64_u64)
                                .build()
                        },
                        _ => InvalidTransaction::Custom(ERROR_CODE_INVALID_NODE).into(),
                    }
                },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    impl<T: Config> Pallet<T> {
        fn validate_heartbeats(
            node: NodeId<T>,
            reward_period_index: RewardPeriodIndex,
            heartbeat_count: u64,
        ) -> DispatchResult {
            ensure!(<NodeRegistry<T>>::contains_key(&node), Error::<T>::NodeNotRegistered);
            let reward_period = RewardPeriod::<T>::get();
            let current_reward_period = reward_period.current;
            let maybe_uptime_info = <NodeUptime<T>>::get(reward_period_index, &node);

            ensure!(current_reward_period == reward_period_index, Error::<T>::InvalidHeartbeat);

            if let Some(uptime_info) = maybe_uptime_info {
                ensure!(
                    uptime_info.count < reward_period.uptime_threshold as u64,
                    Error::<T>::HeartbeatThresholdReached
                );

                let expected_submission = uptime_info.last_reported +
                    BlockNumberFor::<T>::from(reward_period.heartbeat_period);
                ensure!(
                    frame_system::Pallet::<T>::block_number() >= expected_submission,
                    Error::<T>::DuplicateHeartbeat
                );
                ensure!(heartbeat_count == uptime_info.count, Error::<T>::InvalidHeartbeat);
            } else {
                ensure!(heartbeat_count == 0, Error::<T>::InvalidHeartbeat);
            }

            Ok(())
        }

        fn do_deregister_nodes(
            owner: &T::AccountId,
            nodes: &BoundedVec<NodeId<T>, MaxNodesToDeregister>,
        ) -> DispatchResult {
            for node in nodes {
                ensure!(<OwnedNodes<T>>::contains_key(owner, node), Error::<T>::NodeNotRegistered);

                let info = NodeRegistry::<T>::take(node).ok_or(Error::<T>::NodeNotRegistered)?;
                Self::remove_signing_key_index(node, &info.signing_key)?;

                <OwnedNodes<T>>::remove(owner, node);
                <OwnedNodesCount<T>>::mutate(owner, |count| *count = count.saturating_sub(1));
                <TotalRegisteredNodes<T>>::mutate(|n| *n = n.saturating_sub(1));
                let _ = info;

                Self::deposit_event(Event::NodeDeregistered {
                    owner: owner.clone(),
                    node: node.clone(),
                });
            }
            Ok(())
        }

        pub(crate) fn calculate_uptime_threshold(
            reward_period_length: u32,
            heartbeat_period: u32,
        ) -> u32 {
            let threshold = MinUptimeThreshold::<T>::get().unwrap_or(Self::get_default_threshold());

            let max_heartbeats = reward_period_length.saturating_div(heartbeat_period);
            threshold * max_heartbeats
        }

        fn do_register_node(
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
        ) -> DispatchResult {
            ensure!(!<NodeRegistry<T>>::contains_key(&node), Error::<T>::DuplicateNode);
            ensure!(
                !SigningKeyToNodeId::<T>::contains_key(&signing_key),
                Error::<T>::SigningKeyAlreadyInUse
            );
            ensure!(
                TotalRegisteredNodes::<T>::get() < T::MaxRegisteredNodes::get(),
                Error::<T>::MaxNodesReached
            );

            <OwnedNodes<T>>::insert(&owner, &node, ());
            <OwnedNodesCount<T>>::mutate(&owner, |count| *count = count.saturating_add(1));

            <TotalRegisteredNodes<T>>::mutate(|n| {
                *n = n.saturating_add(1);
            });

            Self::insert_signing_key_index(&node, &signing_key)?;

            let node_serial_number = Self::calculate_node_serial();

            <NodeRegistry<T>>::insert(
                &node,
                NodeInfo::<T::SignerId, T::AccountId>::new(
                    owner.clone(),
                    signing_key,
                    node_serial_number,
                ),
            );

            Self::deposit_event(Event::NodeRegistered { owner, node });

            Ok(())
        }

        pub fn calculate_node_serial() -> u32 {
            <NextNodeSerialNumber<T>>::mutate(|n| {
                let current = *n;
                *n = n.saturating_add(1);
                current
            })
        }

        pub fn offchain_signature_is_valid<D: Encode>(
            data: &D,
            signer: &T::SignerId,
            signature: &<T::SignerId as RuntimeAppPublic>::Signature,
        ) -> bool {
            let signature_valid =
                data.using_encoded(|encoded_data| signer.verify(&encoded_data, &signature));

            log::trace!(
                "🪲 Validating signature: [ data {:?} - account {:?} - signature {:?} ] Result: {}",
                data.encode(),
                signer.encode(),
                signature,
                signature_valid
            );
            return signature_valid
        }

        pub fn get_encoded_call_param(
            call: &<T as Config>::RuntimeCall,
        ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
            let call = match call.is_sub_type() {
                Some(call) => call,
                None => return None,
            };

            match call {
                Call::signed_register_node {
                    ref proof,
                    ref node,
                    ref owner,
                    ref signing_key,
                    ref block_number,
                } => {
                    let encoded_data = encode_signed_register_node_params::<T>(
                        &proof.relayer,
                        node,
                        owner,
                        signing_key,
                        block_number,
                    );

                    Some((proof, encoded_data))
                },
                Call::signed_deregister_nodes {
                    ref proof,
                    ref owner,
                    ref nodes_to_deregister,
                    ref block_number,
                } => {
                    let encoded_data = encode_signed_deregister_node_params::<T>(
                        &proof.relayer,
                        owner,
                        nodes_to_deregister,
                        &(nodes_to_deregister.len() as u32),
                        block_number,
                    );

                    Some((proof, encoded_data))
                },
                Call::heartbeat_for_owned_nodes { ref proof, ref nodes, ref block_number } => {
                    let encoded_data = encode_aggregate_heartbeat_params::<T>(
                        &proof.relayer,
                        nodes,
                        &(nodes.len() as u32),
                        block_number,
                    );

                    Some((proof, encoded_data))
                },
                _ => None,
            }
        }

        pub fn get_default_threshold() -> Perbill {
            Perbill::from_percent(33)
        }

        /// Insert signing key reverse index. Fails if key already belongs to another node.
        fn insert_signing_key_index(node: &NodeId<T>, signing_key: &T::SignerId) -> DispatchResult {
            if let Some(existing_node) = SigningKeyToNodeId::<T>::get(signing_key) {
                ensure!(&existing_node == node, Error::<T>::SigningKeyAlreadyInUse);
                // If it already maps to this node, do nothing.
                return Ok(())
            }

            SigningKeyToNodeId::<T>::insert(signing_key, node);
            Ok(())
        }

        /// Remove signing key reverse index. Defensive: only remove if it points at this node.
        fn remove_signing_key_index(node: &NodeId<T>, signing_key: &T::SignerId) -> DispatchResult {
            if let Some(existing_node) = SigningKeyToNodeId::<T>::get(signing_key) {
                ensure!(&existing_node == node, Error::<T>::InvalidSigningKey);
                SigningKeyToNodeId::<T>::remove(signing_key);
            }
            Ok(())
        }

        fn rotate_signing_key_index(
            node: &NodeId<T>,
            old_key: &T::SignerId,
            new_key: &T::SignerId,
        ) -> DispatchResult {
            if old_key == new_key {
                return Ok(())
            }

            Self::remove_signing_key_index(node, old_key)?;
            Self::insert_signing_key_index(node, new_key)?;
            Ok(())
        }
    }

    impl<T: Config> InnerCallValidator for Pallet<T> {
        type Call = <T as Config>::RuntimeCall;

        fn signature_is_valid(call: &Box<Self::Call>) -> bool {
            if let Some((proof, signed_payload)) = Self::get_encoded_call_param(call) {
                return verify_signature::<T::Signature, T::AccountId>(
                    &proof,
                    &signed_payload.as_slice(),
                )
                .is_ok()
            }

            false
        }
    }
}

pub fn encode_signed_register_node_params<T: Config>(
    relayer: &T::AccountId,
    node: &NodeId<T>,
    owner: &T::AccountId,
    signing_key: &T::SignerId,
    block_number: &BlockNumberFor<T>,
) -> Vec<u8> {
    (SIGNED_REGISTER_NODE_CONTEXT, relayer.clone(), node, owner, signing_key, block_number).encode()
}

pub fn encode_aggregate_heartbeat_params<T: Config>(
    relayer: &T::AccountId,
    nodes: &BoundedVec<NodeId<T>, T::MaxNodesPerAggregateHeartbeat>,
    number_of_nodes: &u32,
    block_number: &BlockNumberFor<T>,
) -> Vec<u8> {
    (AGGREGATE_HEARTBEAT_CONTEXT, relayer.clone(), nodes, number_of_nodes, block_number).encode()
}

pub fn encode_signed_deregister_node_params<T: Config>(
    relayer: &T::AccountId,
    owner: &T::AccountId,
    nodes_to_deregister: &BoundedVec<NodeId<T>, MaxNodesToDeregister>,
    number_of_nodes_to_deregister: &u32,
    block_number: &BlockNumberFor<T>,
) -> Vec<u8> {
    (
        SIGNED_DEREGISTER_NODE_CONTEXT,
        relayer.clone(),
        owner,
        nodes_to_deregister,
        number_of_nodes_to_deregister,
        block_number,
    )
        .encode()
}
