// Copyright 2026 Aventus DAO Ltd

use crate::*;
use sp_runtime::{traits::CheckedDiv, ArithmeticError, SaturatedConversion};

impl<T: Config> Pallet<T> {
    // Nodes should not be able to submit over the min uptime required.
    // but we still check it here to be sure.
    pub fn calculate_node_weight(
        node_id: &NodeId<T>,
        uptime_info: UptimeInfo<BlockNumberFor<T>>,
        _node_info: &NodeInfo<T::SignerId, T::AccountId>,
        uptime_threshold: u32,
        _reward_period_end_time: Duration,
    ) -> u128 {
        let actual_uptime = uptime_info.count;
        let weight = uptime_info.weight;

        if actual_uptime > uptime_threshold.into() {
            log::warn!("⚠️ Node ({:?}) has been up for more than the expected uptime. Actual: {:?}, Expected: {:?}",
                node_id, actual_uptime, uptime_threshold);

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
    fn pay_one_node(
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
        let weight = Self::calculate_node_weight(
            &node,
            uptime_info,
            &node_info,
            pot_info.uptime_threshold,
            pot_info.reward_end_time,
        );
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
            if pot_info.funding_failed {
                // The rollover treasury transfer for this period failed, so it
                // is recorded with `total_reward == 0` and awaits recovery via
                // `top_up_reward_pot`. While the period is still within the
                // bounded recovery window, leave the snapshot in place and do
                // NOT advance the cursor past it as if paid - otherwise the
                // documented recovery would be impossible. Stop the drain here
                // (rather than spinning on an unadvanceable period); it resumes
                // automatically once a top-up funds the period.
                //
                // Once the period's age exceeds the window, abandon it: a
                // recovery may never arrive, and an indefinite head-of-line
                // block would freeze the entire payout stream behind one
                // unfunded period. Nothing was ever funded for it
                // (`total_reward == 0`), so there is nothing to reclaim - just
                // complete it so the cursor advances and later periods pay out.
                let age = current.saturating_sub(period);
                if age <= T::MaxFailedFundingRecoveryPeriods::get() {
                    break
                }
                Self::complete_reward_payout(period);
                used = used.saturating_add(per_iter);
                continue
            }
            if pot_info.total_reward.is_zero() {
                // Legitimately zero-reward period (funded successfully with a
                // zero amount): nothing to distribute and nothing to reclaim.
                Self::complete_reward_payout(period);
                used = used.saturating_add(per_iter);
                continue
            }
            let total_uptime = TotalUptime::<T>::get(period);
            if total_uptime.total_weight == 0u128 {
                // No reportable uptime this period - nothing to distribute. The
                // pot was funded for this period at rollover, so reclaim those
                // funds back to the treasury instead of stranding them in the
                // pot, then advance.
                Self::reclaim_undistributed_reward(period, pot_info.total_reward);
                Self::complete_reward_payout(period);
                used = used.saturating_add(per_iter);
                continue
            }

            // Build an iterator that starts where we left off in this period
            // (or from the beginning if no pointer set).
            let iter_result = match LastPaidPointer::<T>::get() {
                Some(ptr) => Self::get_iterator_from_last_paid(period, ptr),
                None => Ok(NodeUptime::<T>::iter_prefix(period)),
            };
            let mut iter = match iter_result {
                Ok(it) => it,
                Err(_) => {
                    // Defensive: a pointer mismatch shouldn't happen but if it
                    // does, advance and don't get stuck.
                    Self::complete_reward_payout(period);
                    used = used.saturating_add(per_iter);
                    continue
                },
            };

            // Per-node loop, bounded by remaining weight and batch cap.
            let mut paid_nodes: Vec<T::AccountId> = Vec::new();
            let mut last_paid: Option<T::AccountId> = None;
            let mut iterator_exhausted = false;
            loop {
                if remaining_weight.saturating_sub(used).any_lt(per_iter) {
                    break
                }
                if paid_this_block >= max_batch {
                    break
                }
                let next = iter.next();
                let (node, uptime_info) = match next {
                    Some(x) => x,
                    None => {
                        iterator_exhausted = true;
                        break
                    },
                };
                // Pay (or soft-fail with ErrorPayingReward event).
                let _ = Self::pay_one_node(
                    period,
                    &pot_info,
                    &total_uptime.total_weight,
                    node.clone(),
                    uptime_info,
                );
                paid_nodes.push(node.clone());
                last_paid = Some(node);
                paid_this_block = paid_this_block.saturating_add(1);
                used = used.saturating_add(per_iter);
            }

            // Reconcile per-period bookkeeping for this iteration:
            //  - drop the NodeUptime entries we just paid, so they're not rescanned (and so the
            //    pointer's "node not in storage" invariant holds on the next block).
            //  - either advance the pointer (drain not yet done) or call complete_reward_payout
            //    (period drained).
            Self::remove_paid_nodes(period, &paid_nodes);
            if iterator_exhausted {
                Self::complete_reward_payout(period);
                // Loop continues: try the next unpaid period if any weight left.
            } else {
                Self::update_last_paid_pointer(period, last_paid);
                // Out of weight or batch - exit the outer loop too.
                break
            }
        }

        used
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

    /// Return a non-distributable period's funded reward from the pot to the
    /// treasury source. Called when the drain skips a period that was funded at
    /// rollover but has no reportable uptime, so the funds are recycled instead
    /// of being orphaned in the pot. Best-effort: if the transfer fails the
    /// funds remain in the pot and `OutstandingRewardToPay` is still cleared by
    /// `complete_reward_payout`, leaving the pot's surplus reclaimable by a
    /// later top-up/admin action rather than blocking the drain.
    pub fn reclaim_undistributed_reward(period_index: RewardPeriodIndex, amount: BalanceOf<T>) {
        if amount.is_zero() {
            return
        }
        let pot = Self::compute_reward_account_id();
        let treasury = T::TreasurySource::get();
        // `AllowDeath`: in the earliest periods the reclaimed amount can be the
        // pot's only balance, so a `KeepAlive` transfer would fail the `>= ED`
        // check and strand the funds. The pot's genesis provider reference keeps
        // the account from being reaped, and it is re-funded at the next
        // rollover, so allowing the balance to reach zero here is safe.
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

    /// Apply any pending halvings to `NextRewardAmountPerPeriod`. Idempotent
    /// within a block: the operation is counter-based, comparing the number
    /// of halvings the current block-height implies against the running
    /// `RewardAmountHalvingsApplied`. If the chain has skipped past several
    /// halving boundaries between calls (extended downtime, manual replay)
    /// the catch-up applies in a single tick.
    pub fn apply_halving_if_due(n: BlockNumberFor<T>) {
        if !HalvingEnabled::<T>::get() {
            return
        }
        let interval = T::HalvingInterval::get();
        if interval.is_zero() {
            return
        }

        let n_u128: u128 = n.saturated_into();
        let interval_u128: u128 = interval.saturated_into();
        let expected = (n_u128 / interval_u128).min(u32::MAX as u128) as u32;
        let applied = RewardAmountHalvingsApplied::<T>::get();
        if expected <= applied {
            return
        }
        let pending = expected - applied;

        let two = BalanceOf::<T>::from(2u32);
        NextRewardAmountPerPeriod::<T>::mutate(|amt| {
            for _ in 0..pending {
                // Floor at one base unit: the reward must asymptotically
                // approach zero without ever reaching it (Truth paper /
                // Andrey's halving directive). A zero amount stays zero -
                // the floor only protects a non-zero reward from vanishing.
                let halved = amt.checked_div(&two).unwrap_or_else(BalanceOf::<T>::zero);
                if halved.is_zero() && !amt.is_zero() {
                    *amt = BalanceOf::<T>::from(1u32);
                    break
                }
                *amt = halved;
            }
        });
        RewardAmountHalvingsApplied::<T>::put(expected);

        let new_amount = NextRewardAmountPerPeriod::<T>::get();
        let period_index = RewardPeriod::<T>::get().current;
        Self::deposit_event(Event::RewardHalvingApplied {
            period_index,
            new_amount,
            total_halvings: expected,
        });
    }
}
