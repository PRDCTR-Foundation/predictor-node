// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-08-27.

//! `SeedGenesisOnUpgrade` - the migration that makes a forkless introduction of
//! the pallet behave like a genesis start.
//!
//! `ExtBuilder::build_default()` (without `.with_genesis_config()`) leaves the
//! pallet's storage at its type defaults - which is exactly the state a pallet
//! introduced by `System::set_code` is in, because `genesis_build` never runs on
//! that path. So these tests reproduce the forkless condition faithfully.

#![cfg(test)]

use crate::{migrations::*, tests::mock::*, *};
use frame_support::{
    assert_noop, assert_ok,
    traits::{GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
};
use frame_system::RawOrigin;

type Migration = SeedGenesisOnUpgrade<TestRuntime>;

fn pot_providers() -> u32 {
    frame_system::Pallet::<TestRuntime>::providers(&NodeManager::compute_reward_account_id())
}

#[test]
fn forkless_defaults_make_the_pallet_usable_without_seeding() {
    // Without genesis_build the storage defaults alone give a working config, and
    // `RewardPeriod` agrees with them.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), DEFAULT_BATCH_SIZE);
        assert_eq!(NextRewardPeriodLength::<TestRuntime>::get(), DEFAULT_REWARD_PERIOD);
        assert_eq!(NextHeartbeatPeriod::<TestRuntime>::get(), DEFAULT_HEARTBEAT_PERIOD);
        assert_eq!(MinUptimeThreshold::<TestRuntime>::get(), DEFAULT_MIN_UPTIME_THRESHOLD);
        assert!(OutstandingRewardToPay::<TestRuntime>::get().is_zero());
        let period = RewardPeriod::<TestRuntime>::get();
        assert_eq!(period.length, DEFAULT_REWARD_PERIOD);
        assert_eq!(period.heartbeat_period, DEFAULT_HEARTBEAT_PERIOD);
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextRewardPeriodLength(10)
        ));
        assert_ok!(NodeManager::set_admin_config(
            RawOrigin::Root.into(),
            AdminConfig::NextHeartbeatPeriod(5)
        ));
    });
}

#[test]
fn migration_seeds_what_defaults_cannot_express() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        // The mock ext leaves the version at 0; a real set_code introduction
        // pre-initialises it to the in-code version instead. The gate is the data
        // (no provider on the pot), see `migration_gate_is_the_data_not_the_storage_version`.
        assert_eq!(Pallet::<TestRuntime>::on_chain_storage_version(), StorageVersion::new(0));
        assert_eq!(pot_providers(), 0);
        assert!(LockSchedule::<TestRuntime>::get().is_none());

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(pot_providers(), 1);
        assert!(LockSchedule::<TestRuntime>::get().is_some());
        assert_eq!(Pallet::<TestRuntime>::on_chain_storage_version(), StorageVersion::new(1));
    });
}

#[test]
fn migration_gate_is_the_data_not_the_storage_version() {
    // Regression guard for a real bug: gating on `on_chain_storage_version() < 1` passed every
    // mock test but SKIPPED on a real forkless upgrade, because a pallet added by `set_code` has
    // its on-chain version pre-initialised to the in-code one. This forces that condition:
    // version already at the seeded value, data still unseeded. The migration MUST still fire.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<TestRuntime>>();
        assert_eq!(pot_providers(), 0, "still unseeded");

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(pot_providers(), 1, "migration skipped because the version was already 1");
        assert!(LockSchedule::<TestRuntime>::get().is_some());
    });
}

#[test]
fn migration_retires_once_the_version_moves_past_the_seeded_layout() {
    // After a future migration bumps the pallet past SEEDED_STORAGE_VERSION, this seeder must
    // never fire again, even if the data looks unseeded.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        StorageVersion::new(SEEDED_STORAGE_VERSION + 1).put::<Pallet<TestRuntime>>();

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(pot_providers(), 0, "retired seeder fired on a newer storage layout");
        assert!(LockSchedule::<TestRuntime>::get().is_none());
        assert_eq!(
            Pallet::<TestRuntime>::on_chain_storage_version(),
            StorageVersion::new(SEEDED_STORAGE_VERSION + 1),
            "retired seeder rewrote the storage version",
        );
    });
}

#[test]
fn migration_converges_a_version_zero_genesis_chain_without_touching_data() {
    // A chain whose genesis ran on a runtime that still declared version 0 has seeded data but
    // sits below SEEDED_STORAGE_VERSION. The seeder must bump the version and leave data alone.
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        StorageVersion::new(0).put::<Pallet<TestRuntime>>();
        MaxBatchSize::<TestRuntime>::put(500); // operator-tuned, must survive

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(
            Pallet::<TestRuntime>::on_chain_storage_version(),
            StorageVersion::new(SEEDED_STORAGE_VERSION),
            "version not converged",
        );
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), 500, "seeder clobbered live data");
        assert!(LockSchedule::<TestRuntime>::get().is_none(), "genesis left it unset on purpose");
    });
}

#[test]
fn migration_is_idempotent() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let _ = Migration::on_runtime_upgrade();
        let configured = crate::types::LockScheduleInfo::new(1_234_567, 31);
        LockSchedule::<TestRuntime>::put(configured);

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(pot_providers(), 1, "provider added twice");
        assert_eq!(LockSchedule::<TestRuntime>::get(), Some(configured));
    });
}

#[test]
fn migration_seeds_a_usable_lock_window() {
    // Without this, a forkless introduction leaves `LockSchedule` at `None`,
    // which the payout path reads as "locked" while `withdraw_rewards` rejects
    // with `LockScheduleNotSet` - rewards accrue that nobody can claim.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        assert!(LockSchedule::<TestRuntime>::get().is_none(), "unseeded on the upgrade path");

        let _ = Migration::on_runtime_upgrade();

        let schedule = LockSchedule::<TestRuntime>::get().expect("window seeded");
        assert_eq!(schedule.initial_penalty_percent, 52);
        // Anchored at the upgrade block, so the window is live rather than
        // already expired.
        assert_eq!(schedule.start, NodeManager::time_now_sec());
        assert!(!schedule.is_expired(NodeManager::time_now_sec()));
        assert_eq!(schedule.penalty_at(NodeManager::time_now_sec()), Perbill::from_percent(52));
    });
}

#[test]
fn withdrawals_work_after_a_forkless_introduction() {
    // End-to-end proof that the seeded window unbricks the claim path: before
    // the migration `withdraw_rewards` errors, after it the owner gets their
    // net at the week-one rate.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let owner = TestAccount::new([117u8; 32]).account_id();
        LockedRewards::<TestRuntime>::insert(owner, 100 * PRD);
        TotalLockedRewards::<TestRuntime>::put(100 * PRD);
        let _ = Balances::deposit_creating(&NodeManager::compute_reward_account_id(), 100 * PRD);

        assert_noop!(
            NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None),
            Error::<TestRuntime>::LockScheduleNotSet,
        );

        let _ = Migration::on_runtime_upgrade();

        assert_ok!(NodeManager::withdraw_rewards(RawOrigin::Signed(owner).into(), None));
        // Week one of the seeded window: 52% forfeited, 48% to the owner.
        assert_eq!(Balances::free_balance(owner), 48 * PRD);
    });
}
