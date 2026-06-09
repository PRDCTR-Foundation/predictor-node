// Copyright 2024-2025 Forecasting Technologies LTD.
//
// This file is part of Predictor.
//
// Predictor is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at
// your option) any later version.
//
// Predictor is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Predictor. If not, see <https://www.gnu.org/licenses/>.

#![allow(unused_parens)]
#![allow(unused_imports)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

pub trait SignedWeightInfo {
    fn signed_join(n: u32) -> Weight;
    fn signed_withdraw_fees() -> Weight;
    fn signed_exit(n: u32) -> Weight;
}

pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> SignedWeightInfo for WeightInfo<T> {
    /// Storage: `MarketCommons::Markets` (r:1 w:0)
    /// Storage: `NeoSwaps::Pools` (r:1 w:1)
    /// Storage: `Tokens::Accounts` (r:256 w:256)
    /// Storage: `System::Account` (r:1 w:0)
    fn signed_join(n: u32) -> Weight {
        Weight::from_parts(140_527_727, 148211)
            .saturating_add(Weight::from_parts(20_021_386, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(3_u64))
            .saturating_add(T::DbWeight::get().reads((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().writes(1_u64))
            .saturating_add(T::DbWeight::get().writes((2_u64).saturating_mul(n.into())))
            .saturating_add(Weight::from_parts(0, 5196).saturating_mul(n.into()))
    }

    /// Storage: `NeoSwaps::Pools` (r:1 w:1)
    /// Storage: `System::Account` (r:2 w:2)
    fn signed_withdraw_fees() -> Weight {
        Weight::from_parts(204_537_000, 148211)
            .saturating_add(T::DbWeight::get().reads(3_u64))
            .saturating_add(T::DbWeight::get().writes(3_u64))
    }

    /// Storage: `MarketCommons::Markets` (r:1 w:0)
    /// Storage: `NeoSwaps::Pools` (r:1 w:1)
    /// Storage: `Tokens::Accounts` (r:256 w:256)
    /// Storage: `System::Account` (r:1 w:0)
    fn signed_exit(n: u32) -> Weight {
        Weight::from_parts(229_835_711, 148211)
            .saturating_add(Weight::from_parts(21_115_060, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(3_u64))
            .saturating_add(T::DbWeight::get().reads((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().writes(1_u64))
            .saturating_add(T::DbWeight::get().writes((2_u64).saturating_mul(n.into())))
            .saturating_add(Weight::from_parts(0, 5196).saturating_mul(n.into()))
    }
}
