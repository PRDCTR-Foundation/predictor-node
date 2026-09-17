// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-09-14.
// Rewritten for PRDCTR on 2026-09-18: a reward period rolls over on schedule
// regardless of funding; `set_reward_amount` may only fund a period that has
// already ended (rolled over) and whose distribution hasn't started yet.
// Rewritten again for PRDCTR on 2026-09-19: `set_reward_amount` no longer
// moves currency - it only records the period's distribution total, and
// rejects outright if the pot's balance (topped up separately via
// `top_up_reward_pot`, see `test_top_up_reward_pot.rs`) can't cover it. See
// `Pallet::do_set_reward_amount`.

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
        assert!(pot.funding_failed);
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
        assert!(!pot.funding_failed);
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
        assert!(RewardPot::<TestRuntime>::get(period_0).unwrap().funding_failed);
        assert!(RewardPot::<TestRuntime>::get(period_0 + 1).unwrap().funding_failed);
    });
}

mod fails_to_be_set_when {
    use super::*;

    #[test]
    fn amount_is_zero() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 0u128),
                Error::<TestRuntime>::ZeroAmount
            );
        });
    }

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
    fn distribution_has_already_started() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let period = roll_past_period_0();
            top_up(20 * PRD);
            assert_ok!(NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 20 * PRD));

            assert_noop!(
                NodeManager::set_reward_amount(RawOrigin::Root.into(), period, 1_000),
                Error::<TestRuntime>::RewardPotAlreadyFunded
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
            assert!(pot.funding_failed);
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
