// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-09-14.

#![cfg(test)]

use crate::{tests::mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

fn set_registrar() -> AccountId {
    let registrar = TestAccount::new([55u8; 32]).account_id();
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::NodeRegistrar(registrar),
    ));
    registrar
}

/// Roll past the genesis period's boundary (`length = 200`) so it closes and
/// awaits funding, and return its index.
fn roll_past_period_0() -> RewardPeriodIndex {
    let period = RewardPeriod::<TestRuntime>::get().current;
    roll_forward(200);
    period
}

/// Move the clock to the point where `period`'s update window has closed.
fn close_update_window(_period: RewardPeriodIndex) {
    advance_time_secs(REWARD_UPDATE_WINDOW_SECS);
}

fn top_up(amount: u128) {
    assert_ok!(NodeManager::top_up_reward_pot(RawOrigin::Root.into(), amount));
}

#[test]
fn a_period_rolls_over_on_schedule_without_being_funded() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();

        assert_eq!(RewardPeriod::<TestRuntime>::get().current, period + 1);
        let pot = RewardPot::<TestRuntime>::get(period).expect("period must exist, awaiting funds");
        assert!(!pot.funded);
        assert!(pot.total_reward.is_zero());
    });
}

#[test]
fn root_can_fund_an_ended_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        let amount = 20 * PRD;
        top_up(amount);

        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, amount));

        let pot = RewardPot::<TestRuntime>::get(period).expect("period must exist");
        assert_eq!(pot.total_reward, amount);
        assert!(pot.funded);
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), amount);
        System::assert_last_event(Event::RewardAmountSet { period, amount }.into());
    });
}

#[test]
fn registrar_can_fund_an_ended_period() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = set_registrar();
        let period = roll_past_period_0();
        let amount = 20 * PRD;
        top_up(amount);

        assert_ok!(NodeManager::set_reward_amount(
            RuntimeOrigin::signed(registrar),
            period,
            amount,
        ));

        let pot = RewardPot::<TestRuntime>::get(period).expect("period must exist");
        assert_eq!(pot.total_reward, amount);
        System::assert_last_event(Event::RewardAmountSet { period, amount }.into());
    });
}

#[test]
fn rollover_never_waits_on_funding_to_advance_further_periods() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period_0 = roll_past_period_0();
        // Period 0 is still unfunded, yet period 1 closes on schedule too
        // (it inherited period 0's length - 200 - when it opened).
        roll_forward(200);

        assert_eq!(RewardPeriod::<TestRuntime>::get().current, period_0 + 2);
        assert!(!RewardPot::<TestRuntime>::get(period_0).unwrap().funded);
        assert!(!RewardPot::<TestRuntime>::get(period_0 + 1).unwrap().funded);
    });
}

#[test]
fn ended_period_records_its_end_time() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();

        let pot = RewardPot::<TestRuntime>::get(period).unwrap();
        assert_eq!(pot.reward_end_time, NodeManager::time_now_sec());
        assert!(pot.update_window_open(NodeManager::time_now_sec()));
    });
}

#[test]
fn update_window_closes_five_minutes_after_the_period_ends() {
    let pot = RewardPotInfo::new(0u128, 20u32, 1_000, true);

    assert!(pot.update_window_open(1_000));
    assert!(pot.update_window_open(1_000 + REWARD_UPDATE_WINDOW_SECS - 1));
    assert!(!pot.update_window_open(1_000 + REWARD_UPDATE_WINDOW_SECS));
    assert_eq!(REWARD_UPDATE_WINDOW_SECS, 300);
}

#[test]
fn zero_amount_can_be_set() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();

        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 0));

        let pot = RewardPot::<TestRuntime>::get(period).unwrap();
        assert!(pot.funded);
        assert!(pot.total_reward.is_zero());
        assert!(OutstandingRewardToPay::<TestRuntime>::get().is_zero());
        System::assert_last_event(Event::RewardAmountSet { period, amount: 0 }.into());
    });
}

#[test]
fn zero_reward_period_completes_without_paying_once_the_window_closes() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 0));
        close_update_window(period);

        let _ = NodeManager::drain_outstanding_payouts(
            NodeManager::worst_case_iteration_weight().saturating_mul(10),
        );

        assert!(RewardPot::<TestRuntime>::get(period).is_none());
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period + 1);
        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: period }.into(),
        );
    });
}

#[test]
fn funded_amount_can_be_updated_within_the_window() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        top_up(30 * PRD);
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));

        // Raise, then lower: outstanding always tracks the latest amount.
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 30 * PRD));
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), 30 * PRD);
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 5 * PRD));
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), 5 * PRD);
        assert_eq!(RewardPot::<TestRuntime>::get(period).unwrap().total_reward, 5 * PRD);
    });
}

#[test]
fn raising_an_amount_beyond_the_pot_is_rejected_and_leaves_the_old_amount() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        top_up(20 * PRD);
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));

        assert_noop!(
            NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 21 * PRD),
            Error::<TestRuntime>::InsufficientPotBalance
        );
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), 20 * PRD);
    });
}

#[test]
fn no_rewards_are_paid_while_the_update_window_is_open() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        top_up(20 * PRD);
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));
        let budget = NodeManager::worst_case_iteration_weight().saturating_mul(10);

        let _ = NodeManager::drain_outstanding_payouts(budget);
        assert!(RewardPot::<TestRuntime>::get(period).is_some());
        assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), 20 * PRD);

        close_update_window(period);
        let _ = NodeManager::drain_outstanding_payouts(budget);
        assert!(RewardPot::<TestRuntime>::get(period).is_none());
    });
}

#[test]
fn unfunded_period_does_not_start_after_the_window_but_can_still_be_set() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let period = roll_past_period_0();
        close_update_window(period);
        let budget = NodeManager::worst_case_iteration_weight().saturating_mul(10);

        let _ = NodeManager::drain_outstanding_payouts(budget);
        assert!(RewardPot::<TestRuntime>::get(period).is_some());
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period);

        top_up(20 * PRD);
        assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));

        // Now funded and past the window: the drain proceeds.
        let _ = NodeManager::drain_outstanding_payouts(budget);
        assert!(RewardPot::<TestRuntime>::get(period).is_none());
        assert_eq!(OldestUnpaidRewardPeriodIndex::<TestRuntime>::get(), period + 1);
    });
}

mod fails_to_be_set_when {
    use super::*;

    #[test]
    fn the_period_has_not_ended_yet() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let live_period = RewardPeriod::<TestRuntime>::get().current;

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), live_period, 1_000),
                Error::<TestRuntime>::RewardPotNotFound
            );
        });
    }

    #[test]
    fn the_period_was_already_finished_and_cleaned_up() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            top_up(20 * PRD);
            assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));
            close_update_window(period);
            // No uptime recorded, so the drain reclaims and completes it -
            // removing its `RewardPot` entry.
            let _ = NodeManager::drain_outstanding_payouts(
                NodeManager::worst_case_iteration_weight().saturating_mul(10),
            );
            assert!(RewardPot::<TestRuntime>::get(period).is_none());

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 1_000),
                Error::<TestRuntime>::RewardPotNotFound
            );
        });
    }

    #[test]
    fn the_period_is_funded_and_its_update_window_has_closed() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            top_up(20 * PRD);
            assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));
            close_update_window(period);

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 1_000),
                Error::<TestRuntime>::RewardUpdateWindowClosed
            );
        });
    }

    #[test]
    fn amount_exceeds_the_per_period_cap() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();

            assert_noop!(
                NodeManager::set_reward_amount(
                    RawOrigin::Root.into(),
                    period,
                    <TestRuntime as Config>::MaxRewardPerPeriod::get().saturating_add(1)
                ),
                Error::<TestRuntime>::RewardExceedsMax
            );
        });
    }

    #[test]
    fn the_pot_does_not_have_enough_balance() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            // No top up at all - the pot's spendable balance is zero.

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD),
                Error::<TestRuntime>::InsufficientPotBalance
            );

            // Left retriable: still awaiting funding, not consumed by the
            // failed attempt.
            let pot = RewardPot::<TestRuntime>::get(period).expect("period must still exist");
            assert!(!pot.funded);
            assert!(pot.total_reward.is_zero());
        });
    }

    #[test]
    fn the_pot_only_partially_covers_the_amount() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            top_up(5 * PRD);

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD),
                Error::<TestRuntime>::InsufficientPotBalance
            );

            // Topping up the shortfall unblocks it.
            top_up(15 * PRD);
            assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));
        });
    }

    #[test]
    fn origin_is_an_unauthorised_signed_account() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            set_registrar();
            let period = roll_past_period_0();
            let caller = TestAccount::new([7u8; 32]).account_id();

            assert_noop!(
                NodeManager::set_reward_amount(RuntimeOrigin::signed(caller), period, 1_000),
                Error::<TestRuntime>::OriginNotRegistrar
            );
        });
    }

    #[test]
    fn origin_is_signed_and_no_registrar_is_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            let caller = TestAccount::new([7u8; 32]).account_id();

            assert_noop!(
                NodeManager::set_reward_amount(RuntimeOrigin::signed(caller), period, 1_000),
                Error::<TestRuntime>::RegistrarNotSet
            );
        });
    }

    #[test]
    fn origin_is_none() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::None.into(), period, 1_000),
                DispatchError::BadOrigin
            );
        });
    }
}
