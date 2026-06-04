use core::marker::PhantomData;
use frame_support::{dispatch::DispatchResult, storage::PrefixIterator};
use frame_system::pallet_prelude::BlockNumberFor;
use orml_traits::MultiCurrency;
use pallet_pm_market_commons::MarketCommonsPalletApi as PredictorMarketCommonsPalletApi;
use parity_scale_codec::{Decode, Encode};
use prediction_market_primitives::{
    hybrid_router_api_types::{
        AmmSoftFail as PredictorAmmSoftFail, AmmTrade as PredictorAmmTrade,
        ApiError as PredictorApiError, OrderbookSoftFail as PredictorOrderbookSoftFail,
        OrderbookTrade as PredictorOrderbookTrade,
    },
    orderbook::Order as PredictorOrder,
    traits::{
        HybridRouterAmmApi as PredictorHybridRouterAmmApi,
        HybridRouterOrderbookApi as PredictorHybridRouterOrderbookApi,
    },
    types::{Asset as PredictorAsset, ScalarPosition as PredictorScalarPosition},
};
use sp_runtime::DispatchError;
use zeitgeist_market_commons::MarketCommonsPalletApi as ZeitgeistMarketCommonsPalletApi;
use zeitgeist_primitives::{
    hybrid_router_api_types::{
        AmmSoftFail as ZeitgeistAmmSoftFail, AmmTrade as ZeitgeistAmmTrade,
        ApiError as ZeitgeistApiError, OrderbookSoftFail as ZeitgeistOrderbookSoftFail,
        OrderbookTrade as ZeitgeistOrderbookTrade,
    },
    orderbook::Order as ZeitgeistOrder,
    traits::{
        HybridRouterAmmApi as ZeitgeistHybridRouterAmmApi,
        HybridRouterOrderbookApi as ZeitgeistHybridRouterOrderbookApi,
        MarketBuilderTrait as ZeitgeistMarketBuilderTrait, MarketOf as ZeitgeistMarketOf,
    },
    types::{Asset as ZeitgeistAsset, PoolId, ScalarPosition as ZeitgeistScalarPosition},
};

type PredictorMarketOf<T> =
    prediction_market_primitives::traits::MarketOf<<T as crate::Config>::MarketCommons>;
type BalanceOf<T> = <<T as crate::Config>::AssetManager as MultiCurrency<
    <T as frame_system::Config>::AccountId,
>>::Balance;
type MarketIdOf<T> =
    <<T as crate::Config>::MarketCommons as PredictorMarketCommonsPalletApi>::MarketId;
type MomentOf<T> = <<T as crate::Config>::MarketCommons as PredictorMarketCommonsPalletApi>::Moment;
type AdaptedZeitgeistMarketOf<T> = ZeitgeistMarketOf<PredictorMarketCommonsAdapter<T>>;
type ZeitgeistOrderOf<T> =
    ZeitgeistOrder<<T as frame_system::Config>::AccountId, BalanceOf<T>, MarketIdOf<T>>;
type OrderIdOf<T> = <<T as crate::Config>::Orderbook as PredictorHybridRouterOrderbookApi>::OrderId;

pub struct PredictorAssetManagerAdapter<T>(PhantomData<T>);
pub struct PredictorMarketCommonsAdapter<T>(PhantomData<T>);
pub struct PredictorAmmAdapter<T>(PhantomData<T>);
pub struct PredictorOrderbookAdapter<T>(PhantomData<T>);

impl<T> ZeitgeistHybridRouterOrderbookApi for PredictorOrderbookAdapter<T>
where
    T: crate::Config,
{
    type AccountId = <T as frame_system::Config>::AccountId;
    type Asset = ZeitgeistAsset<MarketIdOf<T>>;
    type Balance = BalanceOf<T>;
    type MarketId = MarketIdOf<T>;
    type Order = ZeitgeistOrderOf<T>;
    type OrderId = OrderIdOf<T>;

    fn order(order_id: Self::OrderId) -> Result<Self::Order, DispatchError> {
        <T as crate::Config>::Orderbook::order(order_id).map(to_zeitgeist_order)
    }

    fn fill_order(
        who: Self::AccountId,
        order_id: Self::OrderId,
        maker_partial_fill: Option<Self::Balance>,
    ) -> Result<
        ZeitgeistOrderbookTrade<Self::AccountId, Self::Balance>,
        ZeitgeistApiError<ZeitgeistOrderbookSoftFail>,
    > {
        <T as crate::Config>::Orderbook::fill_order(who, order_id, maker_partial_fill)
            .map(to_zeitgeist_orderbook_trade)
            .map_err(to_zeitgeist_orderbook_error)
    }

    fn place_order(
        who: Self::AccountId,
        market_id: Self::MarketId,
        maker_asset: Self::Asset,
        maker_amount: Self::Balance,
        taker_asset: Self::Asset,
        taker_amount: Self::Balance,
    ) -> Result<(), ZeitgeistApiError<ZeitgeistOrderbookSoftFail>> {
        <T as crate::Config>::Orderbook::place_order(
            who,
            market_id,
            to_predictor_asset(maker_asset),
            maker_amount,
            to_predictor_asset(taker_asset),
            taker_amount,
        )
        .map_err(to_zeitgeist_orderbook_error)
    }
}

impl<T> ZeitgeistHybridRouterAmmApi for PredictorAmmAdapter<T>
where
    T: crate::Config,
{
    type AccountId = <T as frame_system::Config>::AccountId;
    type Asset = ZeitgeistAsset<MarketIdOf<T>>;
    type Balance = BalanceOf<T>;
    type MarketId = MarketIdOf<T>;

    fn pool_exists(market_id: Self::MarketId) -> bool {
        <T as crate::Config>::Amm::pool_exists(market_id)
    }

    fn get_spot_price(
        market_id: Self::MarketId,
        asset: Self::Asset,
    ) -> Result<Self::Balance, DispatchError> {
        <T as crate::Config>::Amm::get_spot_price(market_id, to_predictor_asset(asset))
    }

    fn calculate_buy_amount_until(
        market_id: Self::MarketId,
        asset: Self::Asset,
        until: Self::Balance,
    ) -> Result<Self::Balance, DispatchError> {
        <T as crate::Config>::Amm::calculate_buy_amount_until(
            market_id,
            to_predictor_asset(asset),
            until,
        )
    }

    fn buy(
        who: Self::AccountId,
        market_id: Self::MarketId,
        asset_out: Self::Asset,
        amount_in: Self::Balance,
        min_amount_out: Self::Balance,
    ) -> Result<ZeitgeistAmmTrade<Self::Balance>, ZeitgeistApiError<ZeitgeistAmmSoftFail>> {
        <T as crate::Config>::Amm::buy(
            who,
            market_id,
            to_predictor_asset(asset_out),
            amount_in,
            min_amount_out,
        )
        .map(to_zeitgeist_amm_trade)
        .map_err(to_zeitgeist_amm_error)
    }

    fn calculate_sell_amount_until(
        market_id: Self::MarketId,
        asset: Self::Asset,
        until: Self::Balance,
    ) -> Result<Self::Balance, DispatchError> {
        <T as crate::Config>::Amm::calculate_sell_amount_until(
            market_id,
            to_predictor_asset(asset),
            until,
        )
    }

    fn sell(
        who: Self::AccountId,
        market_id: Self::MarketId,
        asset_in: Self::Asset,
        amount_in: Self::Balance,
        min_amount_out: Self::Balance,
    ) -> Result<ZeitgeistAmmTrade<Self::Balance>, ZeitgeistApiError<ZeitgeistAmmSoftFail>> {
        <T as crate::Config>::Amm::sell(
            who,
            market_id,
            to_predictor_asset(asset_in),
            amount_in,
            min_amount_out,
        )
        .map(to_zeitgeist_amm_trade)
        .map_err(to_zeitgeist_amm_error)
    }
}

impl<T> MultiCurrency<<T as frame_system::Config>::AccountId> for PredictorAssetManagerAdapter<T>
where
    T: crate::Config,
{
    type Balance = BalanceOf<T>;
    type CurrencyId = ZeitgeistAsset<MarketIdOf<T>>;

    fn minimum_balance(currency_id: Self::CurrencyId) -> Self::Balance {
        <T as crate::Config>::AssetManager::minimum_balance(to_predictor_asset(currency_id))
    }

    fn total_issuance(currency_id: Self::CurrencyId) -> Self::Balance {
        <T as crate::Config>::AssetManager::total_issuance(to_predictor_asset(currency_id))
    }

    fn total_balance(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
    ) -> Self::Balance {
        <T as crate::Config>::AssetManager::total_balance(to_predictor_asset(currency_id), who)
    }

    fn free_balance(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
    ) -> Self::Balance {
        <T as crate::Config>::AssetManager::free_balance(to_predictor_asset(currency_id), who)
    }

    fn ensure_can_withdraw(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        <T as crate::Config>::AssetManager::ensure_can_withdraw(
            to_predictor_asset(currency_id),
            who,
            amount,
        )
    }

    fn transfer(
        currency_id: Self::CurrencyId,
        from: &<T as frame_system::Config>::AccountId,
        to: &<T as frame_system::Config>::AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        <T as crate::Config>::AssetManager::transfer(
            to_predictor_asset(currency_id),
            from,
            to,
            amount,
        )
    }

    fn deposit(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        <T as crate::Config>::AssetManager::deposit(to_predictor_asset(currency_id), who, amount)
    }

    fn withdraw(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
        amount: Self::Balance,
    ) -> DispatchResult {
        <T as crate::Config>::AssetManager::withdraw(to_predictor_asset(currency_id), who, amount)
    }

    fn can_slash(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
        value: Self::Balance,
    ) -> bool {
        <T as crate::Config>::AssetManager::can_slash(to_predictor_asset(currency_id), who, value)
    }

    fn slash(
        currency_id: Self::CurrencyId,
        who: &<T as frame_system::Config>::AccountId,
        amount: Self::Balance,
    ) -> Self::Balance {
        <T as crate::Config>::AssetManager::slash(to_predictor_asset(currency_id), who, amount)
    }
}

impl<T> ZeitgeistMarketCommonsPalletApi for PredictorMarketCommonsAdapter<T>
where
    T: crate::Config,
    <T as frame_system::Config>::AccountId: Decode + Encode,
    BlockNumberFor<T>: Decode + Encode,
    BalanceOf<T>: Decode + Encode,
    MarketIdOf<T>: Decode + Encode,
    MomentOf<T>: Decode + Encode,
{
    type AccountId = <T as frame_system::Config>::AccountId;
    type Balance = BalanceOf<T>;
    type BlockNumber = BlockNumberFor<T>;
    type MarketId = MarketIdOf<T>;
    type Moment = MomentOf<T>;

    fn latest_market_id() -> Result<Self::MarketId, DispatchError> {
        <T as crate::Config>::MarketCommons::latest_market_id()
    }

    fn market_iter() -> PrefixIterator<(Self::MarketId, AdaptedZeitgeistMarketOf<T>)> {
        unimplemented!("market iteration is not supported by the hybrid-router adapter")
    }

    fn market(market_id: &Self::MarketId) -> Result<AdaptedZeitgeistMarketOf<T>, DispatchError> {
        codec_convert(<T as crate::Config>::MarketCommons::market(market_id)?)
    }

    fn mutate_market<F>(market_id: &Self::MarketId, cb: F) -> DispatchResult
    where
        F: FnOnce(&mut AdaptedZeitgeistMarketOf<T>) -> DispatchResult,
    {
        <T as crate::Config>::MarketCommons::mutate_market(market_id, |predictor_market| {
            let mut zeitgeist_market = codec_convert(predictor_market.clone())?;
            cb(&mut zeitgeist_market)?;
            *predictor_market = codec_convert(zeitgeist_market)?;
            Ok(())
        })
    }

    fn push_market(market: AdaptedZeitgeistMarketOf<T>) -> Result<Self::MarketId, DispatchError> {
        <T as crate::Config>::MarketCommons::push_market(codec_convert::<_, PredictorMarketOf<T>>(
            market,
        )?)
    }

    fn build_market<U>(
        _market_builder: U,
    ) -> Result<(Self::MarketId, AdaptedZeitgeistMarketOf<T>), DispatchError>
    where
        U: ZeitgeistMarketBuilderTrait<
            Self::AccountId,
            Self::Balance,
            Self::BlockNumber,
            Self::Moment,
            Self::MarketId,
        >,
    {
        Err(unsupported_market_commons_method())
    }

    fn remove_market(market_id: &Self::MarketId) -> DispatchResult {
        <T as crate::Config>::MarketCommons::remove_market(market_id)
    }

    fn insert_market_pool(market_id: Self::MarketId, pool_id: PoolId) -> DispatchResult {
        <T as crate::Config>::MarketCommons::insert_market_pool(market_id, pool_id)
    }

    fn remove_market_pool(market_id: &Self::MarketId) -> DispatchResult {
        <T as crate::Config>::MarketCommons::remove_market_pool(market_id)
    }

    fn market_pool(market_id: &Self::MarketId) -> Result<PoolId, DispatchError> {
        <T as crate::Config>::MarketCommons::market_pool(market_id)
    }

    fn now() -> Self::Moment {
        <T as crate::Config>::MarketCommons::now()
    }
}

pub(crate) fn codec_convert<From, To>(value: From) -> Result<To, DispatchError>
where
    From: Encode,
    To: Decode,
{
    To::decode(&mut &value.encode()[..])
        .map_err(|_| DispatchError::Other("hybrid-router adapter codec conversion failed"))
}

fn unsupported_market_commons_method() -> DispatchError {
    DispatchError::Other("unsupported hybrid-router market commons adapter method")
}

fn to_zeitgeist_amm_trade<Balance>(
    trade: PredictorAmmTrade<Balance>,
) -> ZeitgeistAmmTrade<Balance> {
    ZeitgeistAmmTrade {
        amount_in: trade.amount_in,
        amount_out: trade.amount_out,
        swap_fee_amount: trade.swap_fee_amount,
        external_fee_amount: trade.external_fee_amount,
    }
}

fn to_zeitgeist_amm_error(
    error: PredictorApiError<PredictorAmmSoftFail>,
) -> ZeitgeistApiError<ZeitgeistAmmSoftFail> {
    match error {
        PredictorApiError::SoftFailure(PredictorAmmSoftFail::Numerical) =>
            ZeitgeistApiError::SoftFailure(ZeitgeistAmmSoftFail::Numerical),
        PredictorApiError::HardFailure(error) => ZeitgeistApiError::HardFailure(error),
    }
}

fn to_zeitgeist_order<AccountId, Balance, MarketId>(
    order: PredictorOrder<AccountId, Balance, MarketId>,
) -> ZeitgeistOrder<AccountId, Balance, MarketId>
where
    MarketId: parity_scale_codec::MaxEncodedLen + parity_scale_codec::HasCompact,
{
    ZeitgeistOrder {
        market_id: order.market_id,
        maker: order.maker,
        maker_asset: to_zeitgeist_asset(order.maker_asset),
        maker_amount: order.maker_amount,
        taker_asset: to_zeitgeist_asset(order.taker_asset),
        taker_amount: order.taker_amount,
    }
}

fn to_zeitgeist_orderbook_trade<AccountId, Balance>(
    trade: PredictorOrderbookTrade<AccountId, Balance>,
) -> ZeitgeistOrderbookTrade<AccountId, Balance> {
    ZeitgeistOrderbookTrade {
        filled_maker_amount: trade.filled_maker_amount,
        filled_taker_amount: trade.filled_taker_amount,
        external_fee: zeitgeist_primitives::hybrid_router_api_types::ExternalFee {
            account: trade.external_fee.account,
            amount: trade.external_fee.amount,
        },
    }
}

fn to_zeitgeist_orderbook_error(
    error: PredictorApiError<PredictorOrderbookSoftFail>,
) -> ZeitgeistApiError<ZeitgeistOrderbookSoftFail> {
    match error {
        PredictorApiError::SoftFailure(PredictorOrderbookSoftFail::BelowMinimumBalance) =>
            ZeitgeistApiError::SoftFailure(ZeitgeistOrderbookSoftFail::BelowMinimumBalance),
        PredictorApiError::SoftFailure(
            PredictorOrderbookSoftFail::PartialFillNearFullFillNotAllowed,
        ) => ZeitgeistApiError::SoftFailure(
            ZeitgeistOrderbookSoftFail::PartialFillNearFullFillNotAllowed,
        ),
        PredictorApiError::HardFailure(error) => ZeitgeistApiError::HardFailure(error),
    }
}

pub(crate) fn to_zeitgeist_asset<MarketId>(
    asset: PredictorAsset<MarketId>,
) -> ZeitgeistAsset<MarketId> {
    match asset {
        PredictorAsset::CategoricalOutcome(market_id, category) =>
            ZeitgeistAsset::CategoricalOutcome(market_id, category),
        PredictorAsset::ScalarOutcome(market_id, position) =>
            ZeitgeistAsset::ScalarOutcome(market_id, to_zeitgeist_scalar_position(position)),
        PredictorAsset::CombinatorialOutcomeLegacy => ZeitgeistAsset::CombinatorialOutcomeLegacy,
        PredictorAsset::PoolShare(pool_id) => ZeitgeistAsset::PoolShare(pool_id),
        PredictorAsset::Prd => ZeitgeistAsset::Prd,
        PredictorAsset::ForeignAsset(asset_id) => ZeitgeistAsset::ForeignAsset(asset_id),
        PredictorAsset::ParimutuelShare(market_id, category) =>
            ZeitgeistAsset::ParimutuelShare(market_id, category),
        PredictorAsset::CombinatorialToken(combinatorial_id) =>
            ZeitgeistAsset::CombinatorialToken(combinatorial_id),
    }
}

pub(crate) fn to_predictor_asset<MarketId>(
    asset: ZeitgeistAsset<MarketId>,
) -> PredictorAsset<MarketId> {
    match asset {
        ZeitgeistAsset::CategoricalOutcome(market_id, category) =>
            PredictorAsset::CategoricalOutcome(market_id, category),
        ZeitgeistAsset::ScalarOutcome(market_id, position) =>
            PredictorAsset::ScalarOutcome(market_id, to_predictor_scalar_position(position)),
        ZeitgeistAsset::CombinatorialOutcomeLegacy => PredictorAsset::CombinatorialOutcomeLegacy,
        ZeitgeistAsset::PoolShare(pool_id) => PredictorAsset::PoolShare(pool_id),
        ZeitgeistAsset::Prd => PredictorAsset::Prd,
        ZeitgeistAsset::ForeignAsset(asset_id) => PredictorAsset::ForeignAsset(asset_id),
        ZeitgeistAsset::ParimutuelShare(market_id, category) =>
            PredictorAsset::ParimutuelShare(market_id, category),
        ZeitgeistAsset::CombinatorialToken(combinatorial_id) =>
            PredictorAsset::CombinatorialToken(combinatorial_id),
    }
}

fn to_zeitgeist_scalar_position(position: PredictorScalarPosition) -> ZeitgeistScalarPosition {
    match position {
        PredictorScalarPosition::Long => ZeitgeistScalarPosition::Long,
        PredictorScalarPosition::Short => ZeitgeistScalarPosition::Short,
    }
}

fn to_predictor_scalar_position(position: ZeitgeistScalarPosition) -> PredictorScalarPosition {
    match position {
        ZeitgeistScalarPosition::Long => PredictorScalarPosition::Long,
        ZeitgeistScalarPosition::Short => PredictorScalarPosition::Short,
    }
}
