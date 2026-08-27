// Copyright 2026 Aventus DAO.

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

#[test]
fn forkless_defaults_are_the_broken_state_the_migration_fixes() {
    // Documents the bug the migration exists for: without genesis_build, the
    // pallet is unusable, and RewardPeriod resolves to a third default (20) that
    // matches neither the GenesisConfig default (2) nor zero.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), 0, "type default is 0");
        assert_eq!(NextRewardPeriodLength::<TestRuntime>::get(), 0);
        assert_eq!(
            RewardPeriod::<TestRuntime>::get().length,
            20,
            "ValueQuery resolves to RewardPeriodInfo::default() = 20, not the GenesisConfig 2",
        );
        // The brick: with a zero period length, no heartbeat period is settable
        // (it must be strictly below the period length).
        assert_noop!(
            NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::NextHeartbeatPeriod(1)
            ),
            Error::<TestRuntime>::NextHeartbeatPeriodInvalid,
        );
    });
}

#[test]
fn migration_seeds_genesis_equivalent_storage() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        // The mock ext leaves the version at 0; a real set_code introduction
        // pre-initialises it to the in-code version (1) instead. The seeder
        // accepts both (its gate is `<= SEEDED` + the data tell-tale);
        // migration_gate_is_the_data_not_the_storage_version covers the
        // pre-initialised variant.
        assert_eq!(Pallet::<TestRuntime>::on_chain_storage_version(), StorageVersion::new(0));

        let _ = Migration::on_runtime_upgrade();

        // Matches GenesisConfig::default() in lib.rs.
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), 1);
        assert_eq!(NextRewardPeriodLength::<TestRuntime>::get(), 2);
        assert_eq!(NextHeartbeatPeriod::<TestRuntime>::get(), 1);
        assert_eq!(RewardPeriod::<TestRuntime>::get().length, 2, "no longer the junk 20");
        assert!(MinUptimeThreshold::<TestRuntime>::get().is_some());
        // Version is still bumped for hygiene (marks the pallet as touched), even
        // though the gate no longer reads it.
        assert_eq!(Pallet::<TestRuntime>::on_chain_storage_version(), StorageVersion::new(1));
    });
}

#[test]
fn after_migration_the_pallet_is_usable() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let _ = Migration::on_runtime_upgrade();
        // The whole point: the seeded config makes the admin surface usable
        // again - a longer reward period, then a heartbeat period below it.
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
fn migration_gate_is_the_data_not_the_storage_version() {
    // Regression guard for a real bug: the first cut gated on
    // `on_chain_storage_version() < 1`, which passed every mock test but SKIPPED
    // on a real forkless upgrade - a pallet added by `set_code` has its on-chain
    // version pre-initialised to the in-code STORAGE_VERSION (1), so `1 < 1` is
    // false. The gate must be the data tell-tale (`MaxBatchSize == 0`) instead.
    //
    // This test forces the exact production condition the mock otherwise hides:
    // version already at the seeded value, but data still unseeded. The migration
    // MUST still fire.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<TestRuntime>>();
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), 0, "still unseeded");

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(
            MaxBatchSize::<TestRuntime>::get(),
            1,
            "migration skipped because the version was already 1 - the production bug",
        );
        assert_eq!(RewardPeriod::<TestRuntime>::get().length, 2);
    });
}

#[test]
fn migration_retires_once_the_version_moves_past_the_seeded_layout() {
    // The version check the retirement contract promises: after a future
    // migration bumps the pallet past SEEDED_STORAGE_VERSION, this seeder must
    // never fire again - even if the data happens to look unseeded (a future
    // layout is free to give MaxBatchSize new semantics, including 0).
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        StorageVersion::new(SEEDED_STORAGE_VERSION + 1).put::<Pallet<TestRuntime>>();
        assert_eq!(MaxBatchSize::<TestRuntime>::get(), 0, "data looks unseeded on purpose");

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(
            MaxBatchSize::<TestRuntime>::get(),
            0,
            "retired seeder fired on a newer storage layout",
        );
        assert_eq!(
            Pallet::<TestRuntime>::on_chain_storage_version(),
            StorageVersion::new(SEEDED_STORAGE_VERSION + 1),
            "retired seeder rewrote the storage version",
        );
    });
}

#[test]
fn migration_converges_a_version_zero_genesis_chain_without_touching_data() {
    // A chain whose genesis ran on a runtime that still declared version 0 has
    // seeded data but sits below SEEDED_STORAGE_VERSION. The seeder must bump
    // the version and leave the data alone.
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
    });
}

#[test]
fn migration_is_idempotent() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let _ = Migration::on_runtime_upgrade();
        // Mutate a seeded value, then run again: the second pass must NOT clobber
        // it, because MaxBatchSize is now non-zero so the data gate skips.
        MaxBatchSize::<TestRuntime>::put(500);
        let _ = Migration::on_runtime_upgrade();
        assert_eq!(
            MaxBatchSize::<TestRuntime>::get(),
            500,
            "re-running the migration re-seeded over live data",
        );
    });
}

#[test]
fn migration_does_not_touch_a_genesis_started_chain() {
    // A chain that ran genesis_build already has real config and is at the
    // declared storage version. The migration must be inert.
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let batch_before = MaxBatchSize::<TestRuntime>::get();
        let period_before = RewardPeriod::<TestRuntime>::get().length;
        assert!(batch_before > 0, "genesis_config sets a real batch size");

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(MaxBatchSize::<TestRuntime>::get(), batch_before, "genesis data clobbered");
        assert_eq!(RewardPeriod::<TestRuntime>::get().length, period_before);
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

#[test]
fn migration_does_not_clobber_a_configured_lock_window() {
    // If root already set a real Global Start Date, a re-run must leave it be.
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let _ = Migration::on_runtime_upgrade();
        let configured = crate::types::LockScheduleInfo::new(1_234_567, 31);
        LockSchedule::<TestRuntime>::put(configured);

        let _ = Migration::on_runtime_upgrade();

        assert_eq!(
            LockSchedule::<TestRuntime>::get(),
            Some(configured),
            "re-running the migration overwrote an operator-configured window",
        );
    });
}
