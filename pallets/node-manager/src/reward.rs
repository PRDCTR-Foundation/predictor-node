// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Modified for PRDCTR on 2026-07-07.

use crate::*;
use frame_support::weights::WeightMeter;
use sp_runtime::{ArithmeticError, SaturatedConversion};

impl<T: Config> Pallet<T> {
    // Nodes should not be able to submit over the min uptime required.
    // but we still check it here to be sure.
    pub fn calculate_node_weight(
        node_id: &NodeId<T>,
        uptime_info: UptimeInfo<BlockNumberFor<T>>,
        _node_info: &NodeInfo<T::SignerId, T::AccountId>,
        uptime_threshold: u32,
    ) -> u128 {
        let actual_uptime = uptime_info.count;
        let weight = uptime_info.weight;

        if actual_uptime > uptime_threshold.into() {
            log::warn!("⚠️ Node ({node_id:?}) has been up for more than the expected uptime. Actual: {actual_uptime:?}, Expected: {uptime_threshold:?}");

            // Cap at threshold. With staking removed, each heartbeat carries
            // HEARTBEAT_BASE_WEIGHT, so the capped contribution is exactly
            // threshold * HEARTBEAT_BASE_WEIGHT.
            HEARTBEAT_BASE_WEIGHT.saturating_mul(u128::from(uptime_threshold))
        } else {
            weight
        }
    }

    pub fn calculate_reward(
        weight: u128,
        total_weight: &u128,
        total_reward: &BalanceOf<T>,
    ) -> Result<(BalanceOf<T>, Perquintill), DispatchError> {
        if total_weight.is_zero() {
            return Err(DispatchError::Arithmetic(ArithmeticError::DivisionByZero))
        }

        // Convert everything to u128 to satisfy Perquintill requirements.
        let ratio = Perquintill::from_rational(weight, *total_weight);
        let total_rewards_u128: u128 = (*total_reward).saturated_into();

        Ok((ratio.mul_floor(total_rewards_u128).saturated_into(), ratio))
    }

    pub fn pay_reward(
        period: &RewardPeriodIndex,
        node_id: NodeId<T>,
        node_info: &NodeInfo<T::SignerId, T::AccountId>,
        amount: BalanceOf<T>,
        _reward_percentage: Perquintill,
    ) -> DispatchResult {
        let node_owner = node_info.owner.clone();

        // While the global lock window is active - or not yet configured
        // (lock-by-default, so rewards can't escape the lock through an ops
        // mis-ordering) - rewards accrue in `LockedRewards` and the funds
        // stay in the reward pot, to be released via `withdraw_rewards`.
        // Once the window's penalty decays to zero the lock is spent and
        // payouts credit free balance directly again.
        let lock_active = match LockSchedule::<T>::get() {
            None => true,
            Some(schedule) => !schedule.is_expired(Self::time_now_sec()),
        };

        if lock_active {
            // A zero reward still emits the event for visibility, and skips
            // the storage writes (nothing to accrue).
            if !amount.is_zero() {
                LockedRewards::<T>::mutate(&node_owner, |locked| {
                    *locked = locked.saturating_add(amount)
                });
                TotalLockedRewards::<T>::mutate(|total| *total = total.saturating_add(amount));
            }

            Self::deposit_event(Event::RewardLocked {
                reward_period: *period,
                owner: node_owner,
                node: node_id,
                amount,
            });

            return Ok(())
        }

        // A zero reward still emits the event for visibility, and skips the
        // transfer (nothing to move).
        if !amount.is_zero() {
            let reward_pot = Self::compute_reward_account_id();
            T::Currency::transfer(
                &reward_pot,
                &node_owner,
                amount,
                ExistenceRequirement::AllowDeath,
            )?;
        }

        Self::deposit_event(Event::RewardPaid {
            reward_period: *period,
            owner: node_owner,
            node: node_id,
            amount,
        });

        Ok(())
    }

    /// Pay one node out of the given period. Returns the amount paid (or
    /// `Zero` on a soft-failure path that emitted `ErrorPayingReward`).
    pub(crate) fn pay_one_node(
        period: RewardPeriodIndex,
        pot_info: &RewardPotInfo<BalanceOf<T>>,
        total_weight: &u128,
        node: T::AccountId,
        uptime_info: UptimeInfo<BlockNumberFor<T>>,
    ) -> Result<BalanceOf<T>, ()> {
        let node_info = match NodeRegistry::<T>::get(&node) {
            Some(n) => n,
            None => {
                Self::deposit_event(Event::ErrorPayingReward {
                    reward_period: period,
                    node,
                    error: Error::<T>::NodeNotRegistered.into(),
                });
                return Err(())
            },
        };
        let weight =
            Self::calculate_node_weight(&node, uptime_info, &node_info, pot_info.uptime_threshold);
        let (amount, percentage) =
            match Self::calculate_reward(weight, total_weight, &pot_info.total_reward) {
                Ok(x) => x,
                Err(e) => {
                    Self::deposit_event(Event::ErrorPayingReward {
                        reward_period: period,
                        node,
                        error: e,
                    });
                    return Err(())
                },
            };

        if let Err(e) = Self::pay_reward(&period, node.clone(), &node_info, amount, percentage) {
            Self::deposit_event(Event::ErrorPayingReward { reward_period: period, node, error: e });
            return Err(())
        }
        Ok(amount)
    }

    /// Pay out finished reward periods in order, starting at `OldestUnpaidRewardPeriodIndex`.
    ///
    /// Walks `oldest..current` and stops at the first period that must wait for its amount
    /// (see `awaiting_funding`), when a period's entries do not fit in the budget, or when
    /// `meter` cannot afford one more period step plus one node. Each period is drained by
    /// `drain_period_in_batches`; its nodes are paid only when it is funded with a non-zero
    /// reward and has uptime. A funded reward with no uptime is returned to the treasury once
    /// the period completes. Returns the weight consumed.
    pub fn drain_outstanding_payouts(remaining_weight: Weight) -> Weight {
        let mut meter = WeightMeter::with_limit(remaining_weight);
        // `MaxBatchSize`, `RewardPeriod`, `OldestUnpaidRewardPeriodIndex` and the timestamp.
        if meter
            .try_consume(<T as frame_system::Config>::DbWeight::get().reads(4))
            .is_err()
        {
            return meter.consumed()
        }
        let mut nodes_left = MaxBatchSize::<T>::get();
        let current = RewardPeriod::<T>::get().current;
        let oldest = OldestUnpaidRewardPeriodIndex::<T>::get();
        let now = Self::time_now_sec();
        // Entering a period must afford its completion plus one node, so every period that is
        // entered either completes or drains at least one entry.
        let min_step = <T as Config>::WeightInfo::complete_reward_period()
            .saturating_add(<T as Config>::WeightInfo::pay_one_node());

        // `complete_reward_payout` advances `OldestUnpaidRewardPeriodIndex` by one, so the
        // walk visits periods in the same order the cursor does.
        for period in oldest..current {
            if !meter.can_consume(min_step) || nodes_left == 0 {
                break
            }

            let pot_info = RewardPot::<T>::get(period);
            if pot_info
                .as_ref()
                .is_some_and(|p| Self::awaiting_funding(p, period, current, now))
            {
                break
            }

            let total_weight = TotalUptime::<T>::get(period).total_weight;
            let reward = pot_info.filter(|p| p.funded && !p.total_reward.is_zero());
            let payout = reward.as_ref().filter(|_| total_weight != 0).map(|p| (p, total_weight));

            if !Self::drain_period_in_batches(period, payout, &mut meter, &mut nodes_left) {
                break
            }
            // Reclaim only after completion, so a period that spans several calls cannot
            // return its reward twice.
            if let (Some(p), None) = (&reward, payout) {
                Self::reclaim_undistributed_reward(period, p.total_reward);
            }
        }

        meter.consumed()
    }

    /// Whether `period` must wait before it is drained: its amount can still be set or
    /// changed, and it is not an unfunded period older than `MaxFailedFundingRecoveryPeriods`.
    fn awaiting_funding(
        pot_info: &RewardPotInfo<BalanceOf<T>>,
        period: RewardPeriodIndex,
        current: RewardPeriodIndex,
        now: Duration,
    ) -> bool {
        let abandoned = !pot_info.funded &&
            current.saturating_sub(period) > T::MaxFailedFundingRecoveryPeriods::get();
        pot_info.can_update_amount(now) && !abandoned
    }

    /// Remove up to `limit` of `period`'s `NodeUptime` rows and complete the period once it
    /// has no rows left. With `payout` set to `(pot_info, total_weight)`, each removed node is
    /// paid; with `None`, rows are only deleted and no reward event is emitted.
    ///
    /// `limit` is the smaller of `nodes_left` and the number of `WeightInfo::pay_one_node`
    /// weights (which include deleting the entry) that fit in `meter` after reserving one
    /// `WeightInfo::complete_reward_period` for the completion.
    /// Both `meter` and `nodes_left` are charged for what is spent, and `meter` never exceeds
    /// its limit. A later call continues with the rows still in storage.
    ///
    /// Returns `true` if the period was completed.
    pub(crate) fn drain_period_in_batches(
        period: RewardPeriodIndex,
        payout: Option<(&RewardPotInfo<BalanceOf<T>>, u128)>,
        meter: &mut WeightMeter,
        nodes_left: &mut u32,
    ) -> bool {
        let per_node = <T as Config>::WeightInfo::pay_one_node();
        let completion = <T as Config>::WeightInfo::complete_reward_period();
        // A zero `per_node` returns `None`: weight then places no limit on the count.
        let by_weight: u32 = meter
            .remaining()
            .saturating_sub(completion)
            .checked_div_per_component(&per_node)
            .unwrap_or(u64::MAX)
            .saturated_into();
        let limit = by_weight.min(*nodes_left);

        let mut drained: u32 = 0;
        for (node, uptime_info) in NodeUptime::<T>::drain_prefix(period).take(limit as usize) {
            if let Some((pot_info, total_weight)) = payout {
                // Failures are reported by `pay_one_node` as `ErrorPayingReward` events.
                let _ = Self::pay_one_node(period, pot_info, &total_weight, node, uptime_info);
            }
            meter.consume(per_node);
            drained = drained.saturating_add(1);
        }
        *nodes_left = nodes_left.saturating_sub(drained);

        // Fewer rows than `limit` means the period is empty. If exactly `limit` rows were
        // left, the next call finds it empty and completes it.
        let period_drained = drained < limit;
        if period_drained {
            Self::complete_reward_payout(period);
            meter.consume(completion);
        }
        period_drained
    }

    /// Return a funded period's reward from the pot to the treasury when the
    /// period has no reportable uptime. Best-effort: if the transfer fails the
    /// funds stay in the pot, `OutstandingRewardToPay` is still cleared by
    /// `complete_reward_payout`, and the drain is not blocked.
    pub fn reclaim_undistributed_reward(period_index: RewardPeriodIndex, amount: BalanceOf<T>) {
        if amount.is_zero() {
            return
        }
        let pot = Self::compute_reward_account_id();
        let treasury = T::TreasurySource::get();
        // `AllowDeath`: the reclaimed amount can be the pot's only balance, so `KeepAlive`
        // would fail the `>= ED` check and strand the funds. The pot's genesis provider
        // reference keeps the account from being reaped, so reaching zero is safe.
        match T::Currency::transfer(&pot, &treasury, amount, ExistenceRequirement::AllowDeath) {
            Ok(()) => {
                Self::deposit_event(Event::UndistributedRewardReclaimed {
                    reward_period: period_index,
                    amount,
                });
            },
            Err(_) => {
                Self::deposit_event(Event::UndistributedRewardReclaimFailed {
                    reward_period: period_index,
                    amount,
                });
            },
        }
    }

    pub fn complete_reward_payout(period_index: RewardPeriodIndex) {
        if let Some(reward_pot) = RewardPot::<T>::get(period_index) {
            let paid_reward = reward_pot.total_reward;
            OutstandingRewardToPay::<T>::mutate(|outstanding| {
                *outstanding = outstanding.saturating_sub(paid_reward);
            });
        }

        // We finished paying all nodes for this period
        OldestUnpaidRewardPeriodIndex::<T>::put(period_index.saturating_add(1));
        <TotalUptime<T>>::remove(period_index);
        <RewardPot<T>>::remove(period_index);

        Self::deposit_event(Event::RewardPayoutCompleted { reward_period_index: period_index });
    }

    /// The account ID of the reward pot.
    pub fn compute_reward_account_id() -> T::AccountId {
        T::RewardPotId::get().into_account_truncating()
    }

    /// The total amount of funds stored in this pallet
    pub fn reward_pot_balance() -> BalanceOf<T> {
        // Must never be less than 0 but better be safe.
        <T as pallet::Config>::Currency::free_balance(&Self::compute_reward_account_id())
            .saturating_sub(<T as pallet::Config>::Currency::minimum_balance())
    }

    /// Get the current time in seconds
    pub fn time_now_sec() -> Duration {
        T::TimeProvider::now().as_secs()
    }

    /// Credit `node` with a full reward period's uptime for the *current*
    /// period, as if it had already reported `uptime_threshold` heartbeats -
    /// the number required to earn a full share of the period's reward.
    /// Used during a reserved node migration.
    pub(crate) fn credit_full_period_uptime(node: &NodeId<T>) -> u32 {
        let reward_period = RewardPeriod::<T>::get();
        let threshold = reward_period.uptime_threshold;
        if threshold.is_zero() {
            return 0
        }

        let now = frame_system::Pallet::<T>::block_number();
        let weight = HEARTBEAT_BASE_WEIGHT.saturating_mul(u128::from(threshold));

        NodeUptime::<T>::insert(
            reward_period.current,
            node,
            UptimeInfo::new(threshold.into(), weight, now),
        );
        TotalUptime::<T>::mutate(reward_period.current, |total| {
            total.total_heartbeats = total.total_heartbeats.saturating_add(threshold.into());
            total.total_weight = total.total_weight.saturating_add(weight);
        });

        threshold
    }

    /// Root: move `amount` from `T::TreasurySource` into the reward pot
    /// account's balance. Not tied to any period - it just makes funds
    /// available for a later `set_reward_amount` to allocate.
    pub(crate) fn do_top_up_reward_pot(amount: BalanceOf<T>) -> DispatchResult {
        ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);

        let treasury = T::TreasurySource::get();
        let pot = Self::compute_reward_account_id();
        T::Currency::transfer(&treasury, &pot, amount, ExistenceRequirement::KeepAlive)
            .map_err(|_| Error::<T>::TreasuryUnderfunded)?;

        Self::deposit_event(Event::RewardPotToppedUp { amount });
        Ok(())
    }

    /// Set `amount` (which may be zero) as `period`'s reward total. `period` must have ended
    /// and still have its `RewardPot` entry.
    ///
    /// Allowed while the period is unfunded, or funded and still inside its update window
    /// (`REWARD_UPDATE_WINDOW_SECS` after it ends). Re-setting a funded amount replaces the
    /// previous one: `OutstandingRewardToPay` is adjusted by the difference. Rewards are paid
    /// only once the window has closed.
    ///
    /// The pot must already hold `amount` on top of everything else promised
    /// (`OutstandingRewardToPay` excluding this period's current amount, and
    /// `TotalLockedRewards`). No currency moves here; fund the pot with `top_up_reward_pot`.
    pub(crate) fn do_set_reward_amount(
        period: RewardPeriodIndex,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        ensure!(amount <= T::MaxRewardPerPeriod::get(), Error::<T>::RewardExceedsMax);

        let mut pot_info = RewardPot::<T>::get(period).ok_or(Error::<T>::RewardPotNotFound)?;
        ensure!(
            pot_info.can_update_amount(Self::time_now_sec()),
            Error::<T>::RewardUpdateWindowClosed
        );

        let previous = pot_info.total_reward;
        let outstanding_without_period =
            OutstandingRewardToPay::<T>::get().saturating_sub(previous);
        let already_committed =
            outstanding_without_period.saturating_add(TotalLockedRewards::<T>::get());
        ensure!(
            Self::reward_pot_balance() >= already_committed.saturating_add(amount),
            Error::<T>::InsufficientPotBalance
        );

        pot_info.total_reward = amount;
        pot_info.funded = true;
        RewardPot::<T>::insert(period, pot_info);
        OutstandingRewardToPay::<T>::put(outstanding_without_period.saturating_add(amount));

        Self::deposit_event(Event::RewardAmountSet { period, amount });
        Ok(())
    }
}
