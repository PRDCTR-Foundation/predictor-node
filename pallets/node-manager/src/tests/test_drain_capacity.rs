// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-08-27.

//! The reward period must always be long enough to drain the registry it pays.
//!
//! Background: `on_idle` only advances `OldestUnpaidRewardPeriodIndex` once its
//! iterator is exhausted. If a period rolls over before its predecessor has
//! finished draining, a fresh unpaid generation accrues on top of an unfinished
//! one, and nothing throttles the producer - the backlog is unbounded. This was
//! not hypothetical: a probe against a live 2-validator chain reproduced it
//! without trying, accumulating two unpaid generations while its first period
//! was still 70% undrained.
//!
//! The invariant that prevents it is
//!
//! ```text
//! total_registered_nodes <= reward_period_length * effective_drain_rate()
//! ```
//!
//! It can be broken from two directions, so both are guarded:
//!   - shortening the period / lowering `BatchSize` -> `set_admin_config`
//!   - growing the registry                        -> `register_node`
//!
//! Guarding only the first would be nearly useless: in production the config is
//! set once and the node count is what grows over time.

#![cfg(test)]

use crate::{
    tests::{mock, mock::*},
    *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    origin: RuntimeOrigin,
    owner: AccountId,
    node_id: AccountId,
    signing_key: <mock::TestRuntime as pallet::Config>::SignerId,
}

impl Default for Context {
    fn default() -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        <NodeRegistrar<TestRuntime>>::set(Some(registrar));

        Context {
            origin: RuntimeOrigin::signed(registrar),
            owner: TestAccount::new([101u8; 32]).account_id(),
            node_id: TestAccount::new([202u8; 32]).account_id(),
            signing_key: <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None),
        }
    }
}

/// Set the registry size the guards read, without the per-node key plumbing.
/// Both guards branch on `TotalRegisteredNodes`, so this drives exactly what
/// they check.
fn fake_registry_of(count: u32) {
    TotalRegisteredNodes::<TestRuntime>::put(count);
}

fn set_period_length(length: u32) {
    RewardPeriod::<TestRuntime>::mutate(|p| p.length = length);
}

// ---- the capacity arithmetic itself ----------------------------------------

#[test]
fn drain_rate_is_the_smaller_of_the_two_brakes() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let weight_brake = NodeManager::nodes_paid_per_idle_block();
        assert!(weight_brake > 0, "weight brake must admit progress");

        // Batch size well above the weight brake -> weight brake binds.
        MaxBatchSize::<TestRuntime>::put(1_000);
        assert_eq!(NodeManager::effective_drain_rate(), weight_brake);

        // Batch size below the weight brake -> batch size binds.
        MaxBatchSize::<TestRuntime>::put(1);
        assert_eq!(NodeManager::effective_drain_rate(), 1);
    });
}

#[test]
fn min_reward_period_rounds_up_not_down() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Pin the rate so the arithmetic is exact regardless of the mock's
        // block weight budget.
        MaxBatchSize::<TestRuntime>::put(1);
        assert_eq!(NodeManager::effective_drain_rate(), 1);
        assert_eq!(NodeManager::min_reward_period_for(10), 10);

        MaxBatchSize::<TestRuntime>::put(3);
        let rate = NodeManager::effective_drain_rate();
        if rate == 3 {
            // A partial block's worth of nodes still needs a whole block: 10
            // nodes at 3/block is 4 blocks, never 3.
            assert_eq!(NodeManager::min_reward_period_for(10), 4);
            assert_eq!(NodeManager::min_reward_period_for(9), 3);
            assert_eq!(NodeManager::min_reward_period_for(0), 0);
        }
    });
}

#[test]
fn a_zero_drain_rate_demands_an_impossible_period_rather_than_dividing_by_zero() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // `MaxBatchSize` is validated to be >= 1, so a zero rate is not
        // reachable through the extrinsic - but the helper must not panic or
        // silently report "any period is fine" if it ever were.
        MaxBatchSize::<TestRuntime>::put(0);
        assert_eq!(NodeManager::effective_drain_rate(), 0);
        assert_eq!(NodeManager::min_reward_period_for(1), u32::MAX);
        assert_eq!(NodeManager::drain_capacity(1_000_000), 0);
    });
}

// ---- guard 1: shortening the period ----------------------------------------

#[test]
fn cannot_set_a_reward_period_too_short_to_drain_the_registry() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        MaxBatchSize::<TestRuntime>::put(1); // rate = 1 node/block
        fake_registry_of(500);

        // 499 blocks cannot pay 500 nodes at 1/block.
        assert_noop!(
            NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::NextRewardPeriodLength(499)
            ),
            Error::<TestRuntime>::RewardPeriodTooShortToDrain
        );

        // Exactly enough is enough - the bound is inclusive.
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardPeriodLength(500)
        ));
        assert_eq!(NextRewardPeriodLength::<TestRuntime>::get(), 500);
    });
}

#[test]
fn an_empty_registry_permits_any_reward_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        fake_registry_of(0);
        let heartbeat = NextHeartbeatPeriod::<TestRuntime>::get();

        // Nothing to drain, so only the heartbeat ordering rule applies.
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardPeriodLength(heartbeat + 1)
        ));
    });
}

// ---- guard 2: lowering the batch size --------------------------------------

#[test]
fn cannot_lower_batch_size_below_what_the_current_period_needs() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        set_period_length(100);
        fake_registry_of(100);

        // At 1 node/block, 100 blocks pays exactly 100 nodes: allowed.
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::BatchSize(1)
        ));

        // Now grow the registry past what rate 1 can pay in 100 blocks, and
        // re-assert the same batch size: it must now be refused.
        fake_registry_of(101);
        assert_noop!(
            NodeManager::set_admin_config(RawOrigin::Root.into(), AdminConfig::BatchSize(1)),
            Error::<TestRuntime>::RewardPeriodTooShortToDrain
        );

        // A larger batch size raises the rate and makes it payable again -
        // provided the weight brake allows it.
        if NodeManager::nodes_paid_per_idle_block() >= 2 {
            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::BatchSize(2)
            ));
        }
    });
}

// ---- guard 3: growing the registry -----------------------------------------

#[test]
fn cannot_register_beyond_the_reward_periods_drain_capacity() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let context = Context::default();
        MaxBatchSize::<TestRuntime>::put(1);
        set_period_length(10); // capacity = 10 nodes

        // Sitting exactly at capacity, one more node is refused.
        fake_registry_of(10);
        assert_noop!(
            NodeManager::register_node(
                context.origin.clone(),
                context.node_id,
                context.owner,
                context.signing_key.clone(),
            ),
            Error::<TestRuntime>::RewardPeriodTooShortForRegistry
        );

        // Lengthening the period raises capacity and unblocks registration -
        // the error is actionable, not terminal.
        set_period_length(11);
        assert_ok!(NodeManager::register_node(
            context.origin.clone(),
            context.node_id,
            context.owner,
            context.signing_key.clone(),
        ));
        assert_eq!(TotalRegisteredNodes::<TestRuntime>::get(), 11);
    });
}

#[test]
fn the_node_cap_still_takes_precedence_over_drain_capacity() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let context = Context::default();
        let cap = <TestRuntime as pallet::Config>::MaxRegisteredNodes::get();

        // Capacity is ample; the registry is at the hard cap. The caller should
        // learn it hit the CAP, not be misdirected to the reward period.
        ExtBuilder::set_reward_period_for_nodes(cap);
        fake_registry_of(cap);

        assert_noop!(
            NodeManager::register_node(
                context.origin.clone(),
                context.node_id,
                context.owner,
                context.signing_key.clone(),
            ),
            Error::<TestRuntime>::MaxNodesReached
        );
    });
}

#[test]
fn a_production_shaped_period_never_binds_before_the_cap() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // The point of the whole design: with a sensibly long reward period the
        // registration guard is inert and the hard cap is what callers meet.
        // If this ever fails, the guard has started rejecting legitimate
        // production registrations.
        let cap = <TestRuntime as pallet::Config>::MaxRegisteredNodes::get();
        MaxBatchSize::<TestRuntime>::put(1_000);
        let one_day_of_blocks = 14_400u32;
        set_period_length(one_day_of_blocks);

        assert!(
            NodeManager::drain_capacity(one_day_of_blocks) >= cap,
            "a one-day reward period must be able to drain a full {cap}-node registry; \
             capacity was {}",
            NodeManager::drain_capacity(one_day_of_blocks),
        );
    });
}
