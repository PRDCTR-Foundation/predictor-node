// Copyright 2026 Aventus DAO.

//! Global reward-payout lock (single global window mirroring the T1
//! migration claim schedule): accrual into `LockedRewards` while the window
//! is active or unset, `withdraw_rewards(Option<limit>)` with the decaying
//! forfeiture, forfeiture routing, and self-expiry back to direct payout.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok, weights::Weight};
use frame_system::RawOrigin;

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

/// Inject a NodeUptime entry directly (no live OCW in the mock; the drain
/// only reads storage).
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
        AdminConfig::NextRewardAmountPerPeriod(1_000 * AVT),
    ));
    assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), AdminConfig::BatchSize(64),));
}

/// Cross the genesis period boundary, record uptime for the given nodes in
/// the new period, then cross that period's boundary so it's snapshot-funded
/// and drainable. See `test_on_idle_drain.rs` for the period mechanics.
fn setup_unpaid_period_with_nodes(nodes_with_uptime: &[(AccountId, u64)]) -> RewardPeriodIndex {
    roll_forward(200);
    let period_to_pay = RewardPeriod::<TestRuntime>::get().current;
    for (node, count) in nodes_with_uptime {
        record_uptime(period_to_pay, node, *count);
    }
    roll_forward(20);
    period_to_pay
}

/// Seed locked rewards directly: the accrual path is covered by the drain
/// tests below; withdraw-focused tests start from a known locked balance.
fn seed_locked(owner: &AccountId, amount: u128) {
    LockedRewards::<TestRuntime>::insert(owner, amount);
    TotalLockedRewards::<TestRuntime>::put(amount);
    let pot = NodeManager::compute_reward_account_id();
    Balances::make_free_balance_be(&pot, amount + AVT);
}

// ---- accrual via the on_idle drain ------------------------------------

#[test]
fn payout_locks_when_schedule_unset() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        let node = register_node(registrar, 101, 1, 11);
        let owner = TestAccount::new([101u8; 32]).account_id();

        assert!(LockSchedule::<TestRuntime>::get().is_none(), "no schedule at genesis");
        let period = setup_unpaid_period_with_nodes(&[(node, 1)]);
        let pot_before = NodeManager::reward_pot_balance();
        // Period 0 was funded at rollover but has zero uptime, so the drain
        // reclaims it to the treasury; only that amount leaves the pot.
        let reclaimed = RewardPot::<TestRuntime>::get(0).map(|p| p.total_reward).unwrap_or_default();

        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));

        // Lock-by-default: nothing reaches free balance, the claim accrues.
        let locked = LockedRewards::<TestRuntime>::get(&owner);
        assert!(locked > 0, "reward should have accrued into LockedRewards");
        assert_eq!(Balances::free_balance(&owner), 0, "free balance must stay untouched");
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), locked);
        // The locked claim's funds never left the pot (only the reclaimed
        // empty-period reward did).
        assert_eq!(NodeManager::reward_pot_balance(), pot_before.saturating_sub(reclaimed));
        System::assert_has_event(
            Event::RewardLocked { reward_period: period, owner, node, amount: locked }.into(),
        );
        // The drain still completed the period.
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period.saturating_add(1),);

        // And the claim cannot be withdrawn until root configures the window.
        assert_noop!(
            NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None),
            Error::<TestRuntime>::LockScheduleNotSet
        );
    });
}

#[test]
fn payout_locks_during_active_window() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        set_lock_schedule(0, 52); // week-one rate: active
        let node = register_node(registrar, 102, 2, 12);
        let owner = TestAccount::new([102u8; 32]).account_id();

        let period = setup_unpaid_period_with_nodes(&[(node, 1)]);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));

        let locked = LockedRewards::<TestRuntime>::get(&owner);
        assert!(locked > 0);
        assert_eq!(Balances::free_balance(&owner), 0);
        System::assert_has_event(
            Event::RewardLocked { reward_period: period, owner, node, amount: locked }.into(),
        );
    });
}

#[test]
fn payout_direct_after_window_expiry() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        set_lock_schedule(0, 52);
        advance_time_weeks(52); // penalty decayed to zero: the lock is spent
        let node = register_node(registrar, 103, 3, 13);
        let owner = TestAccount::new([103u8; 32]).account_id();

        let period = setup_unpaid_period_with_nodes(&[(node, 1)]);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));

        let paid = Balances::free_balance(&owner);
        assert!(paid > 0, "expired window must pay free balance directly");
        assert_eq!(LockedRewards::<TestRuntime>::get(&owner), 0);
        System::assert_has_event(
            Event::RewardPaid { reward_period: period, owner, node, amount: paid }.into(),
        );
    });
}

#[test]
fn locked_rewards_accumulate_across_periods() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        set_lock_schedule(0, 52);
        let node = register_node(registrar, 104, 4, 14);
        let owner = TestAccount::new([104u8; 32]).account_id();

        // First period accrues...
        setup_unpaid_period_with_nodes(&[(node, 1)]);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));
        let after_first = LockedRewards::<TestRuntime>::get(&owner);
        assert!(after_first > 0);

        // ...and a second period's reward stacks on the same claim.
        let period = RewardPeriod::<TestRuntime>::get().current;
        record_uptime(period, &node, 1);
        roll_forward(20);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));

        let after_second = LockedRewards::<TestRuntime>::get(&owner);
        assert!(after_second > after_first, "second period must accumulate");
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), after_second);
        assert_eq!(Balances::free_balance(&owner), 0);
    });
}

/// Early-chain single-period case: ED > 0 and the pot's only balance is one
/// non-distributable period's funding. `AllowDeath` lets the reclaim drain the
/// pot to zero (the genesis provider ref keeps the account alive) instead of
/// failing the `KeepAlive` `>= ED` check and stranding the funds.
#[test]
fn reclaim_returns_funds_when_pot_holds_only_the_reclaimable_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        set_existential_deposit(AVT);

        let pot = NodeManager::compute_reward_account_id();
        let amount = 20 * AVT;
        Balances::make_free_balance_be(&pot, amount);
        assert_eq!(Balances::free_balance(&pot), amount, "pot holds only the reclaimable amount");

        let treasury_before = Balances::free_balance(&treasury_account());
        NodeManager::reclaim_undistributed_reward(7, amount);

        assert_eq!(
            Balances::free_balance(&treasury_account()),
            treasury_before + amount,
            "funds returned to treasury",
        );
        assert_eq!(Balances::free_balance(&pot), 0, "pot drained to zero");
        System::assert_has_event(
            Event::UndistributedRewardReclaimed { reward_period: 7, amount }.into(),
        );
        assert!(
            !System::events().iter().any(|r| matches!(
                r.event,
                RuntimeEvent::NodeManager(Event::UndistributedRewardReclaimFailed { .. })
            )),
            "reclaim must succeed, not fall into the failure path",
        );
    });
}

// ---- withdraw_rewards ---------------------------------------------------

#[test]
fn withdraw_full_at_week_one_forfeits_52_percent() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let owner = TestAccount::new([105u8; 32]).account_id();
        seed_locked(&owner, 100 * AVT);
        set_lock_schedule(0, 52);

        let treasury_before = Balances::free_balance(&treasury_account());
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));

        assert_eq!(Balances::free_balance(&owner), 48 * AVT);
        // No ForfeitureDestination configured: forfeit falls back to the
        // treasury source.
        assert_eq!(Balances::free_balance(&treasury_account()) - treasury_before, 52 * AVT,);
        assert_eq!(LockedRewards::<TestRuntime>::get(&owner), 0);
        assert!(!LockedRewards::<TestRuntime>::contains_key(&owner));
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), 0);
        System::assert_has_event(
            Event::RewardWithdrawn {
                owner,
                gross: 100 * AVT,
                net: 48 * AVT,
                forfeited: 52 * AVT,
                penalty: Perbill::from_percent(52),
            }
            .into(),
        );
    });
}

#[test]
fn withdraw_partial_with_limit_applies_penalty_to_slice() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let owner = TestAccount::new([106u8; 32]).account_id();
        seed_locked(&owner, 100 * AVT);
        // 26 full weeks elapsed: penalty 52 - 26 = 26%.
        set_lock_schedule(26, 52);

        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), Some(40 * AVT),));

        // forfeit = 26% of 40 = 10.4; net = 29.6.
        assert_eq!(Balances::free_balance(&owner), 29 * AVT + 6 * AVT / 10);
        assert_eq!(LockedRewards::<TestRuntime>::get(&owner), 60 * AVT);
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), 60 * AVT);

        // The remainder withdraws at the same rate.
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));
        assert_eq!(
            Balances::free_balance(&owner),
            (29 * AVT + 6 * AVT / 10) + (60 * AVT - 60 * AVT * 26 / 100),
        );
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), 0);
    });
}

#[test]
fn withdraw_near_and_after_window_end() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Week 52 (51 elapsed weeks): 1% penalty.
        let owner_a = TestAccount::new([107u8; 32]).account_id();
        seed_locked(&owner_a, 100 * AVT);
        set_lock_schedule(51, 52);
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner_a).into(), None));
        assert_eq!(Balances::free_balance(&owner_a), 99 * AVT);

        // Week 53+ (>= 52 elapsed weeks): zero penalty, full amount.
        let owner_b = TestAccount::new([108u8; 32]).account_id();
        seed_locked(&owner_b, 100 * AVT);
        advance_time_weeks(1);
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner_b).into(), None));
        assert_eq!(Balances::free_balance(&owner_b), 100 * AVT);
        System::assert_has_event(
            Event::RewardWithdrawn {
                owner: owner_b,
                gross: 100 * AVT,
                net: 100 * AVT,
                forfeited: 0,
                penalty: Perbill::zero(),
            }
            .into(),
        );
    });
}

#[test]
fn withdraw_routes_forfeit_to_configured_destination() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let owner = TestAccount::new([109u8; 32]).account_id();
        let destination = TestAccount::new([110u8; 32]).account_id();
        seed_locked(&owner, 100 * AVT);
        set_lock_schedule(0, 52);
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::ForfeitureDestination(destination),
        ));

        let treasury_before = Balances::free_balance(&treasury_account());
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));

        assert_eq!(Balances::free_balance(&destination), 52 * AVT);
        assert_eq!(Balances::free_balance(&treasury_account()), treasury_before);
    });
}

/// Single-node early-period exact-division case: ED > 0 and the pot's only
/// balance is exactly the locked rewards being withdrawn. `AllowDeath` lets the
/// net + forfeited transfers drain the pot to zero (the genesis provider ref
/// keeps the account alive) instead of failing the `KeepAlive` `>= ED` check
/// and blocking the owner from realising their reward.
#[test]
fn withdraw_full_succeeds_when_pot_holds_exactly_the_locked_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        set_existential_deposit(AVT);

        let owner = TestAccount::new([114u8; 32]).account_id();
        let destination = TestAccount::new([115u8; 32]).account_id();
        let gross = 100 * AVT;

        LockedRewards::<TestRuntime>::insert(&owner, gross);
        TotalLockedRewards::<TestRuntime>::put(gross);
        let pot = NodeManager::compute_reward_account_id();
        Balances::make_free_balance_be(&pot, gross);
        assert_eq!(Balances::free_balance(&pot), gross, "pot holds only the locked amount");

        set_lock_schedule(0, 52); // week-one: 52% forfeit, 48% net.
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::ForfeitureDestination(destination),
        ));

        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));

        assert_eq!(Balances::free_balance(&owner), 48 * AVT, "net reaches the owner");
        assert_eq!(Balances::free_balance(&destination), 52 * AVT, "forfeit reaches destination");
        assert_eq!(Balances::free_balance(&pot), 0, "pot drained to zero");
        assert_eq!(LockedRewards::<TestRuntime>::get(&owner), 0);
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), 0);
    });
}

/// The decaying forfeiture penalty applies to the FULL locked balance -
/// already-locked (existing) plus newly-accrued (new) rewards - not just one
/// portion. Lock a first reward, advance into the decay window, accrue a second
/// reward into the same locked balance, then withdraw and assert the penalty is
/// charged over the combined total.
#[test]
fn forfeiture_applies_to_combined_existing_and_new_locked() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        fast_periods();
        set_lock_schedule(0, 52);
        let node = register_node(registrar, 113, 13, 23);
        let owner = TestAccount::new([113u8; 32]).account_id();

        // Existing: first period accrues into the locked balance.
        setup_unpaid_period_with_nodes(&[(node, 1)]);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));
        let existing = LockedRewards::<TestRuntime>::get(&owner);
        assert!(existing > 0, "existing locked must be non-zero");

        // Advance 26 weeks (26% penalty), then accrue a NEW reward into the SAME
        // locked balance.
        advance_time_weeks(26);
        let period = RewardPeriod::<TestRuntime>::get().current;
        record_uptime(period, &node, 1);
        roll_forward(20);
        let _ = NodeManager::drain_outstanding_payouts(per_iter().saturating_mul(20));

        let combined = LockedRewards::<TestRuntime>::get(&owner);
        let new_portion = combined.saturating_sub(existing);
        assert!(new_portion > 0, "a new reward must have accrued on top of the existing locked");

        // Withdraw the full locked balance: penalty must be over existing + new.
        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));

        let penalty = Perbill::from_percent(26);
        let expected_forfeit = penalty.mul_floor(combined);
        let expected_net = combined.saturating_sub(expected_forfeit);

        assert_eq!(Balances::free_balance(&owner), expected_net);
        System::assert_has_event(
            Event::RewardWithdrawn {
                owner,
                gross: combined,
                net: expected_net,
                forfeited: expected_forfeit,
                penalty,
            }
            .into(),
        );
        // The forfeit must not match penalising only one portion.
        assert_ne!(
            expected_forfeit,
            penalty.mul_floor(existing),
            "penalty must not apply to the existing portion only",
        );
        assert_ne!(
            expected_forfeit,
            penalty.mul_floor(new_portion),
            "penalty must not apply to the new portion only",
        );
        assert_eq!(LockedRewards::<TestRuntime>::get(&owner), 0);
        assert_eq!(TotalLockedRewards::<TestRuntime>::get(), 0);
    });
}

#[test]
fn withdraw_error_paths() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let owner = TestAccount::new([111u8; 32]).account_id();

        // Nothing locked.
        assert_noop!(
            NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None),
            Error::<TestRuntime>::NoLockedRewards
        );

        seed_locked(&owner, 100 * AVT);
        set_lock_schedule(0, 52);

        // Limit above the locked balance.
        assert_noop!(
            NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), Some(101 * AVT)),
            Error::<TestRuntime>::WithdrawAmountExceedsLocked
        );
        // Zero limit.
        assert_noop!(
            NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), Some(0)),
            Error::<TestRuntime>::ZeroAmount
        );
    });
}

// ---- admin configuration ------------------------------------------------

#[test]
fn admin_sets_lock_schedule_and_forfeiture_destination() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let schedule = LockScheduleInfo::new(1_000_000, 52);
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::LockSchedule(schedule),
        ));
        assert_eq!(LockSchedule::<TestRuntime>::get(), Some(schedule));
        System::assert_last_event(
            Event::LockScheduleSet { start: 1_000_000, initial_penalty_percent: 52 }.into(),
        );

        let destination = TestAccount::new([112u8; 32]).account_id();
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::ForfeitureDestination(destination),
        ));
        assert_eq!(ForfeitureDestination::<TestRuntime>::get(), Some(destination));
        System::assert_last_event(Event::ForfeitureDestinationSet { destination }.into());
    });
}

#[test]
fn admin_rejects_penalty_above_100_percent() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        assert_noop!(
            NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::LockSchedule(LockScheduleInfo::new(0, 101)),
            ),
            Error::<TestRuntime>::InvalidLockSchedule
        );
    });
}

// ---- penalty curve unit checks -------------------------------------------

#[test]
fn penalty_curve_matches_the_proposal_schedule() {
    let schedule = LockScheduleInfo::new(0, 52);
    let week = crate::types::SECONDS_PER_WEEK;

    // Week 1 (0 elapsed weeks): 52%. Decays 1%/week to 0% from week 53 on.
    assert_eq!(schedule.penalty_at(0), Perbill::from_percent(52));
    assert_eq!(schedule.penalty_at(week - 1), Perbill::from_percent(52));
    assert_eq!(schedule.penalty_at(week), Perbill::from_percent(51));
    assert_eq!(schedule.penalty_at(26 * week), Perbill::from_percent(26));
    assert_eq!(schedule.penalty_at(51 * week), Perbill::from_percent(1));
    assert_eq!(schedule.penalty_at(52 * week), Perbill::zero());
    assert_eq!(schedule.penalty_at(1_000 * week), Perbill::zero());
    assert!(!schedule.is_expired(51 * week));
    assert!(schedule.is_expired(52 * week));

    // A start in the future charges the week-one rate.
    let future = LockScheduleInfo::new(10 * week, 52);
    assert_eq!(future.penalty_at(0), Perbill::from_percent(52));
}
