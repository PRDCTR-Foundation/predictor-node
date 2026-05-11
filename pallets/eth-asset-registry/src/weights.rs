#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(clippy::unnecessary_cast)]

use frame_support::{
	traits::Get,
	weights::{constants::RocksDbWeight, Weight},
};
use sp_std::marker::PhantomData;

pub trait WeightInfo {
	fn register_asset() -> Weight;
	fn update_asset() -> Weight;
	fn set_asset_location() -> Weight;
}

/// Reference weights for the asset-registry extrinsics.
///
/// These are conservative reference values modelled on the upstream
/// `orml_asset_registry` benchmarks and adjusted for the storage shape of
/// this pallet.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Register a new asset: 1 read (uniqueness check) + 2 writes
	/// (metadata + location index).
	fn register_asset() -> Weight {
		Weight::from_parts(28_000_000, 3_597)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(2))
	}

	/// Update an existing asset's metadata: 1 read + 1 write.
	fn update_asset() -> Weight {
		Weight::from_parts(22_000_000, 3_597)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// Update an asset's location mapping: 1 read + 1 write.
	fn set_asset_location() -> Weight {
		Weight::from_parts(20_000_000, 3_597)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}
}

/// Default zero weights — kept so existing call sites that pass `()` keep working
/// (e.g. mocks in `pallets/eth-asset-registry/src/mock.rs`).
impl WeightInfo for () {
	fn register_asset() -> Weight {
		Weight::zero()
	}
	fn update_asset() -> Weight {
		Weight::zero()
	}
	fn set_asset_location() -> Weight {
		Weight::zero()
	}
}
