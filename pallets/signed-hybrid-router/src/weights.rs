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
    fn signed_buy(n: u32, o: u32) -> Weight;
    fn signed_sell(n: u32, o: u32) -> Weight;
}

pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> SignedWeightInfo for WeightInfo<T> {
    /// Storage: `SignedHybridRouter::MarketNonces` (r:1 w:1)
    /// Storage: `MarketCommons::Markets` (r:1 w:0)
    /// Storage: `System::Account` (r:13 w:13)
    /// Storage: `Orderbook::Orders` (r:10 w:11)
    /// Storage: `NeoSwaps::Pools` (r:1 w:1)
    /// Storage: `Tokens::Accounts` (r:27 w:27)
    /// Storage: `Tokens::TotalIssuance` (r:16 w:16)
    /// Storage: `Tokens::Reserves` (r:10 w:10)
    /// Storage: `Orderbook::NextOrderId` (r:1 w:1)
    /// Storage: `Balances::Reserves` (r:1 w:1)
    fn signed_buy(n: u32, o: u32) -> Weight {
        Weight::from_parts(718_870_000, 148211)
            .saturating_add(Weight::from_parts(34_084_144, 0).saturating_mul(n.into()))
            .saturating_add(Weight::from_parts(463_352_584, 0).saturating_mul(o.into()))
            .saturating_add(T::DbWeight::get().reads(9_u64))
            .saturating_add(T::DbWeight::get().reads((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().reads((4_u64).saturating_mul(o.into())))
            .saturating_add(T::DbWeight::get().writes(9_u64))
            .saturating_add(T::DbWeight::get().writes((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().writes((4_u64).saturating_mul(o.into())))
            .saturating_add(Weight::from_parts(0, 2598).saturating_mul(n.into()))
            .saturating_add(Weight::from_parts(0, 3751).saturating_mul(o.into()))
    }

    /// Storage: `SignedHybridRouter::MarketNonces` (r:1 w:1)
    /// Storage: `MarketCommons::Markets` (r:1 w:0)
    /// Storage: `Tokens::Accounts` (r:21 w:21)
    /// Storage: `Orderbook::Orders` (r:10 w:11)
    /// Storage: `NeoSwaps::Pools` (r:1 w:1)
    /// Storage: `System::Account` (r:13 w:13)
    /// Storage: `Tokens::TotalIssuance` (r:10 w:10)
    /// Storage: `Balances::Reserves` (r:10 w:10)
    /// Storage: `Orderbook::NextOrderId` (r:1 w:1)
    /// Storage: `Tokens::Reserves` (r:1 w:1)
    fn signed_sell(n: u32, o: u32) -> Weight {
        Weight::from_parts(737_892_000, 148211)
            .saturating_add(Weight::from_parts(52_364_473, 0).saturating_mul(n.into()))
            .saturating_add(Weight::from_parts(458_125_238, 0).saturating_mul(o.into()))
            .saturating_add(T::DbWeight::get().reads(9_u64))
            .saturating_add(T::DbWeight::get().reads((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().reads((4_u64).saturating_mul(o.into())))
            .saturating_add(T::DbWeight::get().writes(9_u64))
            .saturating_add(T::DbWeight::get().writes((2_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().writes((4_u64).saturating_mul(o.into())))
            .saturating_add(Weight::from_parts(0, 2598).saturating_mul(n.into()))
            .saturating_add(Weight::from_parts(0, 3724).saturating_mul(o.into()))
    }
}
