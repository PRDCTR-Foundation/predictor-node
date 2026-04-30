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
        ConstBool, ConstU128, ConstU32, ConstU64, ConstU8, Currency, EitherOfDiverse,
        KeyOwnerProofSystem, OnUnbalanced, VariantCountOf,
    },
    weights::{
        constants::{ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
        IdentityFee, Weight, WeightToFeeCoefficient, WeightToFeePolynomial,
    },
    Blake2_256, PalletId,
};
use frame_system::EnsureRoot;

use frame_support::weights::WeightToFeeCoefficients;
use frame_system::limits::{BlockLength, BlockWeights};
use pallet_grandpa::AuthorityId as GrandpaId;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::crypto::KeyTypeId;
use sp_runtime::{
    traits::One, transaction_validity::TransactionPriority, FixedU128, Perbill, Percent,
};
use sp_version::RuntimeVersion;

pub use prediction_market_primitives::{constants::*, types::*};

pub use common_primitives::constants::{
    currency::*, BLOCKS_PER_DAY, BLOCKS_PER_HOUR, BLOCKS_PER_YEAR, MILLISECS_PER_BLOCK,
    NODE_MANAGER_PALLET_ID,
};

use crate::{asset_registry::CustomAssetProcessor, BlakeTwo256};
use pallet_collective::{EnsureProportionMoreThan, PrimeDefaultVote};
use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use pallet_pm_combinatorial_tokens::types::{CryptographicIdManager, Fuel};
use pallet_prediction_markets::CustomMetadata;
// Local module imports
use super::{
    AccountId, Amount, AssetManager, AssetRegistry, Aura, Authorized, AuthorsManager, Avn, Balance,
    Balances, Block, BlockNumber, CombinatorialTokens, Court, EthBridge, GlobalDisputes, Hash,
    Historical, ImOnline, MarketCommons, NeoSwaps, Nonce, Offences, Orderbook, OriginCaller,
    PalletConfig, PalletInfo, PredictionMarkets, Preimage, RandomnessCollectiveFlip, Runtime,
    RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason, RuntimeOrigin, RuntimeTask,
    Scheduler, SessionKeys, Signature, Summary, System, Timestamp, TokenManager, Tokens,
    UncheckedExtrinsic, EXISTENTIAL_DEPOSIT, MINUTES, SLOT_DURATION, VERSION,
};
use orml_traits::{parameter_type_with_key, LockIdentifier};
use smallvec::smallvec;
use sp_runtime::traits::{ConvertInto, OpaqueKeys};
use sp_watchtower::NoopWatchtower;

mod proxy_config;
use proxy_config::ProxyType;
// TODO uncomment to enable avn-proxy configuration
// mod avn_proxy_config;
// use avn_proxy_config::AvnProxyConfig;

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

impl pallet_avn_proxy::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type ProxyConfig = AvnProxyConfig;
    type WeightInfo = pallet_avn_proxy::default_weights::SubstrateWeight<Runtime>;
    type FeeHandler = TokenManager;
    type Token = EthAddress;
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
// TODO update me
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

// TODO update me
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

type EnsureRootOrMoreThanHalfAdvisoryCommittee = EitherOfDiverse<
    EnsureRoot<AccountId>,
    EnsureProportionMoreThan<AccountId, AdvisoryCommitteeInstance, 1, 2>,
>;

type EnsureRootOrMoreThanOneThirdAdvisoryCommittee = EitherOfDiverse<
    EnsureRoot<AccountId>,
    EnsureProportionMoreThan<AccountId, AdvisoryCommitteeInstance, 1, 3>,
>;

// More than 66%
type EnsureRootOrMoreThanTwoThirdsAdvisoryCommittee = EitherOfDiverse<
    EnsureRoot<AccountId>,
    EnsureProportionMoreThan<AccountId, AdvisoryCommitteeInstance, 2, 3>,
>;

use crate::impl_market_creator_fees;
impl_market_creator_fees!();

// Prediction market runtime API implementations would go here.
impl pallet_pm_market_commons::Config for Runtime {
    type Balance = Balance;
    type MarketId = MarketId;
    type Timestamp = Timestamp;
}

parameter_types! {
    // Asset registry
    pub const AssetRegistryStringLimit: u32 = 1024;
}

impl pallet_pm_eth_asset_registry::Config for Runtime {
    type AssetId = CurrencyId;
    type AuthorityOrigin = EnsureRoot<AccountId>;
    type Balance = Balance;
    type CustomMetadata = CustomMetadata;
    type RuntimeEvent = RuntimeEvent;
    type StringLimit = AssetRegistryStringLimit;
    type AssetProcessor = CustomAssetProcessor;
    type WeightInfo = ();
}

// Prediction Market parameters
parameter_types! {
    /// (Slashable) Bond that is provided for creating an advised market that needs approval.
    /// Slashed in case the market is rejected.
    pub const AdvisoryBond: Balance = 100 * BASE;
    /// The percentage of the advisory bond that gets slashed when a market is rejected.
    pub const AdvisoryBondSlashPercentage: Percent = Percent::from_percent(0);
    /// (Slashable) Bond that is provided for disputing an early market close by the market creator.
    pub const CloseEarlyDisputeBond: Balance = 2_000 * BASE;
    // Fat-finger protection for the advisory committe to reject
    // the early market schedule.
    pub const CloseEarlyProtectionTimeFramePeriod: Moment = CloseEarlyProtectionBlockPeriod::get() as u64 * MILLISECS_PER_BLOCK as u64;
    // Fat-finger protection for the advisory committe to reject
    // the early market schedule.
    pub const CloseEarlyProtectionBlockPeriod: BlockNumber = 12 * BLOCKS_PER_HOUR;
    /// (Slashable) Bond that is provided for scheduling an early market close.
    pub const CloseEarlyRequestBond: Balance = 2_000 * BASE;
    /// (Slashable) Bond that is provided for disputing the outcome.
    /// Unreserved in case the dispute was justified otherwise slashed.
    /// This is when the resolved outcome is different to the default (reported) outcome.
    pub const DisputeBond: Balance = 2_000 * BASE;
    /// Maximum number of disputes.
    pub const MaxDisputes: u16 = 1;
    /// The dispute_duration is time where users can dispute the outcome.
    /// Minimum block period for a dispute.
    pub const MinDisputeDuration: BlockNumber = MIN_DISPUTE_DURATION;
    /// Maximum block period for a dispute.
    pub const MaxDisputeDuration: BlockNumber = MAX_DISPUTE_DURATION;
    /// Maximum Categories a prediciton market can have (excluding base asset).
    pub const MaxCategories: u16 = MAX_CATEGORIES;
    /// Max creator fee, bounds the fraction per trade volume that is moved to the market creator.
    pub const MaxCreatorFee: Perbill = Perbill::from_percent(1);
    /// Maximum string length for edit reason.
    pub const MaxEditReasonLen: u32 = 1024;
    /// Maximum block period for a grace_period.
    /// The grace_period is a delay between the point where the market closes and the point where the oracle may report.
    pub const MaxGracePeriod: BlockNumber = MAX_GRACE_PERIOD;
    /// The maximum allowed duration of a market from creation to market close in blocks.
    pub const MaxMarketLifetime: BlockNumber = MAX_MARKET_LIFETIME;
    /// Maximum block period for an oracle_duration.
    /// The oracle_duration is a duration where the oracle has to submit its report.
    pub const MaxOracleDuration: BlockNumber = MAX_ORACLE_DURATION;
    /// Maximum string length allowed for reject reason.
    pub const MaxRejectReasonLen: u32 = 1024;
    /// Minimum number of categories. The trivial minimum is 2, which represents a binary market.
    pub const MinCategories: u16 = 2;
    /// Minimum block period for an oracle_duration.
    pub const MinOracleDuration: BlockNumber = MIN_ORACLE_DURATION;
    /// (Slashable) The orcale bond. Slashed in case the final outcome does not match the
    /// outcome the oracle reported.
    pub const OracleBond: Balance = 100 * BASE;
    /// (Slashable) A bond for an outcome reporter, who is not the oracle.
    /// Slashed in case the final outcome does not match the outcome by the outsider.
    // If we remove the whitelist restriction for market creation, review this figure and ensure its > OracleBond
    pub const OutsiderBond: Balance = 2000 * BASE;
    /// Pallet identifier, mainly used for named balance reserves. DO NOT CHANGE.
    pub const PmPalletId: PalletId = PM_PALLET_ID;
    // Waiting time for market creator to close the market after an early close schedule.
    pub const CloseEarlyBlockPeriod: BlockNumber = 5 * BLOCKS_PER_DAY;
    pub const CloseEarlyTimeFramePeriod: Moment = CloseEarlyBlockPeriod::get() as u64 * MILLISECS_PER_BLOCK as u64;
    /// (Slashable) A bond for creation markets that do not require approval. Slashed in case
    /// the market is forcefully destroyed.
    // The low amount is assuming only whitelisted accounts can create a market
    pub const ValidityBond: Balance = 100 * BASE;
    // Orderbook parameters
    pub const OrderbookPalletId: PalletId = ORDERBOOK_PALLET_ID;
    // Hybrid Router parameters
    pub const HybridRouterPalletId: PalletId = HYBRID_ROUTER_PALLET_ID;
    /// Maximum number of orders that can be placed in a single trade transaction.
    pub const MaxOrders: u32 = 100;
    /// The percentage of winning we deduct from the winner.
    pub const WinnerFeePercentage: Perbill = Perbill::from_percent(5);
}

use crate::impl_winner_fees;
impl_winner_fees!();

impl pallet_prediction_markets::Config for Runtime {
    type AdvisoryBond = AdvisoryBond;
    type AdvisoryBondSlashPercentage = AdvisoryBondSlashPercentage;
    type ApproveOrigin = EnsureRootOrMoreThanOneThirdAdvisoryCommittee;
    type Authorized = Authorized;
    type Currency = Balances;
    type Court = Court;
    type CloseEarlyDisputeBond = CloseEarlyDisputeBond;
    type CloseMarketEarlyOrigin = EnsureRootOrMoreThanOneThirdAdvisoryCommittee;
    type CloseOrigin = EnsureRoot<AccountId>;
    type CloseEarlyProtectionTimeFramePeriod = CloseEarlyProtectionTimeFramePeriod;
    type CloseEarlyProtectionBlockPeriod = CloseEarlyProtectionBlockPeriod;
    type CloseEarlyRequestBond = CloseEarlyRequestBond;
    type DeployPool = NeoSwaps;
    type DisputeBond = DisputeBond;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type GlobalDisputes = GlobalDisputes;
    type MaxCategories = MaxCategories;
    type MaxCreatorFee = MaxCreatorFee;
    type MaxDisputes = MaxDisputes;
    type MaxMarketLifetime = MaxMarketLifetime;
    type MinDisputeDuration = MinDisputeDuration;
    type MaxDisputeDuration = MaxDisputeDuration;
    type MaxGracePeriod = MaxGracePeriod;
    type MaxOracleDuration = MaxOracleDuration;
    type MinOracleDuration = MinOracleDuration;
    type MinCategories = MinCategories;
    type MaxEditReasonLen = MaxEditReasonLen;
    type MaxRejectReasonLen = MaxRejectReasonLen;
    type OracleBond = OracleBond;
    type OutsiderBond = OutsiderBond;
    type PalletId = PmPalletId;
    type CloseEarlyBlockPeriod = CloseEarlyBlockPeriod;
    type CloseEarlyTimeFramePeriod = CloseEarlyTimeFramePeriod;
    type RejectOrigin = EnsureRootOrMoreThanTwoThirdsAdvisoryCommittee;
    type RequestEditOrigin = EnsureRootOrMoreThanOneThirdAdvisoryCommittee;
    type ResolveOrigin = EnsureRoot<AccountId>;
    type AssetManager = AssetManager;
    type Slash = Treasury<Runtime>;
    type ValidityBond = ValidityBond;
    type WeightInfo = pallet_prediction_markets::weights::WeightInfo<Runtime>;
    type AssetRegistry = AssetRegistry;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type TokenInterface = TokenManager;
    type WinnerFeePercentage = WinnerFeePercentage;
    type WinnerFeeHandler = WinnerFee;
}

parameter_types! {
    // Authorized
    pub const AuthorizedPalletId: PalletId = AUTHORIZED_PALLET_ID;
    pub const CorrectionPeriod: BlockNumber = BLOCKS_PER_DAY;

}

impl pallet_pm_authorized::Config for Runtime {
    type AuthorizedDisputeResolutionOrigin = EnsureRootOrMoreThanHalfAdvisoryCommittee;
    type Currency = Balances;
    type CorrectionPeriod = CorrectionPeriod;
    type DisputeResolution = pallet_prediction_markets::Pallet<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type MarketCommons = MarketCommons;
    type PalletId = AuthorizedPalletId;
    type WeightInfo = pallet_pm_authorized::weights::WeightInfo<Runtime>;
}

parameter_types! {
    // Court
    /// (Slashable) Bond that is provided for overriding the last appeal.
    /// This bond increases exponentially with the number of appeals.
    /// Slashed in case the final outcome does match the appealed outcome for which the `AppealBond`
    /// was deposited.
    pub const AppealBond: Balance = 2000 * BASE;
    /// The blocks per year required to calculate the yearly inflation for court incentivisation.
    pub const BlocksPerYear: BlockNumber = BLOCKS_PER_YEAR;
    /// Pallet identifier, mainly used for named balance reserves. DO NOT CHANGE.
    pub const CourtPalletId: PalletId = COURT_PALLET_ID;
    /// The time in which the jurors can cast their secret vote.
    pub const CourtVotePeriod: BlockNumber = 3 * BLOCKS_PER_DAY;
    /// The time in which the jurors should reveal their secret vote.
    pub const CourtAggregationPeriod: BlockNumber = 3 * BLOCKS_PER_DAY;
    /// The time in which a court case can get appealed.
    pub const CourtAppealPeriod: BlockNumber = BLOCKS_PER_DAY;
    /// The lock identifier for the court votes.
    pub const CourtLockId: LockIdentifier = COURT_LOCK_ID;
    /// The time in which the inflation is periodically issued.
    pub const InflationPeriod: BlockNumber = 30 * BLOCKS_PER_DAY;
    /// The maximum number of appeals until the court fails.
    pub const MaxAppeals: u32 = 4;
    /// The maximum number of delegations per juror account.
    pub const MaxDelegations: u32 = 5;
    /// The maximum number of randomly selected `MinJurorStake` draws / atoms of jurors for a dispute.
    pub const MaxSelectedDraws: u32 = 510;
    /// The maximum number of jurors / delegators that can be registered.
    pub const MaxCourtParticipants: u32 = 1_000;
    /// The maximum yearly inflation for court incentivisation.
    pub const MaxYearlyInflation: Perbill = Perbill::from_percent(10);
    /// The minimum stake a user needs to reserve to become a juror.
    pub const MinJurorStake: Balance = 500 * BASE;
    /// The interval for requesting multiple court votes at once.
    pub const RequestInterval: BlockNumber = 7 * BLOCKS_PER_DAY;
}

impl pallet_pm_court::Config for Runtime {
    type AppealBond = AppealBond;
    type BlocksPerYear = BlocksPerYear;
    type VotePeriod = CourtVotePeriod;
    type AggregationPeriod = CourtAggregationPeriod;
    type AppealPeriod = CourtAppealPeriod;
    type LockId = CourtLockId;
    type PalletId = CourtPalletId;
    type Currency = Balances;
    type DisputeResolution = pallet_prediction_markets::Pallet<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type InflationPeriod = InflationPeriod;
    type MarketCommons = MarketCommons;
    type MaxAppeals = MaxAppeals;
    type MaxDelegations = MaxDelegations;
    type MaxSelectedDraws = MaxSelectedDraws;
    type MaxCourtParticipants = MaxCourtParticipants;
    type MaxYearlyInflation = MaxYearlyInflation;
    type MinJurorStake = MinJurorStake;
    type MonetaryGovernanceOrigin = EnsureRoot<AccountId>;
    type Random = RandomnessCollectiveFlip;
    type RequestInterval = RequestInterval;
    type Slash = Treasury<Runtime>;
    type TreasuryPalletId = AvnTreasuryPotId;
    type WeightInfo = pallet_pm_court::weights::WeightInfo<Runtime>;
}

// Global disputes parameters
parameter_types! {
    pub const AddOutcomePeriod: BlockNumber = 20;
    pub const GlobalDisputeLockId: LockIdentifier = GLOBAL_DISPUTES_LOCK_ID;
    pub const GlobalDisputesPalletId: PalletId = GLOBAL_DISPUTES_PALLET_ID;
    pub const MaxGlobalDisputeVotes: u32 = 50;
    pub const MaxOwners: u32 = 10;
    pub const MinOutcomeVoteAmount: Balance = 10 * CENT_BASE;
    pub const RemoveKeysLimit: u32 = 250;
    pub const GdVotingPeriod: BlockNumber = 140;
    pub const VotingOutcomeFee: Balance = 100 * CENT_BASE;
}

impl pallet_pm_global_disputes::Config for Runtime {
    type AddOutcomePeriod = AddOutcomePeriod;
    type Currency = Balances;
    type DisputeResolution = pallet_prediction_markets::Pallet<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type GlobalDisputeLockId = GlobalDisputeLockId;
    type GlobalDisputesPalletId = GlobalDisputesPalletId;
    type MarketCommons = MarketCommons;
    type MaxGlobalDisputeVotes = MaxGlobalDisputeVotes;
    type MaxOwners = MaxOwners;
    type MinOutcomeVoteAmount = MinOutcomeVoteAmount;
    type RemoveKeysLimit = RemoveKeysLimit;
    type GdVotingPeriod = GdVotingPeriod;
    type VotingOutcomeFee = VotingOutcomeFee;
    type WeightInfo = pallet_pm_global_disputes::weights::WeightInfo<Runtime>;
}

parameter_types! {
    // NeoSwaps
    pub const NeoSwapsMaxSwapFee: Balance = 10 * CENT_BASE;
    pub const NeoSwapsPalletId: PalletId = NS_PALLET_ID;
    pub const MaxLiquidityTreeDepth: u32 = 9u32;
    pub const MaxSplits: u16 = 128u16;

}

impl pallet_pm_neo_swaps::Config for Runtime {
    type CombinatorialId = CombinatorialId;
    type CombinatorialTokens = CombinatorialTokens;
    type CombinatorialTokensUnsafe = CombinatorialTokens;
    type CompleteSetOperations = PredictionMarkets;
    type ExternalFees = AdditionalSwapFee;
    type MarketCommons = MarketCommons;
    type MultiCurrency = AssetManager;
    type PoolId = MarketId;
    type MaxSplits = MaxSplits;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_pm_neo_swaps::weights::WeightInfo<Runtime>;
    type MaxLiquidityTreeDepth = MaxLiquidityTreeDepth;
    type MaxSwapFee = NeoSwapsMaxSwapFee;
    type PalletId = NeoSwapsPalletId;
    type SignedTxLifetime = ConstU32<16>;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type PalletAdminGetter = PredictionMarkets;
    type OnLiquidityProvided = PredictionMarkets;
}

impl pallet_pm_order_book::Config for Runtime {
    type AssetManager = AssetManager;
    type ExternalFees = AdditionalSwapFee;
    type RuntimeEvent = RuntimeEvent;
    type MarketCommons = MarketCommons;
    type PalletId = OrderbookPalletId;
    type WeightInfo = pallet_pm_order_book::weights::WeightInfo<Runtime>;
}

impl pallet_pm_hybrid_router::Config for Runtime {
    type AssetManager = AssetManager;
    #[cfg(feature = "runtime-benchmarks")]
    type AmmPoolDeployer = NeoSwaps;
    #[cfg(feature = "runtime-benchmarks")]
    type CompleteSetOperations = PredictionMarkets;
    type MarketCommons = MarketCommons;
    type Amm = NeoSwaps;
    type Orderbook = Orderbook;
    type MaxOrders = MaxOrders;
    type RuntimeEvent = RuntimeEvent;
    type PalletId = HybridRouterPalletId;
    type RuntimeCall = RuntimeCall;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type WeightInfo = pallet_pm_hybrid_router::weights::WeightInfo<Runtime>;
}

impl pallet_config::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_config::default_weights::SubstrateWeight<Runtime>;
}

parameter_types! {
        // CombinatorialTokens
        pub const CombinatorialTokensPalletId: PalletId = COMBINATORIAL_TOKENS_PALLET_ID;
}

impl pallet_pm_combinatorial_tokens::Config for Runtime {
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = PredictionMarketsCombinatorialTokensBenchmarkHelper<Runtime>;
    type CombinatorialIdManager = CryptographicIdManager<MarketId, Blake2_256>;
    type Fuel = Fuel;
    type MarketCommons = MarketCommons;
    type MultiCurrency = AssetManager;
    type Payout = PredictionMarkets;
    type RuntimeEvent = RuntimeEvent;
    type PalletId = CombinatorialTokensPalletId;
    type WeightInfo = pallet_pm_combinatorial_tokens::weights::WeightInfo<Runtime>;
}

// To split to another file
use crate::impl_fee_types;
impl_fee_types!();

/// ORML adapter
pub type BasicCurrencyAdapter<R, B> =
    orml_currencies::BasicCurrencyAdapter<R, B, OrmlAmount, Balance>;
pub type CurrencyId = Asset<MarketId>;

pub type NegativeImbalance<T> = <pallet_balances::Pallet<T> as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

pub struct Treasury<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for Treasury<R>
where
    R: pallet_balances::Config + pallet_token_manager::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_nonzero_unbalanced(amount: NegativeImbalance<R>) {
        let recipient: <R as frame_system::Config>::AccountId = PalletConfig::gas_fee_recipient()
            .map(Into::into)
            .unwrap_or_else(|_| <pallet_token_manager::Pallet<R>>::compute_treasury_account_id());

        <pallet_balances::Pallet<R>>::resolve_creating(&recipient, amount);
    }
}

pub struct DealWithFees<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for DealWithFees<R>
where
    R: pallet_balances::Config + pallet_token_manager::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_unbalanceds<B>(mut fees_then_tips: impl Iterator<Item = NegativeImbalance<R>>) {
        if let Some(mut fees) = fees_then_tips.next() {
            if let Some(tips) = fees_then_tips.next() {
                tips.merge_into(&mut fees);
            }

            // 100% of fees + tips goes to the treasury
            <Treasury<R> as OnUnbalanced<_>>::on_unbalanced(fees);
        }
    }
}

/// Handles converting a weight scalar to a fee value, based on the scale and granularity of the
/// node's balance type.
///
/// This should typically create a mapping between the following ranges:
///   - `[0, MAXIMUM_BLOCK_WEIGHT]`
///   - `[Balance::min, Balance::max]`
///
/// Yet, it can be used for any other sort of change to weight-fee. Some examples being:
///   - Setting it to `0` will essentially disable the weight fee.
///   - Setting it to `1` will cause the literal `#[weight = x]` values to be charged.
pub struct WeightToFee;
impl WeightToFeePolynomial for WeightToFee {
    type Balance = Balance;
    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        // We adjust the fee conversion so that a simple token transfer
        // direct to chain costs base_fee TRUU.
        let base_fee = PalletConfig::base_gas_fee();

        // The magic number (2.380951) is the result of :
        // setting p = 50 * MILLI_BASE, the cost of a simple transfer was 119.04775 milli TRUU
        // (visual observation on polkadot.js). magic_number = 119.04775 / 50 = 2.380951
        let factor = FixedU128::saturating_from_rational(1_000_000u128, 2_380_951u128);

        let p = factor.saturating_mul_int(base_fee);
        let q = Balance::from(ExtrinsicBaseWeight::get().ref_time());
        smallvec![WeightToFeeCoefficient {
            degree: 1,
            negative: false,
            coeff_frac: Perbill::from_rational(p % q, q),
            coeff_integer: p / q,
        }]
    }
}
