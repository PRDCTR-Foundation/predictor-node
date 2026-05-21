// Copyright 2026 Aventus DAO Ltd

use crate::*;
use frame_support::traits::Get;
use sp_runtime::{FixedPointNumber, FixedU128, Saturating};
// This is used to scale a single heartbeat so we can preserve precision when applying the reward
// weight.
pub const HEARTBEAT_BASE_WEIGHT: u128 = 100_000_000;
pub type Duration = u64;
pub type RewardPeriodIndex = u64;

/// Local mirror of sp_avn_common's TotalSupplyUpdatedData. Lives here while
/// the published sp_avn_common on the consumed branch lacks it; the pallet
/// only needs the struct shape, not the EventData wiring.
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct TotalSupplyUpdatedData {
    pub amount: u128,
    pub t2_tx_id: u32,
}

impl TotalSupplyUpdatedData {
    pub fn is_valid(&self) -> bool {
        self.amount > 0u128
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// The current era index and transition information
pub struct RewardPeriodInfo<BlockNumber, Balance> {
    /// Current era index
    pub current: RewardPeriodIndex,
    /// The first block of the current era
    pub first: BlockNumber,
    /// The length of the current era in number of blocks
    pub length: u32,
    // The length of the heartbeat period in blocks
    pub heartbeat_period: u32,
    /// The minimum number of uptime reports required to earn full reward
    pub uptime_threshold: u32,
    // Total reward amount for the period
    pub reward_amount: Balance,
}

impl<
        B: Copy
            + sp_std::ops::Add<Output = B>
            + sp_std::ops::Sub<Output = B>
            + From<u32>
            + PartialOrd
            + Saturating,
        Balance: Copy,
    > RewardPeriodInfo<B, Balance>
{
    pub fn new(
        current: RewardPeriodIndex,
        first: B,
        length: u32,
        heartbeat_period: u32,
        uptime_threshold: u32,
        reward_amount: Balance,
    ) -> Self {
        RewardPeriodInfo {
            current,
            first,
            length,
            heartbeat_period,
            uptime_threshold,
            reward_amount,
        }
    }

    /// Check if the reward period should be updated
    pub fn should_update(&self, now: B) -> bool {
        now.saturating_sub(self.first) >= self.length.into()
    }

    /// New reward period
    pub fn update(
        &self,
        now: B,
        length: u32,
        heartbeat_period: u32,
        uptime_threshold: u32,
        reward_amount: Balance,
    ) -> Self {
        let current = self.current.saturating_add(1u64);
        let first = now;
        Self { current, first, length, heartbeat_period, uptime_threshold, reward_amount }
    }
}

impl<
        B: Copy
            + sp_std::ops::Add<Output = B>
            + sp_std::ops::Sub<Output = B>
            + From<u32>
            + PartialOrd
            + Saturating,
        Balance: Default + Copy,
    > Default for RewardPeriodInfo<B, Balance>
{
    fn default() -> RewardPeriodInfo<B, Balance> {
        RewardPeriodInfo::new(0u64, 0u32.into(), 20u32, 10u32, u32::MAX, Default::default())
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct RewardPotInfo<Balance> {
    /// The total reward to pay out
    pub total_reward: Balance,
    /// The minimum number of uptime reports required to earn full reward
    pub uptime_threshold: u32,
    /// The last timestamp of the previous reward period, used to calculate genesis bonus
    pub reward_end_time: Duration,
}

impl<Balance: Copy> RewardPotInfo<Balance> {
    pub fn new(total_reward: Balance, uptime_threshold: u32, reward_end_time: Duration) -> Self {
        RewardPotInfo { total_reward, uptime_threshold, reward_end_time }
    }
}

#[derive(
    Copy,
    Clone,
    PartialEq,
    Default,
    Eq,
    Encode,
    Decode,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct UptimeInfo<BlockNumber> {
    /// Number of uptime reported
    pub count: u64,
    /// The weight of the node (including genesis bonus and stake multiplier)
    pub weight: u128,
    /// Block number when the uptime was last reported
    pub last_reported: BlockNumber,
}

impl<BlockNumber: Copy> UptimeInfo<BlockNumber> {
    pub fn new(count: u64, weight: u128, last_reported: BlockNumber) -> Self {
        UptimeInfo { count, weight, last_reported }
    }
}

#[derive(
    Encode,
    Decode,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct PaymentPointer<AccountId> {
    pub period_index: RewardPeriodIndex,
    pub node: AccountId,
}

impl<AccountId: Clone + FullCodec + MaxEncodedLen + TypeInfo> PaymentPointer<AccountId> {
    /// Return the *final* storage key for NodeUptime<(period, node)>.
    /// This positions iteration beyond (period,node), preventing double payments.
    pub fn get_final_key<T: Config<AccountId = AccountId>>(&self) -> Vec<u8> {
        crate::pallet::NodeUptime::<T>::storage_double_map_final_key(
            self.period_index,
            self.node.clone(),
        )
    }
}

#[derive(
    Encode,
    Decode,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct NodeInfo<SignerId, AccountId> {
    /// The node owner
    pub owner: AccountId,
    /// The node signing key
    pub signing_key: SignerId,
    /// serial number of the node
    pub serial_number: u32,
}

impl<
        AccountId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
        SignerId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
    > NodeInfo<SignerId, AccountId>
{
    pub fn new(
        owner: AccountId,
        signing_key: SignerId,
        serial_number: u32,
    ) -> NodeInfo<SignerId, AccountId> {
        NodeInfo { owner, signing_key, serial_number }
    }
}

#[derive(Encode, Decode, TypeInfo, Debug, Clone, PartialEq)]
pub enum AdminConfig<AccountId, Balance> {
    NodeRegistrar(AccountId),
    NextRewardPeriodLength(u32),
    BatchSize(u32),
    NextHeartbeatPeriod(u32),
    NextRewardAmountPerPeriod(Balance),
    NumPeriodsToMint(u32),
    RewardEnabled(bool),
    MinUptimeThreshold(Perbill),
    RewardFee(Perbill),
    GenesisBonus50(BonusRange),
    GenesisBonus25(BonusRange),
}

#[derive(
    Copy,
    Clone,
    PartialEq,
    Default,
    Eq,
    Encode,
    Decode,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct TotalUptimeInfo {
    /// Total number of uptime reported for reward period
    pub total_heartbeats: u64,
    /// Total weight of the total heartbeats reported for reward period
    pub total_weight: u128,
}

impl TotalUptimeInfo {
    pub fn new(total_heartbeats: u64, total_weight: u128) -> TotalUptimeInfo {
        TotalUptimeInfo { total_heartbeats, total_weight }
    }
}

#[derive(Clone, Copy)]
pub struct RewardWeight {
    pub genesis_bonus: FixedU128,
    pub stake_multiplier: FixedU128,
}

impl RewardWeight {
    pub fn to_heartbeat_weight(&self) -> u128 {
        let scaled_stake_weight = self.stake_multiplier.saturating_mul_int(HEARTBEAT_BASE_WEIGHT);
        // apply the bonus last to preserve precision.
        self.genesis_bonus.saturating_mul_int(scaled_stake_weight)
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct PendingMintRequest<Balance> {
    pub tx_id: EthereumId,
    pub amount: Balance,
    pub bridge_confirmed: bool,
    pub credit_received: bool,
}

#[derive(
    Encode,
    Decode,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct BonusRange {
    pub start: u32,
    pub end: u32,
}

impl BonusRange {
    pub fn new(start: u32, end: u32) -> Self {
        BonusRange { start, end }
    }

    pub fn contains(&self, n: &u32) -> bool {
        *n >= self.start && *n <= self.end
    }
}

pub struct DefaultGenesisBonus50;
impl Get<BonusRange> for DefaultGenesisBonus50 {
    fn get() -> BonusRange {
        BonusRange::new(3001, 6000)
    }
}

pub struct DefaultGenesisBonus25;
impl Get<BonusRange> for DefaultGenesisBonus25 {
    fn get() -> BonusRange {
        BonusRange::new(6001, 11000)
    }
}
