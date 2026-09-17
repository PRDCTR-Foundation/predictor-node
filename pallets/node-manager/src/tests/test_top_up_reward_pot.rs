// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-07-07.
// Rewritten for PRDCTR on 2026-09-18: `on_initialize` no longer attempts a
// treasury transfer at rollover - every period closes into an unfunded
// `RewardPot` (`funding_failed: true`) regardless of treasury balance.
// Rewritten again for PRDCTR on 2026-09-19: `top_up_reward_pot` is now a
// plain, period-agnostic balance top-up (root-only) - it just moves funds
// from the treasury into the pot's balance, see `Pallet::do_top_up_reward_pot`.
// Allocating a topped-up balance to a specific period's distribution is
// `set_reward_amount`'s job, covered in `test_next_reward_amount.rs`. This
// file focuses on the top-up mechanics themselves and the drain's
// recovery-window handling of a still-unfunded period.

#![cfg(test)]

use crate::{tests::mock::*, *};
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
/// fires the rollover branch, leaving the closed period awaiting funding.
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
fn top_up_moves_treasury_funds_into_the_pot_balance() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let pot = NodeManager::compute_reward_account_id();
        let treasury_before = Balances::free_balance(treasury_account());
        let pot_before = Balances::free_balance(pot);
        let amount = 20 * PRD;

        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), amount));

        assert_eq!(Balances::free_balance(pot), pot_before.saturating_add(amount));
        assert_eq!(
            Balances::free_balance(treasury_account()),
            treasury_before.saturating_sub(amount)
        );
        System::assert_last_event(Event::RewardPotToppedUp { amount }.into());
    });
}

#[test]
fn top_up_is_not_tied_to_any_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // No rollover has happened yet - there is no `RewardPot` entry for
        // any period - yet the top-up still succeeds since it only touches
        // the pot's raw balance.
        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 20 * PRD));
        assert!(RewardPot::<TestRuntime>::get(0).is_none());
    });
}

#[test]
fn top_up_accumulates_across_multiple_calls() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let pot = NodeManager::compute_reward_account_id();
        let pot_before = Balances::free_balance(pot);

        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 5 * PRD));
        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 7 * PRD));

        assert_eq!(Balances::free_balance(pot), pot_before.saturating_add(12 * PRD));
    });
}

#[test]
fn top_up_rejects_zero_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 0),
            Error::<TestRuntime>::ZeroAmount
        );
    });
}

#[test]
fn top_up_rejects_when_treasury_underfunded() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let pot = NodeManager::compute_reward_account_id();
        let pot_before = Balances::free_balance(pot);
        drain_treasury();

        assert_noop!(
            NodeManager::top_up_reward_pot(RawOrigin::Root.into(), 20 * PRD),
            Error::<TestRuntime>::TreasuryUnderfunded
        );

        // No state change on the failed path.
        assert_eq!(Balances::free_balance(pot), pot_before);
    });
}

#[test]
fn top_up_rejects_non_root_origin() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let signed = TestAccount::new([7u8; 32]).account_id();
        assert_noop!(
            NodeManager::top_up_reward_pot(RuntimeOrigin::signed(signed), PRD),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn drain_blocks_on_an_unfunded_period_then_resumes_after_top_up_and_set_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        set_idle_drain_weight(generous_idle_weight());

        roll_past_next_period();

        // Period 0 holds the cursor, awaiting funding; the drain does not
        // advance past it even after several idle blocks.
        roll_forward(3);
        assert_eq!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(),
            0,
            "drain must keep waiting on the unfunded period",
        );
        assert!(RewardPot::<TestRuntime>::get(0).is_some());

        // Top up the pot and record the period's amount, then let the drain
        // run again: with no uptime the now-funded period is reclaimed and
        // completed, so the cursor advances - proving the drain resumes
        // rather than permanently stalling.
        let reward_amount = 20 * PRD;
        assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), reward_amount));
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), 0, reward_amount));

        roll_forward(1);
        assert!(
            OldestUnpaidRewardPeriodIndex::<TestRuntime>::get() > 0,
            "drain should advance past the funded period",
        );
        assert!(RewardPot::<TestRuntime>::get(0).is_none(), "funded period drained and removed");
    });
}
