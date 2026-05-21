use frame_support::{
    parameter_types,
    weights::{WeightToFeeCoefficient, WeightToFeeCoefficients, WeightToFeePolynomial},
};

use crate::{
    configs::{Currency, ExtrinsicBaseWeight, OnUnbalanced},
    AccountId, Avn, Balance, PalletConfig, SLOT_DURATION,
};
use smallvec::smallvec;
use sp_avn_common::QuorumPolicy;
use sp_runtime::{FixedPointNumber, FixedU128, Perbill};

parameter_types! {
    pub const MinimumPeriodValue: u64 = SLOT_DURATION / 2;
}

// Timestamp
/// Custom getter for minimum timestamp delta.
/// This ensures that consensus systems like Aura don't break assertions
/// in a benchmark environment
pub struct MinimumPeriod;
impl MinimumPeriod {
    /// Returns the value of this parameter type.
    pub fn get() -> u64 {
        #[cfg(feature = "runtime-benchmarks")]
        {
            use frame_benchmarking::benchmarking::get_whitelist;
            // Should that condition be true, we can assume that we are in a benchmark environment.
            if !get_whitelist().is_empty() {
                return u64::MAX
            }
        }

        MinimumPeriodValue::get()
    }
}
impl<I: From<u64>> frame_support::traits::Get<I> for MinimumPeriod {
    fn get() -> I {
        I::from(Self::get())
    }
}
impl frame_support::traits::TypedGet for MinimumPeriod {
    type Type = u64;
    fn get() -> u64 {
        Self::get()
    }
}

/// Handles converting a weight scalar to a fee value, based on the scale and granularity of the
/// node's balance type.
///
/// This should typically create a mapping between the following ranges:
///   - `[0, MAXIMUM_BLOCK_WEIGHT]`
///   - `[Balance::min, Balance::max]`
///
/// Yet, it can be used for any other sort of change to weight-fee. Some examples being:
///   - Setting it to `0` will essentially disable the weight fee.
///   - Setting it to `1` will cause the literal `#[weight = x]` values to be charged.
pub struct WeightToFee;
impl WeightToFeePolynomial for WeightToFee {
    type Balance = Balance;
    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        // We adjust the fee conversion so that a simple token transfer
        // direct to chain costs base_fee TRUU.
        let base_fee = PalletConfig::base_gas_fee();

        // The magic number (2.380951) is the result of :
        // setting p = 50 * MILLI_BASE, the cost of a simple transfer was 119.04775 milli TRUU
        // (visual observation on polkadot.js). magic_number = 119.04775 / 50 = 2.380951
        let factor = FixedU128::saturating_from_rational(1_000_000u128, 2_380_951u128);

        let p = factor.saturating_mul_int(base_fee);
        let q = Balance::from(ExtrinsicBaseWeight::get().ref_time());
        smallvec![WeightToFeeCoefficient {
            degree: 1,
            negative: false,
            coeff_frac: Perbill::from_rational(p % q, q),
            coeff_integer: p / q,
        }]
    }
}

/// ORML adapter
pub type NegativeImbalance<T> = <pallet_balances::Pallet<T> as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

pub struct Treasury<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for Treasury<R>
where
    R: pallet_balances::Config + pallet_token_manager::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_nonzero_unbalanced(amount: NegativeImbalance<R>) {
        let recipient: <R as frame_system::Config>::AccountId = PalletConfig::gas_fee_recipient()
            .map(Into::into)
            .unwrap_or_else(|_| <pallet_token_manager::Pallet<R>>::compute_treasury_account_id());

        <pallet_balances::Pallet<R>>::resolve_creating(&recipient, amount);
    }
}

pub struct MajorityQuorum {}

impl QuorumPolicy for MajorityQuorum {
    const QUORUM_PERCENT: u32 = 51;
    const SUPERMAJORITY_PERCENT: u32 = 67;

    fn get_quorum() -> u32 {
        let total_num_of_validators = Avn::validators().len() as u32;
        Self::required_for(total_num_of_validators)
    }

    fn get_supermajority_quorum() -> u32 {
        let total_num_of_validators = Avn::validators().len() as u32;
        Self::required_for_supermajority(total_num_of_validators)
    }
}
