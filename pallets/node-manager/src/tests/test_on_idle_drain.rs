// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_ok, weights::Weight};
use frame_system::RawOrigin;

/// Per-iteration weight that the drain charges. Tests that need a small budget
/// pick a multiple of this to constrain how many nodes drain in one shot.
fn per_iter() -> Weight {
    NodeManager::worst_case_iteration_weight()
}

fn setup_registrar() -> AccountId {
    let registrar = TestAccount::new([1u8; 32]).account_id();
    NodeRegistrar::<TestRuntime>::set(Some(registrar));
    registrar
}

fn register_node(registrar: AccountId, owner_seed: u8, node_seed: u8, key_seed: u8) -> AccountId {
    let owner = TestAccount::new([owner_seed; 32]).account_id();
    let node = TestAccount::new([node_seed; 32]).account_id();
    let key = UintAuthorityId(key_seed as u64);
    assert_ok!(NodeManager::register_node(
        RawOrigin::Signed(registrar).into(),
        node,
        owner,
        key,
    ));
    node
}

/// Inject a NodeUptime entry directly. The mock has no live OCW so a heartbeat
/// would be unsigned/invalid; the drain logic only reads the storage so the
/// shortcut is valid.
fn record_uptime(period: RewardPeriodIndex, node: &AccountId, count: u64) {
    let weight = HEARTBEAT_BASE_WEIGHT.saturating_mul(count as u128);
    NodeUptime::<TestRuntime>::insert(
        period,
        node,
        UptimeInfo::new(count, weight, System::block_number()),
    );
    TotalUptime::<TestRuntime>::mutate(period, |t| {
        t.total_heartbeats = t.total_heartbeats.saturating_add(count);
        t.total_weight = t.total_weight.saturating_add(weight);
    });
}

/// Configure the chain for fast-rolling reward periods so a single
/// `roll_forward` triggers `on_initialize` rollover and seeds the period
/// snapshot we want to drain.
fn fast_periods() {
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::NextRewardPeriodLength(20),
    ));
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::NextHeartbeatPeriod(5),
    ));
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::NextRewardAmountPerPeriod(1_000 * PRD),
    ));
    // Generous batch cap so the per-test scenarios don't accidentally hit it.
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::BatchSize(64),
    ));
}

/// Set MaxBatchSize directly (bypassing admin invariants) for tests that
/// want a small cap. AdminConfig::BatchSize bounds to [1, 1000] so this is
/// only useful for picking values inside that range.
fn set_batch_size(n: u32) {
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::BatchSize(n),
    ));
}

/// With `with_genesis_config()` the chain starts on period 0 with
/// `length=200` and `reward_amount=20 PRD`. `fast_periods` sets the NEXT
/// period to `length=20, amount=1000 PRD`. This helper crosses the first
/// rollover so the chain is in period 1 (the new config), records uptime
/// for caller-supplied nodes, then crosses the second rollover so period 1
/// is snapshot-funded and ready to be drained.
///
/// Returns the period index that's now the oldest unpaid (period 0 is also
/// snapshotted but with total_weight=0 so the drain will skip it).
fn setup_unpaid_period_with_nodes(nodes_with_uptime: &[(AccountId, u64)]) -> RewardPeriodIndex {
    // Cross period 0 boundary (length=200 from genesis).
    roll_forward(200);
    // We're now in period 1 (the post-rollover config).
    let period_to_pay = RewardPeriod::<TestRuntime>::get().current;
    for (node, count) in nodes_with_uptime {
        record_uptime(period_to_pay, node, *count);
    }
    // Cross period 1 boundary (length=20 from fast_periods).
    roll_forward(20);
    period_to_pay
}

#[test]
fn drain_pays_all_nodes_within_one_block() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        let n1 = register_node(registrar, 101, 1, 11);
        let n2 = register_node(registrar, 102, 2, 12);
        let n3 = register_node(registrar, 103, 3, 13);

        let period = setup_unpaid_period_with_nodes(&[(n1, 5), (n2, 3), (n3, 2)]);

        // Owners are fresh accounts: zero free balance before payout.
        let owners: Vec<AccountId> = [101u8, 102, 103]
            .iter()
            .map(|s| TestAccount::new([*s; 32]).account_id())
            .collect();
        for owner in &owners {
            assert_eq!(Balances::free_balance(owner), 0, "owner should start unfunded");
        }
        let pot_before = NodeManager::reward_pot_balance();
        assert!(pot_before > 0, "reward pot expected to be funded before drain");

        // Generous weight budget: pay all 3 nodes (and skip the empty period 0).
        let budget = per_iter().saturating_mul(20);
        let used = NodeManager::drain_outstanding_payouts(budget);
        assert!(used.any_gt(Weight::zero()), "expected non-zero used weight");

        // OldestUnpaidRewardPeriodIndex advanced past the paid period.
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            period.saturating_add(1),
            "period should be marked complete after draining all nodes",
        );

        // NodeUptime cleaned up for that period.
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, &n1));
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, &n2));
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, &n3));

        // Direct payout: each owner received their reward straight into free
        // balance (no lock), and the pot was drawn down by what was paid.
        let mut paid_total: u128 = 0;
        for owner in &owners {
            let bal = Balances::free_balance(owner);
            assert!(bal > 0, "owner expected a positive direct payout, got {}", bal);
            paid_total = paid_total.saturating_add(bal);
        }
        let pot_after = NodeManager::reward_pot_balance();
        assert_eq!(
            pot_before.saturating_sub(pot_after),
            paid_total,
            "pot drawdown ({}) should equal the sum paid to owners ({})",
            pot_before.saturating_sub(pot_after),
            paid_total,
        );
    });
}

#[test]
fn drain_respects_max_batch_size() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        let nodes = (0..5u8)
            .map(|i| register_node(registrar, 130 + i, 1 + i, 30 + i))
            .collect::<Vec<_>>();
        let with_uptime: Vec<_> = nodes.iter().map(|n| (*n, 3u64)).collect();
        let period = setup_unpaid_period_with_nodes(&with_uptime);

        // Tighten the cap AFTER setup_unpaid_period_with_nodes (which set a
        // generous default) so the test can observe the per-block cap kicking in.
        set_batch_size(2);

        // First call: skips empty period 0, then pays 2 of the 5 nodes in period 1.
        let budget = per_iter().saturating_mul(100); // plenty of weight
        let _ = NodeManager::drain_outstanding_payouts(budget);

        // Period 1 not yet complete - 2 of 5 paid.
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            period,
            "period 1 should still be the oldest unpaid after a single capped drain",
        );
        let ptr = LastPaidPointer::<TestRuntime>::get();
        assert!(ptr.is_some(), "LastPaidPointer should be set after a partial drain");
        assert_eq!(ptr.as_ref().unwrap().period_index, period);

        let remaining = NodeUptime::<TestRuntime>::iter_prefix(period).count();
        assert_eq!(remaining, 3);

        // Subsequent calls finish the period.
        let _ = NodeManager::drain_outstanding_payouts(budget); // pays next 2
        let _ = NodeManager::drain_outstanding_payouts(budget); // pays last 1, completes
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            period.saturating_add(1),
        );
        assert!(LastPaidPointer::<TestRuntime>::get().is_none());
    });
}

#[test]
fn drain_respects_weight_budget() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        let nodes = (0..4u8)
            .map(|i| register_node(registrar, 140 + i, 20 + i, 50 + i))
            .collect::<Vec<_>>();
        let with_uptime: Vec<_> = nodes.iter().map(|n| (*n, 3u64)).collect();
        let period = setup_unpaid_period_with_nodes(&with_uptime);

        // Budget = 3 iterations: 1 for skipping period 0, 2 for paying nodes.
        let budget = per_iter().saturating_mul(3);
        let used = NodeManager::drain_outstanding_payouts(budget);
        assert!(used.any_gt(Weight::zero()));
        // 2 nodes paid in period 1 -> 2 remain.
        let remaining = NodeUptime::<TestRuntime>::iter_prefix(period).count();
        assert_eq!(remaining, 2, "drain should have paid only 2 of 4 nodes");
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            period,
        );
    });
}

#[test]
fn drain_advances_past_empty_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        setup_registrar();
        fast_periods();
        // No nodes, no uptime - both snapshotted periods will have
        // total_uptime.total_weight == 0 (even after they get funded).
        roll_forward(200);
        roll_forward(20);
        let oldest_before = OldestUnpaidRewardPeriodIndex::<TestRuntime>::get();

        let budget = per_iter().saturating_mul(10);
        let _ = NodeManager::drain_outstanding_payouts(budget);

        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() > oldest_before,
            "empty period should be skipped past",
        );
    });
}

#[test]
fn drain_is_noop_when_rewards_disabled() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Disable rewards explicitly (ExtBuilder enables them by default).
        RewardEnabled::<TestRuntime>::put(false);
        let budget = per_iter().saturating_mul(10);
        // Run on_idle as the hook would: the early-exit guard returns Zero.
        let used = NodeManager::on_idle(System::block_number(), budget);
        assert_eq!(used, Weight::zero());
    });
}

#[test]
fn drain_is_noop_when_no_unpaid_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        setup_registrar();
        fast_periods();
        // No rollover yet: oldest unpaid == current.
        let budget = per_iter().saturating_mul(10);
        let used = NodeManager::drain_outstanding_payouts(budget);
        assert_eq!(used, Weight::zero(), "no period to drain -> no weight charged");
    });
}

#[test]
fn drain_pays_correct_amount_to_owners() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        let n1 = register_node(registrar, 121, 30, 70);
        let n2 = register_node(registrar, 122, 31, 71);
        let owner_a = TestAccount::new([121u8; 32]).account_id();
        let owner_b = TestAccount::new([122u8; 32]).account_id();
        // Fresh owners: zero free balance before payout.
        assert_eq!(Balances::free_balance(&owner_a), 0);
        assert_eq!(Balances::free_balance(&owner_b), 0);

        // Equal uptime. Use count=1: with default MinUptimeThreshold=33%
        // and max_heartbeats=4 (period 20 / heartbeat 5), uptime_threshold
        // works out to floor(0.33 * 4) = 1, so count<=1 stays below the cap
        // and each node's effective weight equals its uncapped weight.
        let period = setup_unpaid_period_with_nodes(&[(n1, 1), (n2, 1)]);
        let pot_info = RewardPot::<TestRuntime>::get(period).expect("pot must be funded");
        let total_reward = pot_info.total_reward;
        assert!(total_reward > 0, "period {} reward pot expected to be funded", period);

        let budget = per_iter().saturating_mul(20);
        let _ = NodeManager::drain_outstanding_payouts(budget);

        // Direct payout into free balance (no lock). Equal weight -> half each
        // (with rounding floor), so each owner's balance is the amount paid.
        let a_bal = Balances::free_balance(&owner_a);
        let b_bal = Balances::free_balance(&owner_b);
        let expected = total_reward / 2;
        assert!(
            a_bal == expected || a_bal + 1 == expected,
            "owner A paid {} expected ~{}",
            a_bal,
            expected,
        );
        assert!(
            b_bal == expected || b_bal + 1 == expected,
            "owner B paid {} expected ~{}",
            b_bal,
            expected,
        );
    });
}
