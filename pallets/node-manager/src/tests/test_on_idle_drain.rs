// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{tests::mock::*, *};
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
    assert_ok!(NodeManager::register_node(RawOrigin::Signed(registrar).into(), node, owner, key,));
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
    assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), AdminConfig::BatchSize(64),));
}

/// Set MaxBatchSize directly (bypassing admin invariants) for tests that
/// want a small cap. AdminConfig::BatchSize bounds to [1, 1000] so this is
/// only useful for picking values inside that range.
fn set_batch_size(n: u32) {
    assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), AdminConfig::BatchSize(n),));
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
        // Expired lock window: payouts credit free balance directly.
        expire_lock_schedule();
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

        // Period 0 was funded at rollover but carries zero uptime, so the drain
        // reclaims its reward back to the treasury rather than stranding it.
        let reclaimed = RewardPot::<TestRuntime>::get(0).map(|p| p.total_reward).unwrap_or_default();

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
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, n1));
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, n2));
        assert!(!NodeUptime::<TestRuntime>::contains_key(period, n3));

        // Direct payout: each owner received their reward straight into free
        // balance (no lock), and the pot was drawn down by what was paid.
        let mut paid_total: u128 = 0;
        for owner in &owners {
            let bal = Balances::free_balance(owner);
            assert!(bal > 0, "owner expected a positive direct payout, got {bal}");
            paid_total = paid_total.saturating_add(bal);
        }
        let pot_after = NodeManager::reward_pot_balance();
        assert_eq!(
            pot_before.saturating_sub(pot_after),
            paid_total.saturating_add(reclaimed),
            "pot drawdown ({}) should equal the sum paid to owners ({}) plus the reclaimed empty-period reward ({})",
            pot_before.saturating_sub(pot_after),
            paid_total,
            reclaimed,
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
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period.saturating_add(1),);
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
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period,);
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
fn drain_reclaims_undistributed_reward_for_empty_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        setup_registrar();
        fast_periods();
        // No nodes => every funded period has zero uptime, so its reward is
        // undistributable and must be returned to the treasury, not stranded.
        roll_forward(200); // fund period 0 (genesis amount), enter period 1
        roll_forward(20); // fund period 1 (fast_periods amount), enter period 2

        let p0 = RewardPot::<TestRuntime>::get(0).map(|p| p.total_reward).unwrap_or_default();
        let p1 = RewardPot::<TestRuntime>::get(1).map(|p| p.total_reward).unwrap_or_default();
        let reclaimable = p0.saturating_add(p1);
        assert!(reclaimable > 0, "periods should have been funded at rollover");

        let treasury_before = Balances::free_balance(treasury_account());
        let pot_before = NodeManager::reward_pot_balance();
        let outstanding_before = OutstandingRewardToPay::<TestRuntime>::get();

        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(10));

        // Funds returned to the treasury, pot drawn down, outstanding cleared -
        // nothing stranded.
        assert_eq!(
            Balances::free_balance(treasury_account()).saturating_sub(treasury_before),
            reclaimable,
            "treasury should recover the undistributed reward",
        );
        assert_eq!(pot_before.saturating_sub(NodeManager::reward_pot_balance()), reclaimable);
        assert_eq!(
            outstanding_before.saturating_sub(OutstandingRewardToPay::<TestRuntime>::get()),
            reclaimable,
        );

        let events = System::events();
        assert!(
            events.iter().any(|er| matches!(
                &er.event,
                RuntimeEvent::NodeManager(Event::UndistributedRewardReclaimed { .. })
            )),
            "UndistributedRewardReclaimed event missing",
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
        // Expired lock window: payouts credit free balance directly.
        expire_lock_schedule();
        let n1 = register_node(registrar, 121, 30, 70);
        let n2 = register_node(registrar, 122, 31, 71);
        let owner_a = TestAccount::new([121u8; 32]).account_id();
        let owner_b = TestAccount::new([122u8; 32]).account_id();
        // Fresh owners: zero free balance before payout.
        assert_eq!(Balances::free_balance(owner_a), 0);
        assert_eq!(Balances::free_balance(owner_b), 0);

        // Equal uptime. Use count=1: with default MinUptimeThreshold=33%
        // and max_heartbeats=4 (period 20 / heartbeat 5), uptime_threshold
        // works out to floor(0.33 * 4) = 1, so count<=1 stays below the cap
        // and each node's effective weight equals its uncapped weight.
        let period = setup_unpaid_period_with_nodes(&[(n1, 1), (n2, 1)]);
        let pot_info = RewardPot::<TestRuntime>::get(period).expect("pot must be funded");
        let total_reward = pot_info.total_reward;
        assert!(total_reward > 0, "period {period} reward pot expected to be funded");

        let budget = per_iter().saturating_mul(20);
        let _ = NodeManager::drain_outstanding_payouts(budget);

        // Direct payout into free balance (no lock). Equal weight -> half each
        // (with rounding floor), so each owner's balance is the amount paid.
        let a_bal = Balances::free_balance(owner_a);
        let b_bal = Balances::free_balance(owner_b);
        let expected = total_reward / 2;
        assert!(
            a_bal == expected || a_bal + 1 == expected,
            "owner A paid {a_bal} expected ~{expected}",
        );
        assert!(
            b_bal == expected || b_bal + 1 == expected,
            "owner B paid {b_bal} expected ~{expected}",
        );
    });
}

#[test]
fn drain_keeps_failed_funding_within_recovery_window() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let window = <TestRuntime as Config>::MaxFailedFundingRecoveryPeriods::get();

        // Period 0 failed funding; the current period index is still within the
        // recovery window, so the drain must leave it recoverable and not
        // advance the cursor past it.
        OldestUnpaidRewardPeriodIndex::<TestRuntime>::put(0);
        RewardPot::<TestRuntime>::insert(0, RewardPotInfo::new(0u128, 20u32, 0u64, true));
        RewardPeriod::<TestRuntime>::mutate(|p| p.current = window);

        let used = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));
        assert_eq!(used, Weight::zero(), "drain should not charge weight while blocked");
        assert!(
            RewardPot::<TestRuntime>::get(0).is_some(),
            "in-window failed-funding snapshot must stay recoverable",
        );
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            0,
            "cursor must not advance while period is within the recovery window",
        );
    });
}

#[test]
fn drain_abandons_failed_funding_past_recovery_window_and_pays_later_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        expire_lock_schedule();
        let window = <TestRuntime as Config>::MaxFailedFundingRecoveryPeriods::get();
        let reward = 1_000 * PRD;

        // Period 0: failed funding, never recovered.
        OldestUnpaidRewardPeriodIndex::<TestRuntime>::put(0);
        RewardPot::<TestRuntime>::insert(0, RewardPotInfo::new(0u128, 20u32, 0u64, true));

        // Period 2: funded successfully with real uptime for one node.
        let node = register_node(registrar, 101, 1, 11);
        let pot = NodeManager::compute_reward_account_id();
        let _ = Balances::deposit_creating(&pot, reward);
        RewardPot::<TestRuntime>::insert(2, RewardPotInfo::new(reward, 20u32, 0u64, false));
        OutstandingRewardToPay::<TestRuntime>::put(reward);
        record_uptime(2, &node, 5);

        // Push the current period index past period 0 + the recovery window so
        // the unrecovered failed-funding period must be abandoned.
        RewardPeriod::<TestRuntime>::mutate(|p| p.current = window + 2);

        let owner = TestAccount::new([101u8; 32]).account_id();
        assert_eq!(Balances::free_balance(owner), 0, "owner starts unfunded");

        let used = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));
        assert!(used.any_gt(Weight::zero()), "drain should make progress");

        // Period 0 abandoned: snapshot removed and cursor advanced past it.
        assert!(
            RewardPot::<TestRuntime>::get(0).is_none(),
            "out-of-window failed-funding snapshot must be abandoned",
        );
        // Liveness: the later funded period's operator actually gets paid.
        assert!(
            Balances::free_balance(owner) > 0,
            "later period operator must be paid once the stuck period is abandoned",
        );
        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() > 2,
            "cursor must advance past the paid period",
        );
    });
}

#[test]
fn drain_abandons_failed_funding_clears_node_uptime_in_batches() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        expire_lock_schedule();
        let window = <TestRuntime as Config>::MaxFailedFundingRecoveryPeriods::get();
        let reward = 1_000 * PRD;

        // Period 0: funding failed at rollover, but three nodes had already
        // recorded heartbeats into NodeUptime[0] during the period.
        OldestUnpaidRewardPeriodIndex::<TestRuntime>::put(0);
        RewardPot::<TestRuntime>::insert(0, RewardPotInfo::new(0u128, 20u32, 0u64, true));
        let n1 = TestAccount::new([21u8; 32]).account_id();
        let n2 = TestAccount::new([22u8; 32]).account_id();
        let n3 = TestAccount::new([23u8; 32]).account_id();
        record_uptime(0, &n1, 5);
        record_uptime(0, &n2, 5);
        record_uptime(0, &n3, 5);
        assert_eq!(NodeUptime::<TestRuntime>::iter_prefix(0).count(), 3);

        // A later period funded successfully with real uptime, to prove
        // liveness once the stuck period is abandoned.
        let node = register_node(registrar, 101, 1, 11);
        let pot = NodeManager::compute_reward_account_id();
        let _ = Balances::deposit_creating(&pot, reward);
        RewardPot::<TestRuntime>::insert(
            window + 1,
            RewardPotInfo::new(reward, 20u32, 0u64, false),
        );
        OutstandingRewardToPay::<TestRuntime>::put(reward);
        record_uptime(window + 1, &node, 5);

        // Push the current period past period 0 + the recovery window so the
        // unrecovered failed-funding period must be abandoned.
        RewardPeriod::<TestRuntime>::mutate(|p| p.current = window + 2);

        // First pass: bound the batch to two entries so the abandonment cleanup
        // is forced to span more than one drain call. The period must NOT be
        // completed while NodeUptime[0] still holds entries.
        set_batch_size(2);
        let used = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(2));
        assert!(used.any_gt(Weight::zero()), "drain should make progress");
        assert_eq!(
            NodeUptime::<TestRuntime>::iter_prefix(0).count(),
            1,
            "two of three uptime entries cleared in the first bounded pass",
        );
        assert!(
            RewardPot::<TestRuntime>::get(0).is_some(),
            "period must not be completed while NodeUptime[0] still has entries",
        );
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            0,
            "cursor must not advance before the abandoned period is fully drained",
        );

        // Second pass: drains the remaining entry, completes period 0, and pays
        // the later funded period's operator.
        set_batch_size(64);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(50));

        // (a) NodeUptime[0] fully cleared - no orphaned entries.
        assert_eq!(
            NodeUptime::<TestRuntime>::iter_prefix(0).count(),
            0,
            "all uptime entries for the abandoned period must be cleared",
        );
        assert!(
            RewardPot::<TestRuntime>::get(0).is_none(),
            "abandoned failed-funding snapshot must be removed",
        );
        // (b) cursor advanced past the abandoned period.
        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() > 0,
            "cursor must advance past the abandoned period",
        );
        // (c) liveness: the later funded period's operator gets paid.
        let owner = TestAccount::new([101u8; 32]).account_id();
        assert!(
            Balances::free_balance(owner) > 0,
            "later period operator must be paid once the stuck period is abandoned",
        );
    });
}

#[test]
fn drain_charges_weight_and_bounds_empty_abandoned_period_completions() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let window = <TestRuntime as Config>::MaxFailedFundingRecoveryPeriods::get();

        // Five consecutive failed-funding periods (0..=4) with NO NodeUptime
        // entries, all pushed past the recovery window so they must be abandoned.
        OldestUnpaidRewardPeriodIndex::<TestRuntime>::put(0);
        for p in 0..5u64 {
            RewardPot::<TestRuntime>::insert(p, RewardPotInfo::new(0u128, 20u32, 0u64, true));
        }
        RewardPeriod::<TestRuntime>::mutate(|p| p.current = window + 10);

        // Budget for exactly three completions. Even though empty periods drain
        // no nodes, completing each still charges one `per_iter`, so the outer
        // loop's weight guard must stop after three.
        let used = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(3));

        assert_eq!(
            used,
            per_iter().saturating_mul(3),
            "each empty-period completion must charge one per_iter",
        );
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            3,
            "weight budget must bound completions to three periods per call",
        );
        for p in 0..3u64 {
            assert!(RewardPot::<TestRuntime>::get(p).is_none(), "period {p} must be completed");
        }
        for p in 3..5u64 {
            assert!(
                RewardPot::<TestRuntime>::get(p).is_some(),
                "period {p} must remain for a later call",
            );
        }

        // A fresh call with ample budget finishes the remaining periods.
        let used2 = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(50));
        assert!(used2.any_gt(Weight::zero()), "drain should make progress");
        for p in 3..5u64 {
            assert!(
                RewardPot::<TestRuntime>::get(p).is_none(),
                "period {p} must be abandoned on the second call",
            );
        }
        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() >= 5,
            "cursor must advance past all abandoned periods",
        );
    });
}
