// This is free and unencumbered software released into the public domain.
//
// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.
//
// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.
//
// For more information, please refer to <http://unlicense.org>

frame_benchmarking::define_benchmarks!(
    [frame_benchmarking, BaselineBench::<Runtime>]
    [frame_system, SystemBench::<Runtime>]
    [pallet_balances, Balances]
    [pallet_grandpa, Grandpa]
    [pallet_im_online, ImOnline]
    [pallet_multisig, Multisig]
    [pallet_preimage, Preimage]
    [pallet_proxy, Proxy]
    [pallet_scheduler, Scheduler]
    [pallet_sudo, Sudo]
    [pallet_timestamp, Timestamp]
    [pallet_utility, Utility]
    [pallet_collective, AdvisoryCommittee]
    // AvN pallets
    [pallet_authors_manager, AuthorsManager]
    [pallet_avn, Avn]
    [pallet_avn_proxy, AvnProxy]
    [pallet_summary, Summary]
    [pallet_token_manager, TokenManager]
    // Prediction-market customisations
    [pallet_config, PalletConfig]
    [pallet_pm_authorized, Authorized]
    [pallet_pm_combinatorial_tokens, CombinatorialTokens]
    [pallet_pm_court, Court]
    [pallet_pm_global_disputes, GlobalDisputes]
    [pallet_pm_hybrid_router, HybridRouter]
    [pallet_pm_signed_hybrid_router, SignedHybridRouter]
    [pallet_pm_neo_swaps, NeoSwaps]
    [pallet_pm_order_book, Orderbook]
    [pallet_prediction_markets, PredictionMarkets]
    // NOTE: `orml_tokens` and `pallet_pm_eth_asset_registry` are excluded
    // from `define_benchmarks!` because they don't expose the standard
    // `frame_benchmarking::Benchmarking` impl. ORML uses its own
    // `orml_benchmarking::define_benchmarks!` macro; eth-asset-registry
    // currently ships only hand-written reference weights. Add them once
    // their benchmark code is wired up.
);
