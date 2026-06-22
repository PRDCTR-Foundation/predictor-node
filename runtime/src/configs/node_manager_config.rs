// Runtime wiring for pallet-node-manager. Covers register/deregister/heartbeat
// (incl. delegated heartbeats) extrinsics, treasury-funded reward-period
// rollover with annual halving, and direct per-period reward payout.
//
// The reward-pot account is not pre-funded by hand for the happy path - on each
// period rollover the pallet pulls `reward_amount` from `TreasurySource` (the
// TokenManager treasury) into the pot account, and the `on_idle` drain pays
// each eligible node's share directly to its owner.

use common_primitives::constants::{BLOCKS_PER_YEAR, NODE_MANAGER_PALLET_ID};
use frame_support::parameter_types;
use sp_runtime::traits::Get;

use crate::{
    AccountId, Balances, BlockNumber, Runtime, RuntimeCall, RuntimeEvent, Signature, Timestamp,
};

parameter_types! {
    pub const NodeManagerRewardPotId: frame_support::PalletId = NODE_MANAGER_PALLET_ID;
    pub const NodeManagerSignedTxLifetime: u32 = 64;
    /// Halve `NextRewardAmountPerPeriod` annually (365.25 days at 6s blocks).
    pub const NodeManagerHalvingInterval: BlockNumber = BLOCKS_PER_YEAR;
    /// Halving stays OFF until root flips it via `set_halving_enabled`. This
    /// keeps the launch reward stable and gives ops a kill-switch.
    pub const NodeManagerHalvingEnabledAtGenesis: bool = false;
    /// Per-call cap on `heartbeat_for_owned_nodes`. Sized to comfortably
    /// cover a single validator's full owned-node set in one call without
    /// risking ExhaustsResources at the pre-dispatch weight check.
    pub const NodeManagerMaxNodesPerAggregateHeartbeat: u32 = 1024;
    /// Network-wide registered-node cap, fixed at 30,000 by the PRDCTR
    /// hard-fork proposal. Deregistration frees capacity under the cap.
    pub const NodeManagerMaxRegisteredNodes: u32 = 30_000;
    /// Recovery window (in reward periods) for a period whose rollover funding
    /// failed. The `on_idle` drain keeps such a period recoverable via
    /// `top_up_reward_pot` while its age stays within this window, then
    /// abandons it so the payout stream cannot freeze indefinitely behind one
    /// unfundable period. Sized to give governance ample time to source and
    /// inject top-up funds before the period is written off.
    pub const NodeManagerMaxFailedFundingRecoveryPeriods: u64 = 100;
}

/// Funding source for reward-period rollover: the TokenManager treasury
/// account. Reusing this account (rather than introducing a new pallet-owned
/// holding) keeps the rollover transfer auditable against the same balance the
/// gas-fee recipient and other treasury flows already write to.
pub struct NodeManagerTreasurySource;
impl Get<AccountId> for NodeManagerTreasurySource {
    fn get() -> AccountId {
        pallet_token_manager::Pallet::<Runtime>::compute_treasury_account_id()
    }
}

impl pallet_node_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type SignerId = pallet_node_manager::sr25519::AuthorityId;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type TimeProvider = Timestamp;
    type Signature = Signature;
    type RewardPotId = NodeManagerRewardPotId;
    type TreasurySource = NodeManagerTreasurySource;
    type HalvingInterval = NodeManagerHalvingInterval;
    type HalvingEnabledAtGenesis = NodeManagerHalvingEnabledAtGenesis;
    type MaxNodesPerAggregateHeartbeat = NodeManagerMaxNodesPerAggregateHeartbeat;
    type MaxRegisteredNodes = NodeManagerMaxRegisteredNodes;
    type MaxFailedFundingRecoveryPeriods = NodeManagerMaxFailedFundingRecoveryPeriods;
    type SignedTxLifetime = NodeManagerSignedTxLifetime;
    type WeightInfo = pallet_node_manager::default_weights::SubstrateWeight<Runtime>;
}
