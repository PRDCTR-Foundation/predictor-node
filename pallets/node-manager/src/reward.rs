// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Modified for PRDCTR on 2026-07-07.

use crate::*;
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

    /// Worst-case weight charged per `on_idle` per-node iteration. Set
    /// conservatively above the sum of a NodeRegistry read, a
    /// `Currency::transfer` (reward pot -> owner, touching two accounts), the
    /// NodeUptime removal write, and the event deposit - or the equivalent
    /// locked-accrual writes when the lock window is active.
    pub fn worst_case_iteration_weight() -> Weight {
        // ref_time is denominated in picoseconds, so 200_000_000_000 is ~200 ms
        // per iteration - a generous safety margin over the measured cost on
        // similar runtimes. The block-weight cap and `MaxBatchSize` are the
        // real upper bounds; this is the granularity at which `on_idle` decides
        // whether to attempt another iteration.
        Weight::from_parts(200_000_000_000, 4096)
            .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(4))
            .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(4))
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

    /// Walk the oldest unpaid reward period (and the next one if weight is
    /// left) paying nodes one at a time. Each iteration consumes at most
    /// `worst_case_iteration_weight`; the loop terminates when the weight
    /// budget cannot cover one more iteration, `MaxBatchSize` per-block is
    /// hit, or the iterator is exhausted (in which case
    /// `complete_reward_payout` advances `OldestUnpaidRewardPeriodIndex`).
    pub fn drain_outstanding_payouts(remaining_weight: Weight) -> Weight {
        let per_iter = Self::worst_case_iteration_weight();
        let max_batch = MaxBatchSize::<T>::get();
        let mut used = Weight::zero();
        let mut paid_this_block: u32 = 0;

        loop {
            // (A) Weight check: can we afford another iteration's worst-case?
            if remaining_weight.saturating_sub(used).any_lt(per_iter) {
                break
            }
            // (B) Batch cap: prevents storage thrash regardless of weight headroom.
            if paid_this_block >= max_batch {
                break
            }

            let period = OldestUnpaidRewardPeriodIndex::<T>::get();
            let current = RewardPeriod::<T>::get().current;
            if period >= current {
                // Nothing to drain yet (the period we'd pay hasn't rolled).
                break
            }

            // Resolve the snapshot for this period. If missing, skip the period
            // cleanly via `complete_reward_payout`.
            let pot_info = match RewardPot::<T>::get(period) {
                Some(p) => p,
                None => {
                    Self::complete_reward_payout(period);
                    used = used.saturating_add(per_iter);
                    continue
                },
            };
            if !pot_info.funded {
                // No amount has been set yet, so nothing is paid and the cursor stays put; the
                // drain resumes once `set_reward_amount` funds the period. An unfunded period
                // older than `MaxFailedFundingRecoveryPeriods` is abandoned instead, so it
                // cannot block later periods forever.
                let age = current.saturating_sub(period);
                if age <= T::MaxFailedFundingRecoveryPeriods::get() {
                    break
                }
                // Abandon: clear the period's `NodeUptime` entries in weight-bounded batches
                // and complete the period once they are drained. Nothing is paid.
                match Self::drain_period_in_batches(
                    period,
                    remaining_weight,
                    per_iter,
                    max_batch,
                    &mut used,
                    &mut paid_this_block,
                    |_node, _uptime_info| {},
                ) {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(()) => {
                        Self::complete_reward_payout(period);
                        used = used.saturating_add(per_iter);
                        continue
                    },
                }
            }
            // No rewards are paid while the amount can still be updated.
            if pot_info.update_window_open(Self::time_now_sec()) {
                break
            }
            if pot_info.total_reward.is_zero() {
                // Funded with a zero amount: nothing to distribute or reclaim.
                Self::complete_reward_payout(period);
                used = used.saturating_add(per_iter);
                continue
            }
            let total_uptime = TotalUptime::<T>::get(period);
            if total_uptime.total_weight == 0u128 {
                // No reportable uptime: nothing to distribute. Return the period's
                // amount to the treasury instead of stranding it in the pot, then advance.
                Self::reclaim_undistributed_reward(period, pot_info.total_reward);
                Self::complete_reward_payout(period);
                used = used.saturating_add(per_iter);
                continue
            }

            // Pay nodes in weight/batch-bounded batches, soft-failing with an
            // `ErrorPayingReward` event per node. The shared helper handles the
            // resume pointer and completes the period only once every node is
            // drained.
            match Self::drain_period_in_batches(
                period,
                remaining_weight,
                per_iter,
                max_batch,
                &mut used,
                &mut paid_this_block,
                |node, uptime_info| {
                    let _ = Self::pay_one_node(
                        period,
                        &pot_info,
                        &total_uptime.total_weight,
                        node,
                        uptime_info,
                    );
                },
            ) {
                Ok(true) => continue,
                Ok(false) => break,
                Err(()) => {
                    // Defensive: a pointer mismatch shouldn't happen but if it
                    // does, advance and don't get stuck.
                    Self::complete_reward_payout(period);
                    used = used.saturating_add(per_iter);
                    continue
                },
            }
        }

        used
    }

    /// Drain a period's `NodeUptime` entries in weight/batch-bounded batches,
    /// invoking `on_node` for each entry before removing it. Resumes from the
    /// `LastPaidPointer` if one is set, otherwise from the start of the period.
    /// Advances the cursor via `complete_reward_payout` only once every entry
    /// has been drained (across as many `on_idle` calls as needed); otherwise
    /// records a fresh `LastPaidPointer` so the next call resumes where this one
    /// stopped. Shared by the normal pay path and the unfunded-period abandonment
    /// path so both complete a period only once it is empty.
    /// Returns `Ok(true)` when the period is fully drained, `Ok(false)` when
    /// stopped early on the weight/batch budget, and `Err(())` on a
    /// pointer-resolution failure (the caller should complete the period).
    fn drain_period_in_batches(
        period: RewardPeriodIndex,
        remaining_weight: Weight,
        per_iter: Weight,
        max_batch: u32,
        used: &mut Weight,
        paid_this_block: &mut u32,
        mut on_node: impl FnMut(T::AccountId, UptimeInfo<BlockNumberFor<T>>),
    ) -> Result<bool, ()> {
        let iter_result = match LastPaidPointer::<T>::get() {
            Some(ptr) => Self::get_iterator_from_last_paid(period, ptr),
            None => Ok(NodeUptime::<T>::iter_prefix(period)),
        };
        let mut iter = iter_result.map_err(|_| ())?;

        // Track the drained nodes so we can drop their NodeUptime entries after
        // iterating (mutating the map mid-iteration is unsafe), keeping the
        // pointer's "node not in storage" invariant on the next block.
        let mut drained_nodes: Vec<T::AccountId> = Vec::new();
        let mut last_node: Option<T::AccountId> = None;
        let mut iterator_exhausted = false;
        loop {
            if remaining_weight.saturating_sub(*used).any_lt(per_iter) {
                break
            }
            if *paid_this_block >= max_batch {
                break
            }
            let (node, uptime_info) = match iter.next() {
                Some(x) => x,
                None => {
                    iterator_exhausted = true;
                    break
                },
            };
            on_node(node.clone(), uptime_info);
            drained_nodes.push(node.clone());
            last_node = Some(node);
            *paid_this_block = paid_this_block.saturating_add(1);
            *used = used.saturating_add(per_iter);
        }

        Self::remove_paid_nodes(period, &drained_nodes);
        if iterator_exhausted {
            // Completing a period with no drained entries (e.g. an empty abandoned
            // period) still does storage work but charged no `per_iter` above; charge
            // one so the outer loop's guards bound how many empty periods complete per
            // block. The outer loop only enters here with at least one `per_iter` of
            // budget left, so progress is still guaranteed.
            if drained_nodes.is_empty() {
                *used = used.saturating_add(per_iter);
            }
            Self::complete_reward_payout(period);
        } else {
            Self::update_last_paid_pointer(period, last_node);
        }
        Ok(iterator_exhausted)
    }

    pub fn remove_paid_nodes(
        period_index: RewardPeriodIndex,
        paid_nodes_to_remove: &Vec<T::AccountId>,
    ) {
        // Remove the paid nodes. We do this separately to avoid changing the map while iterating
        // it
        for node in paid_nodes_to_remove {
            NodeUptime::<T>::remove(period_index, node);
        }
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
        LastPaidPointer::<T>::kill();
        <TotalUptime<T>>::remove(period_index);
        <RewardPot<T>>::remove(period_index);

        Self::deposit_event(Event::RewardPayoutCompleted { reward_period_index: period_index });
    }

    pub fn update_last_paid_pointer(
        period_index: RewardPeriodIndex,
        last_node_paid: Option<T::AccountId>,
    ) {
        if let Some(node) = last_node_paid {
            LastPaidPointer::<T>::put(PaymentPointer { period_index, node });
        }
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

    #[allow(clippy::type_complexity)]
    pub fn get_iterator_from_last_paid(
        oldest_period: RewardPeriodIndex,
        last_paid_pointer: PaymentPointer<T::AccountId>,
    ) -> Result<PrefixIterator<(T::AccountId, UptimeInfo<BlockNumberFor<T>>)>, DispatchError> {
        ensure!(last_paid_pointer.period_index == oldest_period, Error::<T>::InvalidPeriodPointer);
        // Make sure the last paid node has been remove, to be extra sure we won't double pay
        ensure!(
            !NodeUptime::<T>::contains_key(oldest_period, &last_paid_pointer.node),
            Error::<T>::InvalidNodePointer
        );

        // Start iteration just after `(oldest_period, last_paid_pointer.node)`.
        let final_key = last_paid_pointer.get_final_key::<T>();
        Ok(NodeUptime::<T>::iter_prefix_from(oldest_period, final_key))
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
