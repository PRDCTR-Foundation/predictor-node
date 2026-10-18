// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Modified for PRDCTR on 2026-07-07.

// No storage mutation allowed in this file
use crate::*;
use sp_runtime::traits::UniqueSaturatedInto;

pub const OCW_ID: &[u8; 22] = b"node_manager::last_run";

/// Upper bound, in blocks, between two heartbeat attempts of the same signing key (2 minutes at
/// 6s blocks). It leaves time for a submitted heartbeat to reach the chain before the key retries.
pub const HEARTBEAT_ATTEMPT_INTERVAL: u32 = 20;

impl<T: Config> Pallet<T> {
    /// Sends a heartbeat for every registered node whose signing key is in the local keystore,
    /// is in its attempt slot at `block_number` and is due a heartbeat on-chain.
    pub fn send_heartbeat_if_required(block_number: BlockNumberFor<T>) {
        let mut local_keys: Vec<T::SignerId> = T::SignerId::all();
        if local_keys.is_empty() {
            log::debug!("🔐 No node signing keys found in the keystore.");
            return
        }
        // Slot assignment relies on a stable order across OCW runs.
        local_keys.sort();
        local_keys.dedup();

        let reward_period = RewardPeriod::<T>::get();
        let block: u64 = block_number.unique_saturated_into();
        let interval = heartbeat_attempt_interval(reward_period.heartbeat_period);

        for (_, signing_key) in local_keys
            .iter()
            .enumerate()
            .filter(|(index, _)| is_attempt_slot(*index, interval, block))
        {
            if let Some(node) = Self::registered_node_for_key(signing_key) {
                Self::send_node_heartbeat_if_due(block_number, &reward_period, node, signing_key);
            }
        }
    }

    /// Mirrors the on-chain heartbeat checks, so the chain state acts as the submission lock.
    pub fn should_send_heartbeat(
        block_number: BlockNumberFor<T>,
        uptime_info: Option<UptimeInfo<BlockNumberFor<T>>>,
        reward_period: &RewardPeriodInfo<BlockNumberFor<T>>,
    ) -> bool {
        match uptime_info {
            Some(uptime_info) => {
                let below_threshold = uptime_info.count < reward_period.uptime_threshold as u64;
                below_threshold &&
                    block_number >=
                        uptime_info.last_reported.saturating_add(BlockNumberFor::<T>::from(
                            reward_period.heartbeat_period,
                        ))
            },
            // First heartbeat
            None => true,
        }
    }

    /// Resolves the node registered with `signing_key`, ignoring stale reverse-index entries.
    fn registered_node_for_key(signing_key: &T::SignerId) -> Option<NodeId<T>> {
        let node = SigningKeyToNodeId::<T>::get(signing_key)?;
        match NodeRegistry::<T>::get(&node) {
            Some(info) if info.signing_key == *signing_key => Some(node),
            _ => {
                log::warn!(
                    "🔐 Signing key is indexed to node {:?} but the registry does not match.",
                    hex::encode(node.encode())
                );
                None
            },
        }
    }

    fn send_node_heartbeat_if_due(
        block_number: BlockNumberFor<T>,
        reward_period: &RewardPeriodInfo<BlockNumberFor<T>>,
        node: NodeId<T>,
        signing_key: &T::SignerId,
    ) {
        let current_reward_period = reward_period.current;
        let uptime_info = <NodeUptime<T>>::get(current_reward_period, &node);
        if !Self::should_send_heartbeat(block_number, uptime_info, reward_period) {
            return
        }
        let heartbeat_count = uptime_info.map(|info| info.count).unwrap_or(0);

        let node_hex = hex::encode(node.encode());
        log::info!(
            "🛠️  Sending heartbeat for node: {node_hex}, reward period: {current_reward_period:?}, block number: {block_number:?}"
        );

        let Some(signature) =
            signing_key.sign(&(HEARTBEAT_CONTEXT, heartbeat_count, current_reward_period).encode())
        else {
            log::error!(
                "💔 Error signing heartbeat transaction. Node: {node_hex}, Reward period: {current_reward_period:?}, Block number: {block_number:?}");
            return
        };

        let call = Call::<T>::offchain_submit_heartbeat {
            node,
            reward_period_index: current_reward_period,
            heartbeat_count,
            signature,
        };

        match SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(call.into()) {
            Ok(_) => log::info!(
                "🛠️  heartbeat transaction sent. Node: {node_hex}, Reward period: {current_reward_period:?}, Block number: {block_number:?}"),
            Err(e) => log::error!(
                "💔 Error submitting heartbeat transaction. Node: {node_hex}, Period: {current_reward_period:?}, Heartbeat count: {heartbeat_count:?}, Error: {e:?}"),
        }
    }
}

/// Blocks between two attempts of the same key: `HEARTBEAT_ATTEMPT_INTERVAL`, capped at half the
/// heartbeat period so a key gets at least two attempts per period.
pub(crate) fn heartbeat_attempt_interval(heartbeat_period: u32) -> u64 {
    HEARTBEAT_ATTEMPT_INTERVAL.min(heartbeat_period / 2).max(1) as u64
}

/// Whether the key at `key_index` of the sorted local keys may attempt a heartbeat at `block`.
/// Keys are spread round-robin over the blocks of each `interval`.
pub(crate) fn is_attempt_slot(key_index: usize, interval: u64, block: u64) -> bool {
    let interval = interval.max(1);
    (key_index as u64) % interval == block % interval
}
