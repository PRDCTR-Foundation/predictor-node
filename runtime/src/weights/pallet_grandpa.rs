// Hand-written reference weights for `pallet_grandpa`.
//
// `pallet-grandpa-38.0.0` only exports `impl WeightInfo for ()` (zero weights).
// The numbers below are conservative reference values inspired by the
// upstream Polkadot benchmarks for the same pallet. They are intended as
// "non-zero, defensible" placeholders — not authoritative production weights.
// Re-benchmark on Predictor's reference hardware before mainnet.

#![allow(clippy::unnecessary_cast)]

use core::marker::PhantomData;
use frame_support::{
    traits::Get,
    weights::{constants::RocksDbWeight, Weight},
};

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> pallet_grandpa::WeightInfo for SubstrateWeight<T> {
    /// `report_equivocation` verifies a key-ownership proof and submits the
    /// offence, scaling roughly linearly with `validator_count`.
    fn report_equivocation(validator_count: u32, max_nominators_per_validator: u32) -> Weight {
        Weight::from_parts(95_000_000, 4_000)
            .saturating_add(Weight::from_parts(80_000, 0).saturating_mul(validator_count as u64))
            .saturating_add(
                Weight::from_parts(60_000, 0).saturating_mul(max_nominators_per_validator as u64),
            )
            .saturating_add(T::DbWeight::get().reads(7))
            .saturating_add(T::DbWeight::get().writes(2))
    }

    /// `note_stalled` is a single storage write.
    fn note_stalled() -> Weight {
        Weight::from_parts(3_500_000, 0).saturating_add(RocksDbWeight::get().writes(1))
    }
}
