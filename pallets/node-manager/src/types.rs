// Copyright 2026 Aventus DAO Ltd

use crate::*;
use sp_runtime::Saturating;
// This is used to scale a single heartbeat so we can preserve precision when applying the reward
// weight.
pub const HEARTBEAT_BASE_WEIGHT: u128 = 100_000_000;
pub type Duration = u64;
pub type RewardPeriodIndex = u64;
pub const SECONDS_PER_WEEK: Duration = 7 * 24 * 60 * 60;

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
    /// `true` when the rollover treasury transfer for this period failed, so the
    /// snapshot exists with `total_reward == 0` and is awaiting recovery via
    /// `top_up_reward_pot`. Distinguishes a recoverable failed-funding period
    /// from a legitimately zero-reward period (which the drain may skip).
    pub funding_failed: bool,
}

impl<Balance: Copy> RewardPotInfo<Balance> {
    pub fn new(
        total_reward: Balance,
        uptime_threshold: u32,
        reward_end_time: Duration,
        funding_failed: bool,
    ) -> Self {
        RewardPotInfo { total_reward, uptime_threshold, reward_end_time, funding_failed }
    }
}

#[derive(
    Copy, Clone, PartialEq, Default, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen,
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

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
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

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
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
    RewardEnabled(bool),
    MinUptimeThreshold(Perbill),
    LockSchedule(LockScheduleInfo),
    ForfeitureDestination(AccountId),
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// The single global reward-lock window. Rewards paid out while the window is
/// active accumulate in `LockedRewards` instead of the owner's free balance;
/// withdrawing early forfeits a percentage that decays by 1% per elapsed week
/// (52% in week one -> 0% from week 53 on, mirroring the T1 migration-claim
/// schedule). Once the penalty reaches zero the lock is expired and payouts
/// credit free balance directly again.
pub struct LockScheduleInfo {
    /// Window anchor as a unix timestamp in seconds (the migration
    /// "Global Start Date"). A start in the future charges the week-one rate.
    pub start: Duration,
    /// Forfeiture percentage during the first week (52 per the proposal).
    pub initial_penalty_percent: u32,
}

impl LockScheduleInfo {
    pub fn new(start: Duration, initial_penalty_percent: u32) -> Self {
        LockScheduleInfo { start, initial_penalty_percent }
    }

    /// Forfeiture rate for a withdrawal happening at `now` (unix seconds).
    /// Decays by one percentage point per full week elapsed since `start`.
    pub fn penalty_at(&self, now: Duration) -> Perbill {
        let elapsed_weeks = now.saturating_sub(self.start) / SECONDS_PER_WEEK;
        let percent = self
            .initial_penalty_percent
            .saturating_sub(elapsed_weeks.min(u32::MAX as u64) as u32);
        Perbill::from_percent(percent)
    }

    /// The lock no longer withholds anything once the penalty hits zero.
    pub fn is_expired(&self, now: Duration) -> bool {
        self.penalty_at(now).is_zero()
    }
}

#[derive(
    Copy, Clone, PartialEq, Default, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen,
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
