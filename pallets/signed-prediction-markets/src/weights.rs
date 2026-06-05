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

#![allow(unused_imports)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

pub trait SignedWeightInfo {
    fn signed_create_market_and_deploy_pool(m: u32, n: u32) -> Weight;
    fn signed_withdraw_tokens() -> Weight;
    fn signed_redeem_shares_categorical() -> Weight;
    fn signed_redeem_shares_scalar() -> Weight;
    fn signed_transfer_asset() -> Weight;
    fn signed_report_market_with_dispute_mechanism(m: u32) -> Weight;
    fn signed_report_trusted_market() -> Weight;
    fn signed_buy_complete_set(a: u32) -> Weight;
}

pub struct WeightInfo<T>(PhantomData<T>);

impl<T: frame_system::Config> SignedWeightInfo for WeightInfo<T> {
    fn signed_create_market_and_deploy_pool(m: u32, n: u32) -> Weight {
        let _ = m;
        Weight::from_parts(239_872_618, 148211)
            .saturating_add(Weight::from_parts(47_247_235, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(7))
            .saturating_add(T::DbWeight::get().reads((3_u64).saturating_mul(n.into())))
            .saturating_add(T::DbWeight::get().writes(7))
            .saturating_add(T::DbWeight::get().writes((3_u64).saturating_mul(n.into())))
            .saturating_add(Weight::from_parts(0, 5196).saturating_mul(n.into()))
    }

    fn signed_withdraw_tokens() -> Weight {
        Weight::from_parts(421_048_000, 6188)
            .saturating_add(T::DbWeight::get().reads(6_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }

    fn signed_redeem_shares_categorical() -> Weight {
        Weight::from_parts(110_665_000, 6196)
            .saturating_add(T::DbWeight::get().reads(5_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }

    fn signed_redeem_shares_scalar() -> Weight {
        Weight::from_parts(126_747_000, 6196)
            .saturating_add(T::DbWeight::get().reads(7_u64))
            .saturating_add(T::DbWeight::get().writes(6_u64))
    }

    fn signed_transfer_asset() -> Weight {
        Weight::from_parts(155_927_000, 6196)
            .saturating_add(T::DbWeight::get().reads(7_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }

    fn signed_report_market_with_dispute_mechanism(m: u32) -> Weight {
        let _ = m;
        Weight::from_parts(160_076_189, 4503)
            .saturating_add(T::DbWeight::get().reads(4_u64))
            .saturating_add(T::DbWeight::get().writes(3_u64))
    }

    fn signed_report_trusted_market() -> Weight {
        Weight::from_parts(194_705_000, 4714)
            .saturating_add(T::DbWeight::get().reads(4_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }

    fn signed_buy_complete_set(a: u32) -> Weight {
        Weight::from_parts(187_955_271, 6196)
            .saturating_add(Weight::from_parts(14_113_418, 0).saturating_mul(a.into()))
            .saturating_add(T::DbWeight::get().reads(4_u64))
            .saturating_add(T::DbWeight::get().reads((2_u64).saturating_mul(a.into())))
            .saturating_add(T::DbWeight::get().writes(3_u64))
            .saturating_add(T::DbWeight::get().writes((2_u64).saturating_mul(a.into())))
            .saturating_add(Weight::from_parts(0, 2598).saturating_mul(a.into()))
    }
}

impl SignedWeightInfo for () {
    fn signed_create_market_and_deploy_pool(_m: u32, _n: u32) -> Weight {
        Weight::zero()
    }

    fn signed_withdraw_tokens() -> Weight {
        Weight::zero()
    }

    fn signed_redeem_shares_categorical() -> Weight {
        Weight::zero()
    }

    fn signed_redeem_shares_scalar() -> Weight {
        Weight::zero()
    }

    fn signed_transfer_asset() -> Weight {
        Weight::zero()
    }

    fn signed_report_market_with_dispute_mechanism(_m: u32) -> Weight {
        Weight::zero()
    }

    fn signed_report_trusted_market() -> Weight {
        Weight::zero()
    }

    fn signed_buy_complete_set(_a: u32) -> Weight {
        Weight::zero()
    }
}
