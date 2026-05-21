// Minimum runtime wiring for pallet-node-manager so the pallet's surviving PR2
// extrinsics (register_node, signed_register_node, deregister_nodes,
// signed_deregister_nodes, update_signing_key, offchain_submit_heartbeat,
// set_admin_config) can be exercised in zombienet.
//
// Off-plan: a full PR8 wiring requires TreasurySource / ForfeitureSink /
// HalvingInterval / LockDuration / PenaltyMax / PenaltyPerWeek / MaxLocksPerOwner
// / MaxNodesPerAggregateHeartbeat. None of those Config items exist on PR2 yet.
// This stub fills only what PR2's Config trait still requires; the reward-period
// rollover in on_initialize still depends on `RewardEnabled` being true and on
// the reward-pot account being pre-funded by hand for any payout to happen.

use frame_support::parameter_types;
use common_primitives::constants::NODE_MANAGER_PALLET_ID;

use crate::{Balances, Runtime, RuntimeCall, RuntimeEvent, Signature, Timestamp};

parameter_types! {
    pub const NodeManagerRewardPotId: frame_support::PalletId = NODE_MANAGER_PALLET_ID;
    pub const NodeManagerSignedTxLifetime: u32 = 64;
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
    type SignedTxLifetime = NodeManagerSignedTxLifetime;
    type WeightInfo = pallet_node_manager::default_weights::SubstrateWeight<Runtime>;
}
