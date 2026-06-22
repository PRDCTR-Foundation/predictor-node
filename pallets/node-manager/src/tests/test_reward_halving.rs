// Copyright 2026 Aventus DAO.

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

const HALVING_INTERVAL: u64 = 1_000; // mirrors `HalvingInterval` in mock.rs

/// Roll the chain forward to *just past* the given block. Re-uses the mock's
/// `roll_forward` so each block also fires `on_initialize`.
fn roll_to(target: u64) {
    let now = System::block_number();
    if target > now {
        roll_forward(target - now);
    }
}

fn enable_halving() {
    assert_ok!(NodeManager::set_halving_enabled(RawOrigin::Root.into(), true));
}

#[test]
fn halving_fires_exactly_at_the_boundary() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Start with a known non-zero reward amount via admin.
        let initial = 1_000_000 * AVT;
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardAmountPerPeriod(initial),
        ));
        enable_halving();

        // One block before the first boundary: no halving yet.
        roll_to(HALVING_INTERVAL - 1);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 0);

        // Cross the boundary: amount halves once, counter increments.
        roll_to(HALVING_INTERVAL);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial / 2);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 1);
    });
}

#[test]
fn halving_is_idempotent_within_the_same_interval() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let initial = 1_024 * AVT;
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardAmountPerPeriod(initial),
        ));
        enable_halving();

        // Cross the first boundary.
        roll_to(HALVING_INTERVAL);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial / 2);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 1);

        // Advance further but still within the same interval window: no extra halving.
        roll_to(HALVING_INTERVAL + 100);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial / 2);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 1);
    });
}

#[test]
fn halving_catches_up_across_multiple_boundaries() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let initial = 1_024 * AVT;
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardAmountPerPeriod(initial),
        ));
        // Halving stays disabled while we cross 3 boundaries.
        roll_to(3 * HALVING_INTERVAL + 7);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 0);

        // Now enable: next on_initialize catches up 3 halvings in one shot.
        enable_halving();
        roll_forward(1);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial / 8);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 3);
    });
}

#[test]
fn halving_does_not_fire_when_disabled() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let initial = 1_024 * AVT;
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardAmountPerPeriod(initial),
        ));
        // HalvingEnabledAtGenesis is false in the mock; explicit set is a no-op
        // but documents intent.
        assert_ok!(NodeManager::set_halving_enabled(RawOrigin::Root.into(), false));

        roll_to(2 * HALVING_INTERVAL + 5);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), initial);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 0);
    });
}

#[test]
fn halving_floors_at_one_base_unit() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Tiny initial amount so a handful of halvings reaches the floor.
        let initial: u128 = 4;
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardAmountPerPeriod(initial),
        ));
        enable_halving();

        // Four pending halvings applied in one catch-up tick: 4 → 2 → 1,
        // then the floor holds - the reward approaches zero asymptotically
        // but never reaches it (per the Truth-paper halving directive).
        roll_to(4 * HALVING_INTERVAL);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), 1);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 4);

        // Stepwise boundaries hold the floor too.
        roll_to(5 * HALVING_INTERVAL);
        assert_eq!(NextRewardAmountPerPeriod::<TestRuntime>::get(), 1);
        assert_eq!(RewardAmountHalvingsApplied::<TestRuntime>::get(), 5);
    });
}

#[test]
fn set_halving_enabled_rejects_non_root() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let caller = TestAccount::new([7u8; 32]).account_id();
        assert_noop!(
            NodeManager::set_halving_enabled(RuntimeOrigin::signed(caller), true,),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn set_halving_enabled_emits_event() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        enable_halving();
        System::assert_last_event(Event::HalvingEnabledSet { enabled: true }.into());

        assert_ok!(NodeManager::set_halving_enabled(RawOrigin::Root.into(), false));
        System::assert_last_event(Event::HalvingEnabledSet { enabled: false }.into());
    });
}
