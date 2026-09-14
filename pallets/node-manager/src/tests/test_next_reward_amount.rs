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

#[test]
fn root_can_set_it() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let current_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        let new_amount = current_amount + 1;

        assert_ok!(NodeManager::set_next_reward_amount(RawOrigin::Root.into(), new_amount));

        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), new_amount);
        System::assert_last_event(Event::NextRewardAmountPerPeriodSet { new_amount }.into());
    });
}

#[test]
fn registrar_can_set_it() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = set_registrar();
        let current_amount = NextRewardAmountPerPeriod::<TestRuntime>::get();
        let new_amount = current_amount + 1;

        assert_ok!(NodeManager::set_next_reward_amount(
            RuntimeOrigin::signed(registrar),
            new_amount,
        ));

        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), new_amount);
        System::assert_last_event(Event::NextRewardAmountPerPeriodSet { new_amount }.into());
    });
}

#[test]
fn it_only_updates_the_next_period_not_the_current_one() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let reward_period = RewardPeriod::<TestRuntime>::get();
        let new_amount = reward_period.reward_amount + 1;

        assert_ok!(NodeManager::set_next_reward_amount(RawOrigin::Root.into(), new_amount));

        assert_eq!(RewardPeriod::<TestRuntime>::get().reward_amount, reward_period.reward_amount);

        roll_forward((reward_period.length as u64 - System::block_number()) + 1);

        assert_eq!(RewardPeriod::<TestRuntime>::get().reward_amount, new_amount);
    });
}

mod fails_to_be_set_when {
    use super::*;

    #[test]
    fn amount_is_zero() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let new_amount: BalanceOf<TestRuntime> = 0u128;

            assert_noop!(
                NodeManager::set_next_reward_amount(RawOrigin::Root.into(), new_amount),
                Error::<TestRuntime>::NextRewardAmountPerPeriodZero
            );
        });
    }

    #[test]
    fn origin_is_an_unauthorised_signed_account() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            set_registrar();
            let caller = TestAccount::new([7u8; 32]).account_id();

            assert_noop!(
                NodeManager::set_next_reward_amount(RuntimeOrigin::signed(caller), 1_000),
                Error::<TestRuntime>::OriginNotRegistrar
            );
        });
    }

    #[test]
    fn origin_is_signed_and_no_registrar_is_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let caller = TestAccount::new([7u8; 32]).account_id();

            assert_noop!(
                NodeManager::set_next_reward_amount(RuntimeOrigin::signed(caller), 1_000),
                Error::<TestRuntime>::RegistrarNotSet
            );
        });
    }

    #[test]
    fn origin_is_none() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            assert_noop!(
                NodeManager::set_next_reward_amount(RawOrigin::None.into(), 1_000),
                DispatchError::BadOrigin
            );
        });
    }
}
