// This is free and unencumbered software released into the public domain.
//
// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.
//
// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.
//
// For more information, please refer to <http://unlicense.org>

// Substrate and Polkadot dependencies
use frame_support::{
    derive_impl, parameter_types,
    traits::{
        ConstBool, ConstU128, ConstU32, ConstU64, ConstU8, KeyOwnerProofSystem, VariantCountOf,
    },
    weights::{
        constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
        IdentityFee, Weight,
    },
};
use frame_system::limits::{BlockLength, BlockWeights};
use pallet_grandpa::AuthorityId as GrandpaId;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::crypto::KeyTypeId;
use sp_runtime::{traits::One, transaction_validity::TransactionPriority, Perbill};
use sp_version::RuntimeVersion;

use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use common_primitives::constants::BLOCKS_PER_DAY;
use pallet_collective::{EnsureProportionMoreThan, PrimeDefaultVote};
use frame_system::EnsureRoot;

// Local module imports
use super::{
    AccountId, Amount, Aura, AuthorsManager, Avn, Balance, Balances, Block, BlockNumber,
    CurrencyId, EthBridge, Hash, Historical, ImOnline, Nonce, Offences, OriginCaller, PalletInfo,
    Preimage, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason,
    RuntimeOrigin, RuntimeTask, Scheduler, SessionKeys, Signature, Summary, System, Timestamp,
    TokenManager, Tokens, UncheckedExtrinsic, EXISTENTIAL_DEPOSIT, MINUTES, SLOT_DURATION, VERSION,
};
use orml_traits::parameter_type_with_key;
use sp_runtime::traits::{ConvertInto, OpaqueKeys};
use sp_watchtower::NoopWatchtower;

use crate::BlakeTwo256;

mod proxy_config;
use proxy_config::ProxyType;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
    pub const BlockHashCount: BlockNumber = 2400;
    pub const Version: RuntimeVersion = VERSION;

    /// We allow for 2 seconds of compute with a 6 second average block time.
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
        Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        NORMAL_DISPATCH_RATIO,
    );
    pub RuntimeBlockLength: BlockLength = BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
    pub const SS58Prefix: u8 = 42;
}

pub use common_primitives::constants::currency::*;

/// The default types are being injected by [`derive_impl`](`frame_support::derive_impl`) from
/// [`SoloChainDefaultConfig`](`struct@frame_system::config_preludes::SolochainDefaultConfig`),
/// but overridden as needed.
#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
    /// The block type for the runtime.
    type Block = Block;
    /// Block & extrinsics weights: base values and limits.
    type BlockWeights = RuntimeBlockWeights;
    /// The maximum length of a block (in bytes).
    type BlockLength = RuntimeBlockLength;
    /// The identifier used to distinguish between accounts.
    type AccountId = AccountId;
    /// The type for storing how many extrinsics an account has signed.
    type Nonce = Nonce;
    /// The type for hashing blocks and tries.
    type Hash = Hash;
    /// Maximum number of block number to block hash mappings to keep (oldest pruned first).
    type BlockHashCount = BlockHashCount;
    /// The weight of database operations that the runtime can invoke.
    type DbWeight = RocksDbWeight;
    /// Version of the runtime.
    type Version = Version;
    /// The data to be stored in an account.
    type AccountData = pallet_balances::AccountData<Balance>;
    /// This is used as an identifier of the chain. 42 is the generic substrate prefix.
    type SS58Prefix = SS58Prefix;
    type MaxConsumers = frame_support::traits::ConstU32<16>;
}

impl pallet_aura::Config for Runtime {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = ConstU32<32>;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
    type SlotDuration = pallet_aura::MinimumPeriodTimesTwo<Runtime>;
}

parameter_types! {
    pub const MaxAuthorities: u32 = 32;
    pub const ReportLongevity: u64 = SessionPeriod::get() as u64 * 2u64;
}

impl pallet_grandpa::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type MaxAuthorities = MaxAuthorities;
    type MaxNominators = MaxAuthorities;
    type MaxSetIdSessionEntries = ConstU64<0>;
    type KeyOwnerProof = <Historical as KeyOwnerProofSystem<(KeyTypeId, GrandpaId)>>::Proof;
    type EquivocationReportSystem =
        pallet_grandpa::EquivocationReportSystem<Self, Offences, Historical, ReportLongevity>;
}

impl pallet_timestamp::Config for Runtime {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = u64;
    type OnTimestampSet = Aura;
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_balances::Config for Runtime {
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ();
    type ReserveIdentifier = [u8; 8];
    /// The type for recording an account's balance.
    type Balance = Balance;
    /// The ubiquitous event type.
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
    type AccountStore = System;
    type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
    type FreezeIdentifier = RuntimeFreezeReason;
    type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeHoldReason;
}

parameter_types! {
    pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = FungibleAdapter<Balances, ()>;
    type OperationalFeeMultiplier = ConstU8<5>;
    type WeightToFee = IdentityFee<Balance>;
    type LengthToFee = IdentityFee<Balance>;
    type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
}

impl pallet_sudo::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const SessionPeriod: BlockNumber = MINUTES; // 60 blocks
    pub const SessionOffset: BlockNumber = 0;
}

impl pallet_session::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type ShouldEndSession = pallet_session::PeriodicSessions<SessionPeriod, SessionOffset>;
    type NextSessionRotation = pallet_session::PeriodicSessions<SessionPeriod, SessionOffset>;
    type SessionManager = AuthorsManager;
    type SessionHandler = <SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
    type Keys = SessionKeys;
    type WeightInfo = ();
}

impl pallet_session::historical::Config for Runtime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_offences::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type IdentificationTuple = pallet_session::historical::IdentificationTuple<Self>;
    type OnOffenceHandler = ();
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime
where
    RuntimeCall: From<C>,
{
    type Extrinsic = UncheckedExtrinsic;
    type OverarchingCall = RuntimeCall;
}

impl pallet_preimage::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type Currency = Balances;
    type ManagerOrigin = frame_system::EnsureRoot<AccountId>;
    type Consideration = ();
}

parameter_types! {
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        RuntimeBlockWeights::get().max_block;
}

impl pallet_scheduler::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeOrigin = RuntimeOrigin;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = frame_system::EnsureRoot<AccountId>;
    type OriginPrivilegeCmp = frame_support::traits::EqualPrivilegeOnly;
    type MaxScheduledPerBlock = ConstU32<50>;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
    type Preimages = Preimage;
}

parameter_types! {
    pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
    pub const MaxKeys: u32 = 10_000;
    pub const MaxPeerInHeartbeats: u32 = 10_000;
}

impl pallet_im_online::Config for Runtime {
    type AuthorityId = ImOnlineId;
    type RuntimeEvent = RuntimeEvent;
    type NextSessionRotation = pallet_session::PeriodicSessions<SessionPeriod, SessionOffset>;
    type ValidatorSet = Historical;
    type ReportUnresponsiveness = Offences;
    type UnsignedPriority = ImOnlineUnsignedPriority;
    type WeightInfo = pallet_im_online::weights::SubstrateWeight<Runtime>;
    type MaxKeys = MaxKeys;
    type MaxPeerInHeartbeats = MaxPeerInHeartbeats;
}

impl pallet_authorship::Config for Runtime {
    type FindAuthor = pallet_session::FindAccountFromAuthorIndex<Self, Aura>;
    type EventHandler = ImOnline;
}

impl pallet_authority_discovery::Config for Runtime {
    type MaxAuthorities = MaxAuthorities;
}

impl pallet_utility::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

// Multisig pallet config start
parameter_types! {
    // One storage item; key size is 32; value is size 4+4+16+32 bytes = 56 bytes.
    pub const DepositBase: Balance = deposit(1, 88);
    // Additional storage item size of 32 bytes.
    pub const DepositFactor: Balance = deposit(0, 32);
    pub const MaxSignatories: u32 = 100;
}

impl pallet_multisig::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type DepositBase = DepositBase;
    type DepositFactor = DepositFactor;
    type MaxSignatories = ConstU32<100>;
    type WeightInfo = pallet_multisig::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    // One storage item; key size 32, value size 8; .
    pub const ProxyDepositBase: Balance = deposit(1, 8);
    // Additional storage item size of 33 bytes.
    pub const ProxyDepositFactor: Balance = deposit(0, 33);
    pub const MaxProxies: u16 = 32;
    pub const AnnouncementDepositBase: Balance = deposit(1, 8);
    pub const AnnouncementDepositFactor: Balance = deposit(0, 66);
    pub const MaxPending: u16 = 32;
}
impl pallet_proxy::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProxyType = ProxyType;
    type ProxyDepositBase = ProxyDepositBase;
    type ProxyDepositFactor = ProxyDepositFactor;
    type MaxProxies = MaxProxies;
    type WeightInfo = pallet_proxy::weights::SubstrateWeight<Runtime>;
    type MaxPending = MaxPending;
    type CallHasher = BlakeTwo256;
    type AnnouncementDepositBase = AnnouncementDepositBase;
    type AnnouncementDepositFactor = AnnouncementDepositFactor;
}

impl pallet_avn::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = pallet_avn::sr25519::AuthorityId;
    type EthereumPublicKeyChecker = AuthorsManager;
    type NewSessionHandler = AuthorsManager;
    type DisabledValidatorChecker = ();
    type WeightInfo = pallet_avn::default_weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const MinimumAuthorsCount: u32 = 2;
}

impl pallet_authors_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AccountToBytesConvert = Avn;
    type ValidatorRegistrationNotifier = ();
    type WeightInfo = pallet_authors_manager::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type MinimumAuthorsCount = MinimumAuthorsCount;
}

parameter_types! {
    pub const AdvanceSlotGracePeriod: BlockNumber = 5;
    pub const MinBlockAge: BlockNumber = 5;
    pub const AutoSubmitSummaries: bool = false;
    pub const SummaryInstanceId: u8 = 1;
    pub const ExternalValidationEnabled: bool = false;
}

impl pallet_summary::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = Avn;
    type ReportSummaryOffence = Offences;
    type WeightInfo = pallet_summary::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = AutoSubmitSummaries;
    type InstanceId = SummaryInstanceId;
    type ExternalValidationEnabled = ExternalValidationEnabled;
    type ExternalValidator = NoopWatchtower<AccountId>;
}

parameter_types! {
    pub const MinEthBlockConfirmation: u64 = 20;
}

impl pallet_eth_bridge::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type TimeProvider = Timestamp;
    type MaxQueuedTxRequests = ConstU32<100>;
    type MinEthBlockConfirmation = MinEthBlockConfirmation;
    type AccountToBytesConvert = Avn;
    type BridgeInterfaceNotification = (Summary, AuthorsManager, TokenManager);
    type ReportCorroborationOffence = Offences;
    type ProcessedEventsChecker = ();
    type EthereumEventsMigration = ();
    type ProcessedEventsHandler = ();
    type Quorum = Avn;
    type WeightInfo = pallet_eth_bridge::default_weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const AvnTreasuryPotId: frame_support::PalletId = frame_support::PalletId(*b"Treasury");
    pub const TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
}

impl pallet_token_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type TokenBalance = Balance;
    type TokenId = sp_core::H160;
    type ProcessedEventsChecker = ();
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type OnGrowthLiftedHandler = ();
    type TreasuryGrowthPercentage = TreasuryGrowthPercentage;
    type AvnTreasuryPotId = AvnTreasuryPotId;
    type WeightInfo = pallet_token_manager::default_weights::SubstrateWeight<Runtime>;
    type Scheduler = Scheduler;
    type Preimages = Preimage;
    type PalletsOrigin = OriginCaller;
    type BridgeInterface = EthBridge;
    type OnIdleHandler = ();
    type AccountToBytesConvert = Avn;
}

parameter_type_with_key! {
    pub ExistentialDeposits: |_currency_id: CurrencyId| -> Balance {
        EXISTENTIAL_DEPOSIT
    };
}

pub struct DustRemovalWhitelist;
impl frame_support::traits::Contains<AccountId> for DustRemovalWhitelist {
    fn contains(_a: &AccountId) -> bool {
        false
    }
}

impl orml_tokens::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Balance = Balance;
    type Amount = Amount;
    type CurrencyId = CurrencyId;
    type WeightInfo = ();
    type ExistentialDeposits = ExistentialDeposits;
    type CurrencyHooks = ();
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ConstU32<50>;
    type ReserveIdentifier = [u8; 8];
    type DustRemovalWhitelist = DustRemovalWhitelist;
}

parameter_types! {
    pub const GetNativeCurrencyId: CurrencyId = CurrencyId::Native;
}

impl orml_currencies::Config for Runtime {
    type MultiCurrency = Tokens;
    type NativeCurrency =
        orml_currencies::BasicCurrencyAdapter<Runtime, Balances, Amount, BlockNumber>;
    type GetNativeCurrencyId = GetNativeCurrencyId;
    type WeightInfo = ();
}

type AdvisoryCommitteeInstance = pallet_collective::Instance1;

// Prediction market
impl pallet_insecure_randomness_collective_flip::Config for Runtime {}

parameter_types! {
    // Note: MaxMembers does not influence the pallet logic, but the worst-case weight
    // estimation.     
    pub const AdvisoryCommitteeMaxMembers: u32 = 100;
    // The maximum of proposals is currently u8::MAX otherwise the pallet_collective benchmark
    // fails
    pub const AdvisoryCommitteeMaxProposals: u32 = 255;
    pub const AdvisoryCommitteeMotionDuration: BlockNumber = 3 * BLOCKS_PER_DAY;
    pub MaxProposalWeight: Weight = Perbill::from_percent(50) *
RuntimeBlockWeights::get().max_block; }

impl pallet_collective::Config<AdvisoryCommitteeInstance> for Runtime {
    type DefaultVote = PrimeDefaultVote;
    type RuntimeEvent = RuntimeEvent;
    type MaxMembers = AdvisoryCommitteeMaxMembers;
    type MaxProposals = AdvisoryCommitteeMaxProposals;
    type MaxProposalWeight = MaxProposalWeight;
    type MotionDuration = AdvisoryCommitteeMotionDuration;
    type RuntimeOrigin = RuntimeOrigin;
    type SetMembersOrigin = EnsureRoot<AccountId>;
    type Proposal = RuntimeCall;
    type WeightInfo = pallet_collective::weights::SubstrateWeight<Runtime>;
}
