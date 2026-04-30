use crate::RuntimeCall;
use frame_support::traits::InstanceFilter;
use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
pub use sp_runtime::RuntimeDebug;

/// The type used to represent the kinds of proxying allowed.
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Encode,
    Decode,
    RuntimeDebug,
    MaxEncodedLen,
    scale_info::TypeInfo,
)]
pub enum ProxyType {
    Any,
}
impl Default for ProxyType {
    fn default() -> Self {
        Self::Any
    }
}

impl InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, _c: &RuntimeCall) -> bool {
        match self {
            ProxyType::Any => true,
        }
    }
    fn is_superset(&self, o: &Self) -> bool {
        self == &ProxyType::Any || self == o
    }
}
