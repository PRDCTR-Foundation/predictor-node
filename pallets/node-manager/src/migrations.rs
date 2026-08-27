// Copyright 2026 Aventus DAO.

//! Storage migrations for `pallet-node-manager`.
//!
//! # Why this exists
//!
//! A pallet added to a running chain by `System::set_code` (a forkless upgrade)
//! never has its `genesis_build` executed - that hook only runs when a chain is
//! started from a genesis config that includes the pallet. Introduced by
//! upgrade, every storage item therefore lands on its *type* default rather than
//! the value `genesis_build` would have written.
//!
//! For this pallet that is not a cosmetic gap, it is a brick:
//!
//!   - `MaxBatchSize` defaults to `0`, so `drain_outstanding_payouts` hits its
//!     batch cap immediately and pays nobody, ever - accrued rewards never
//!     drain.
//!   - `NextRewardPeriodLength` and `NextHeartbeatPeriod` default to `0`, so
//!     `on_initialize` never rolls a reward period, and
//!     `AdminConfig::NextHeartbeatPeriod` is unsettable (it must be strictly
//!     below the period length) until root first repairs the period length.
//!   - `RewardPeriod` (a `ValueQuery` item) resolves to
//!     `RewardPeriodInfo::default()`, whose hand-written default is
//!     `length: 20, heartbeat_period: 10, uptime_threshold: u32::MAX` - a third
//!     value that agrees with neither the `GenesisConfig` default nor zero.
//!   - The reward pot account never gets its provider reference, so on a
//!     zero-existential-deposit chain the first rollover transfer into it is
//!     silently lost.
//!   - `LockSchedule` is `None`, which the payout path reads as "locked"
//!     (lock-by-default) while `withdraw_rewards` rejects with
//!     `LockScheduleNotSet`. Rewards would accrue that nobody can ever claim
//!     until root happens to configure a window.
//!
//! This migration seeds exactly what `genesis_build` seeds, so a chain upgraded
//! into the pallet begins in the same state as one started from genesis with the
//! `GenesisConfig` defaults. It runs once, gated on the on-chain
//! `StorageVersion`, and is a no-op on any chain that already ran
//! `genesis_build` (which leaves the version at whatever the pallet declares).
//!
//! Keep the seeded values in lockstep with `GenesisConfig::default()` in
//! `lib.rs`. The single point of truth for "what a fresh pallet looks like" is
//! that `Default` impl; this migration mirrors it for the upgrade path.
//!
//! One value deliberately diverges: `GenesisConfig` leaves `LockSchedule`
//! unset, because a chain started from genesis configures the window in its
//! chain spec before anyone can earn. An upgraded chain has no such moment, so
//! the migration seeds a live window anchored at the upgrade block rather than
//! leaving withdrawals bricked. Both paths end in the same lock-by-default
//! posture; only the upgrade path needs a concrete anchor to get there.

use crate::{
    pallet::Config, HalvingEnabled, LockSchedule, LockScheduleInfo, MaxBatchSize,
    MinUptimeThreshold, NextHeartbeatPeriod, NextRewardAmountPerPeriod, NextRewardPeriodLength,
    OutstandingRewardToPay, Pallet, RewardPeriod, RewardPeriodInfo,
};
use frame_support::{
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};
use sp_runtime::traits::Zero;

/// Genesis-equivalent defaults for a forkless introduction. These mirror
/// `GenesisConfig::default()` in `lib.rs` and must be changed together with it.
mod genesis_defaults {
    pub const MAX_BATCH_SIZE: u32 = 1;
    pub const REWARD_PERIOD: u32 = 2;
    pub const HEARTBEAT_PERIOD: u32 = 1;
    /// Week-one forfeiture rate; the window's length is implied by the 1%-per-
    /// week decay (52% -> zero after 52 weeks).
    pub const LOCK_INITIAL_PENALTY_PERCENT: u32 = 52;
}

/// The storage version this migration brings the pallet up to. The pallet
/// declares `STORAGE_VERSION` at the same number (keep them in lockstep); a
/// chain that ran `genesis_build` is already seeded, so
/// [`SeedGenesisOnUpgrade`] leaves its data alone.
///
/// Once a future migration moves the on-chain version PAST this number, this
/// seeder retires permanently - see the version gate in `on_runtime_upgrade`.
/// If `STORAGE_VERSION` is ever bumped beyond 1, a pallet introduced by
/// `set_code` at that point pre-initialises at the new version, so THIS seeder
/// will (correctly, per its retirement contract) not run - the new version's
/// migration must take over the introduction seeding.
pub const SEEDED_STORAGE_VERSION: u16 = 1;

/// Seed the storage `genesis_build` would have written, for a pallet introduced
/// by a forkless upgrade. Idempotent and gated on the on-chain storage version.
pub struct SeedGenesisOnUpgrade<T>(core::marker::PhantomData<T>);

impl<T: Config> SeedGenesisOnUpgrade<T> {
    /// The predicate the migration acts on: `MaxBatchSize == 0` is the exact
    /// tell-tale of a missing `genesis_build`. `MaxBatchSize` is validated to
    /// `1..=MAX_BATCH_SIZE` whenever it is set (genesis or `set_admin_config`),
    /// so `0` is only ever the un-seeded state, and never a value an operator
    /// can produce. That makes this gate both correct and idempotent: after
    /// seeding it is `1`, so a re-run is a no-op, and a genesis-started chain
    /// (where it is already `>= 1`) is never touched.
    ///
    /// NB: this deliberately does NOT use a LOWER-bound version gate
    /// (`on_chain < SEEDED`). A pallet introduced by `set_code` has its
    /// on-chain storage version pre-initialised to the in-code
    /// `STORAGE_VERSION` (1) - it is never below current for a freshly-added
    /// pallet - so such a gate would skip the very case this migration exists
    /// for. That failure is invisible to a mock runtime (where the pallet is
    /// always present) and only surfaces on a real forkless upgrade. The
    /// UPPER-bound retirement gate in `on_runtime_upgrade` is the only version
    /// check that is safe here.
    fn needs_seeding() -> bool {
        MaxBatchSize::<T>::get() == 0
    }

    fn seed() {
        use genesis_defaults::*;

        // Same provider-reference fix genesis_build applies: on a zero-ED chain
        // a credit to the provider-less pot account would not persist.
        frame_system::Pallet::<T>::inc_providers(&Pallet::<T>::compute_reward_account_id());

        let default_threshold = Pallet::<T>::get_default_threshold();
        NextRewardPeriodLength::<T>::set(REWARD_PERIOD);
        NextHeartbeatPeriod::<T>::set(HEARTBEAT_PERIOD);
        MaxBatchSize::<T>::set(MAX_BATCH_SIZE);
        NextRewardAmountPerPeriod::<T>::set(Zero::zero());
        MinUptimeThreshold::<T>::set(Some(default_threshold));
        OutstandingRewardToPay::<T>::set(Zero::zero());
        HalvingEnabled::<T>::set(T::HalvingEnabledAtGenesis::get());

        let uptime_threshold =
            Pallet::<T>::calculate_uptime_threshold(REWARD_PERIOD, HEARTBEAT_PERIOD);
        RewardPeriod::<T>::put(RewardPeriodInfo::new(
            0u64,
            Zero::zero(),
            REWARD_PERIOD,
            HEARTBEAT_PERIOD,
            uptime_threshold,
            Zero::zero(),
        ));

        // Anchor the lock window at the upgrade itself. `GenesisConfig` leaves
        // the schedule unset (`lock_schedule_start: None`), but "unset" is not a
        // sensible default on the upgrade path: the payout path treats it as
        // locked while `withdraw_rewards` refuses to run, so rewards would pile
        // up unclaimable. Seeding the proposal's 52%-decaying-1%-per-week curve
        // from the upgrade block keeps the lock semantics intended for the T1
        // migration while leaving the pallet immediately usable. Root overrides
        // both the anchor and the shape via `AdminConfig::LockSchedule` once the
        // real Global Start Date is known.
        LockSchedule::<T>::put(LockScheduleInfo::new(
            Pallet::<T>::time_now_sec(),
            LOCK_INITIAL_PENALTY_PERCENT,
        ));
    }
}

impl<T: Config> OnRuntimeUpgrade for SeedGenesisOnUpgrade<T> {
    fn on_runtime_upgrade() -> Weight {
        // Retirement gate: once a future migration has moved the pallet past
        // the seeded layout, this seeder must never run again - whatever the
        // data looks like. This is the only SAFE direction for a version check
        // here (see `needs_seeding` for why a lower bound is not).
        let on_chain = Pallet::<T>::on_chain_storage_version();
        if on_chain > StorageVersion::new(SEEDED_STORAGE_VERSION) {
            log::info!(
                target: "runtime::node-manager",
                "SeedGenesisOnUpgrade: retired (on-chain version {:?} > {}), skipping",
                on_chain,
                SEEDED_STORAGE_VERSION,
            );
            return T::DbWeight::get().reads(1);
        }

        if !Self::needs_seeding() {
            // Already seeded (genesis chain). Converge the version so both
            // paths land at SEEDED_STORAGE_VERSION: a chain whose genesis ran
            // on a runtime that still declared version 0 sits below it.
            if on_chain < StorageVersion::new(SEEDED_STORAGE_VERSION) {
                StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<T>>();
                log::info!(
                    target: "runtime::node-manager",
                    "SeedGenesisOnUpgrade: storage already seeded, converged version {:?} -> {}",
                    on_chain,
                    SEEDED_STORAGE_VERSION,
                );
                return T::DbWeight::get().reads_writes(2, 1);
            }
            log::info!(
                target: "runtime::node-manager",
                "SeedGenesisOnUpgrade: storage already seeded (version {:?}), skipping",
                on_chain,
            );
            return T::DbWeight::get().reads(2);
        }

        log::info!(
            target: "runtime::node-manager",
            "SeedGenesisOnUpgrade: seeding genesis-equivalent storage for forkless introduction",
        );
        Self::seed();
        StorageVersion::new(SEEDED_STORAGE_VERSION).put::<Pallet<T>>();

        // ~9 writes + inc_providers + version write, plus version + data reads.
        T::DbWeight::get().reads_writes(2, 11)
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
        Ok(sp_std::vec![])
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(_state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        use genesis_defaults::*;
        // A retired seeder makes no claims about a newer layout.
        if Pallet::<T>::on_chain_storage_version() > StorageVersion::new(SEEDED_STORAGE_VERSION) {
            return Ok(());
        }
        // After the migration, the pallet must be in a usable state: a non-zero
        // batch size (so the drain rate is > 0) and a reward period that matches
        // the genesis defaults rather than the junk type default of 20.
        frame_support::ensure!(
            MaxBatchSize::<T>::get() == MAX_BATCH_SIZE,
            "SeedGenesisOnUpgrade: MaxBatchSize not seeded",
        );
        frame_support::ensure!(
            RewardPeriod::<T>::get().length == REWARD_PERIOD,
            "SeedGenesisOnUpgrade: RewardPeriod.length not seeded (still the type default?)",
        );
        // A seeded window must exist and be well-formed, otherwise payouts
        // accrue into a lock that `withdraw_rewards` will not open.
        let schedule = LockSchedule::<T>::get().ok_or(
            "SeedGenesisOnUpgrade: LockSchedule not seeded (withdrawals would be bricked)",
        )?;
        frame_support::ensure!(
            schedule.initial_penalty_percent == LOCK_INITIAL_PENALTY_PERCENT,
            "SeedGenesisOnUpgrade: LockSchedule seeded with unexpected parameters",
        );
        frame_support::ensure!(
            Pallet::<T>::on_chain_storage_version() == StorageVersion::new(SEEDED_STORAGE_VERSION),
            "SeedGenesisOnUpgrade: storage version not bumped",
        );
        Ok(())
    }
}
