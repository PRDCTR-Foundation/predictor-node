// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-08-27.

//! Storage migrations for `pallet-node-manager`.
//!
//! A pallet added to a running chain by `System::set_code` never has its `genesis_build`
//! executed. Almost everything `genesis_build` writes is now covered by storage defaults
//! (`MaxBatchSize`, the next period length and heartbeat period, `MinUptimeThreshold`,
//! `RewardPeriod`), so only two things still need seeding on that path:
//!
//!   - The reward pot account's provider reference. On a zero-existential-deposit chain a credit to
//!     a provider-less account does not persist, so the first `top_up_reward_pot` would be lost.
//!   - `LockSchedule`. Left unset, payouts lock while `withdraw_rewards` rejects with
//!     `LockScheduleNotSet`, so rewards would accrue unclaimable. `GenesisConfig` leaves it unset
//!     because a chain spec configures it before anyone can earn. An upgraded chain has no such
//!     moment, so a live window is anchored at the upgrade instead. Root overrides it via
//!     `AdminConfig::LockSchedule`.
//!
//! `SeedGenesisOnUpgrade` runs once, gated on the pot account having no provider (the exact
//! tell-tale of a missing `genesis_build`), and is a no-op on a genesis-started chain.

use crate::{
    pallet::Config, LockSchedule, LockScheduleInfo, Pallet, DEFAULT_LOCK_INITIAL_PENALTY_PERCENT,
};
use frame_support::{
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};

/// The storage version this migration brings the pallet up to. The pallet's declared
/// `STORAGE_VERSION` is deliberately kept BELOW this number: a pallet introduced by `set_code`
/// pre-initialises its on-chain version to the in-code one, so the seeding decision is driven by
/// data, not by a version comparison (see `needs_seeding`).
///
/// Once a future migration moves the on-chain version PAST this number, this seeder retires
/// permanently. If the declared `STORAGE_VERSION` is ever bumped to or past this number, that
/// version's migration must take over the introduction seeding.
pub const SEEDED_STORAGE_VERSION: u16 = 1;

/// Seed what `genesis_build` would have written that storage defaults cannot express, for a
/// pallet introduced by a forkless upgrade. Idempotent and gated on the on-chain storage version.
pub struct SeedGenesisOnUpgrade<T>(core::marker::PhantomData<T>);

impl<T: Config> SeedGenesisOnUpgrade<T> {
    /// `genesis_build` always gives the reward pot account a provider reference, and nothing
    /// else does before the first top-up, so no provider means `genesis_build` never ran. After
    /// seeding there is one, so a re-run is a no-op and a genesis-started chain is never touched.
    ///
    /// This deliberately does NOT use a lower-bound version gate (`on_chain < SEEDED`): a pallet
    /// introduced by `set_code` has its on-chain version pre-initialised to the in-code
    /// `STORAGE_VERSION`, so such a gate would treat a freshly introduced pallet as already
    /// seeded. Mock runtimes hide this; only a real forkless upgrade exposes it.
    fn needs_seeding() -> bool {
        frame_system::Pallet::<T>::providers(&Pallet::<T>::compute_reward_account_id()) == 0
    }

    fn seed() {
        frame_system::Pallet::<T>::inc_providers(&Pallet::<T>::compute_reward_account_id());

        LockSchedule::<T>::put(LockScheduleInfo::new(
            Pallet::<T>::time_now_sec(),
            DEFAULT_LOCK_INITIAL_PENALTY_PERCENT,
        ));
    }
}

impl<T: Config> OnRuntimeUpgrade for SeedGenesisOnUpgrade<T> {
    fn on_runtime_upgrade() -> Weight {
        // Retirement gate: once a future migration has moved the pallet past the seeded layout,
        // this seeder must never run again, whatever the data looks like.
        let on_chain = Pallet::<T>::on_chain_storage_version();
        if on_chain > StorageVersion::new(SEEDED_STORAGE_VERSION) {
            log::info!(
                target: "runtime::node-manager",
                "SeedGenesisOnUpgrade: retired (on-chain version {on_chain:?} > {SEEDED_STORAGE_VERSION}), skipping",
            );
            return T::DbWeight::get().reads(1)
        }

        if !Self::needs_seeding() {
            // Already seeded (genesis chain). Converge the version so both paths land at
            // SEEDED_STORAGE_VERSION.
            if on_chain < StorageVersion::new(SEEDED_STORAGE_VERSION) {
                StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<T>>();
                log::info!(
                    target: "runtime::node-manager",
                    "SeedGenesisOnUpgrade: storage already seeded, converged version {on_chain:?} -> {SEEDED_STORAGE_VERSION}",
                );
                return T::DbWeight::get().reads_writes(2, 1)
            }
            log::info!(
                target: "runtime::node-manager",
                "SeedGenesisOnUpgrade: storage already seeded (version {on_chain:?}), skipping",
            );
            return T::DbWeight::get().reads(2)
        }

        log::info!(
            target: "runtime::node-manager",
            "SeedGenesisOnUpgrade: seeding genesis-equivalent storage for forkless introduction",
        );
        Self::seed();
        StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<T>>();

        // Version + provider reads; provider, lock schedule and version writes.
        T::DbWeight::get().reads_writes(2, 3)
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
        Ok(sp_std::vec![])
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(_state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        // A retired seeder makes no claims about a newer layout.
        if Pallet::<T>::on_chain_storage_version() > StorageVersion::new(SEEDED_STORAGE_VERSION) {
            return Ok(())
        }
        frame_support::ensure!(
            !Self::needs_seeding(),
            "SeedGenesisOnUpgrade: reward pot account has no provider reference",
        );
        // A seeded window must exist, otherwise payouts accrue into a lock that
        // `withdraw_rewards` will not open.
        let schedule = LockSchedule::<T>::get().ok_or(
            "SeedGenesisOnUpgrade: LockSchedule not seeded (withdrawals would be bricked)",
        )?;
        frame_support::ensure!(
            schedule.initial_penalty_percent == DEFAULT_LOCK_INITIAL_PENALTY_PERCENT,
            "SeedGenesisOnUpgrade: LockSchedule seeded with unexpected parameters",
        );
        frame_support::ensure!(
            Pallet::<T>::on_chain_storage_version() == StorageVersion::new(SEEDED_STORAGE_VERSION),
            "SeedGenesisOnUpgrade: storage version not bumped",
        );
        Ok(())
    }
}
