use frame_support::parameter_types;

parameter_types! {
    pub const MinimumPeriodValue: u64 = SLOT_DURATION / 2;
}

use crate::SLOT_DURATION;

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
