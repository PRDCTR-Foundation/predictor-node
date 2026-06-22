// Copyright 2026 Aventus DAO.

use crate::{mock::*, *};
use frame_support::{
    assert_noop, assert_ok,
    traits::{Currency, ExistenceRequirement},
    weights::Weight,
};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

/// Generous idle weight so `on_idle` (run by `roll_one_block`) attempts to
/// drain, reproducing the production sequencing where `on_idle` runs in the
/// same block as the rollover `on_initialize`.
fn generous_idle_weight() -> Weight {
    NodeManager::worst_case_iteration_weight().saturating_mul(20)
}

/// Drain the treasury source so the next rollover transfer fails.
fn drain_treasury() {
    let sink = TestAccount::new([99u8; 32]).account_id();
    let bal = Balances::free_balance(treasury_account());
    let _ = <Balances as Currency<AccountId>>::transfer(
        &treasury_account(),
        &sink,
        bal,
        ExistenceRequirement::AllowDeath,
    );
}

/// Roll forward to just past the next reward-period boundary so on_initialize
/// fires the rollover branch.
fn roll_past_next_period() {
    let reward_period = RewardPeriod::<TestRuntime>::get();
    let now = System::block_number();
    let start = reward_period.first;
    let length = reward_period.length as u64;
    let target_block = start.saturating_add(length).saturating_add(1);
    if target_block > now {
        roll_forward(target_block - now);
    }
}

#[test]
fn rollover_funds_pot_when_treasury_has_funds() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        let before_outstanding = OutstandingRewardToPay::<TestRuntime>::get();
        let pot = NodeManager::compute_reward_account_id();
        let before_pot_balance = Balances::free_balance(pot);

        roll_past_next_period();

        // Treasury → pot transfer succeeded; RewardPotInfo carries the full amount.
        let info = RewardPot::<TestRuntime>::get(0).expect("RewardPotInfo for period 0");
        assert_eq!(info.total_reward, reward_amount);

        // Outstanding bumped, pot balance increased.
        assert_eq!(
            OutstandingRewardToPay::<TestRuntime>::get(),
            before_outstanding.saturating_add(reward_amount)
        );
        assert_eq!(Balances::free_balance(pot), before_pot_balance.saturating_add(reward_amount));

        // RewardPotFunded event emitted alongside NewRewardPeriodStarted.
        let events = System::events();
        assert!(
            events.iter().any(|er| matches!(
                &er.event,
                RuntimeEvent::NodeManager(Event::RewardPotFunded { period: 0, amount })
                    if *amount == reward_amount
            )),
            "RewardPotFunded missing"
        );
    });
}

#[test]
fn rollover_emits_funding_failed_when_treasury_empty() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        let before_outstanding = OutstandingRewardToPay::<TestRuntime>::get();
        let pot = NodeManager::compute_reward_account_id();
        let before_pot_balance = Balances::free_balance(pot);

        drain_treasury();
        roll_past_next_period();

        // Period state is recorded but with total_reward = 0 so on_idle
        // can skip it and top_up_reward_pot can later replace the value.
        let info = RewardPot::<TestRuntime>::get(0).expect("RewardPotInfo for period 0");
        assert!(info.total_reward.is_zero(), "total_reward should be zero on failure");

        // Outstanding NOT bumped, pot balance unchanged.
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), before_outstanding);
        assert_eq!(Balances::free_balance(pot), before_pot_balance);

        let events = System::events();
        assert!(
            events.iter().any(|er| matches!(
                &er.event,
                RuntimeEvent::NodeManager(Event::RewardPotFundingFailed {
                    period: 0,
                    requested_amount,
                    ..
                }) if *requested_amount == reward_amount
            )),
            "RewardPotFundingFailed missing"
        );
    });
}

#[test]
fn top_up_recovers_after_funding_failure() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        let before_outstanding = OutstandingRewardToPay::<TestRuntime>::get();
        let pot = NodeManager::compute_reward_account_id();

        // Run `on_idle` at the end of every block, exactly as production does -
        // including the rollover block, so the drain sees the freshly recorded
        // failed-funding snapshot and must leave it recoverable rather than
        // completing+removing it.
        set_idle_drain_weight(generous_idle_weight());

        drain_treasury();
        roll_past_next_period();

        // The drain (via on_idle) has run on the failed-funding period but must
        // NOT have removed it or advanced past it - it is still recoverable.
        let info = RewardPot::<TestRuntime>::get(0).expect("RewardPotInfo for period 0");
        assert!(info.funding_failed, "failed-funding snapshot should be flagged");
        assert!(info.total_reward.is_zero());
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            0,
            "cursor must not advance past an unrecovered failed-funding period",
        );

        // Refund treasury (simulating an off-chain top-up) and call the
        // extrinsic to recover the period - it still exists despite on_idle.
        let _ = Balances::deposit_creating(&treasury_account(), reward_amount * 10);

        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0, reward_amount,));

        let info = RewardPot::<TestRuntime>::get(0).expect("RewardPotInfo for period 0");
        assert_eq!(info.total_reward, reward_amount);
        assert!(!info.funding_failed, "funding_failed cleared after recovery");
        assert_eq!(
            OutstandingRewardToPay::<TestRuntime>::get(),
            before_outstanding.saturating_add(reward_amount)
        );
        assert_eq!(Balances::free_balance(pot), reward_amount);

        System::assert_last_event(
            Event::RewardPotFunded { period: 0, amount: reward_amount }.into(),
        );
    });
}

#[test]
fn drain_blocks_on_failed_funding_then_resumes_after_top_up() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        set_idle_drain_weight(generous_idle_weight());

        drain_treasury();
        roll_past_next_period();

        // The failed-funding period (0) holds the cursor; the drain does not
        // advance past it even after several idle blocks.
        roll_forward(3);
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            0,
            "drain must keep waiting on the unrecovered period",
        );
        assert!(RewardPot::<TestRuntime>::get(0).is_some());

        // Recover the period, then let the drain run again: with no uptime the
        // now-funded period is reclaimed and completed, so the cursor advances -
        // proving the drain resumes rather than permanently stalling.
        let _ = Balances::deposit_creating(&treasury_account(), reward_amount * 10);
        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0, reward_amount,));

        roll_forward(1);
        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() > 0,
            "drain should advance past the recovered period",
        );
        assert!(RewardPot::<TestRuntime>::get(0).is_none(), "recovered period drained and removed");
    });
}

#[test]
fn top_up_rejects_already_funded_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        roll_past_next_period();

        // Period 0 is now funded; further top_up must fail.
        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0, AVT),
            Error::<TestRuntime>::RewardPotAlreadyFunded
        );
    });
}

#[test]
fn top_up_rejects_legitimately_zero_funded_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // A period funded successfully with a zero `reward_amount` has
        // `total_reward == 0` but `funding_failed == false`. It is NOT a failed
        // funding, so top_up must reject it rather than injecting funds for a
        // period that never had a funding failure.
        RewardPot::<TestRuntime>::insert(3, RewardPotInfo::new(0u128, 20u32, 0u64, false));

        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 3, AVT),
            Error::<TestRuntime>::RewardPotAlreadyFunded
        );
    });
}

#[test]
fn top_up_rejects_unknown_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // No rollover has happened yet; RewardPot for any index is empty.
        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 7, AVT),
            Error::<TestRuntime>::RewardPotNotFound
        );
    });
}

#[test]
fn top_up_rejects_zero_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        drain_treasury();
        roll_past_next_period();

        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0, 0),
            Error::<TestRuntime>::ZeroAmount
        );
    });
}

#[test]
fn top_up_rejects_when_treasury_underfunded() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();

        drain_treasury();
        roll_past_next_period();
        // Treasury is still empty when top_up runs.

        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0, reward_amount),
            Error::<TestRuntime>::TreasuryUnderfunded
        );

        // No state change on the failed path.
        let info = RewardPot::<TestRuntime>::get(0).expect("RewardPotInfo for period 0");
        assert!(info.total_reward.is_zero());
    });
}

#[test]
fn top_up_rejects_non_root_origin() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        drain_treasury();
        roll_past_next_period();

        let signed = TestAccount::new([7u8; 32]).account_id();
        assert_noop!(
            NodeManager::top_up_reward_pot(RuntimeOrigin::signed(signed), 0, AVT,),
            DispatchError::BadOrigin
        );
    });
}
