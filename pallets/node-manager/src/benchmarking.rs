// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Modified for PRDCTR on 2026-08-27.

//! # Node manager benchmarks

use super::*;
use frame_benchmarking::v2::*;
use frame_system::{EventRecord, RawOrigin};

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1_usize)];
    assert_eq!(event, &system_event);
}

fn set_registrar<T: Config>(registrar: T::AccountId) {
    <NodeRegistrar<T>>::set(Some(registrar.clone()));
}

fn register_new_node<T: Config>(node: NodeId<T>, owner: T::AccountId) -> T::SignerId {
    let key = T::SignerId::generate_pair(None);
    <NodeRegistry<T>>::insert(node.clone(), NodeInfo::new(owner.clone(), key.clone(), 0u32));
    <OwnedNodes<T>>::insert(owner.clone(), node, ());
    <OwnedNodesCount<T>>::mutate(owner, |count| *count += 1);

    key
}

fn create_heartbeat<T: Config>(node: NodeId<T>, reward_period_index: RewardPeriodIndex) {
    let uptime = 1u64;
    let weight = HEARTBEAT_BASE_WEIGHT.saturating_mul(uptime.into());

    <NodeUptime<T>>::mutate(reward_period_index, &node, |maybe_info| {
        if let Some(info) = maybe_info.as_mut() {
            info.count = info.count.saturating_add(uptime);
            info.last_reported = frame_system::Pallet::<T>::block_number();
            info.weight = info.weight.saturating_add(weight);
        } else {
            *maybe_info = Some(UptimeInfo {
                count: 1,
                last_reported: frame_system::Pallet::<T>::block_number(),
                weight,
            });
        }
    });

    <TotalUptime<T>>::mutate(reward_period_index, |total| {
        total.total_heartbeats = total.total_heartbeats.saturating_add(1u64);
        total.total_weight = total.total_weight.saturating_add(weight);
    });
}

fn fund_reward_pot<T: Config>() {
    let reward_amount: BalanceOf<T> = 2_000_000_000u32.into();
    let reward_pot_address = Pallet::<T>::compute_reward_account_id();
    T::Currency::make_free_balance_be(&reward_pot_address, reward_amount);
}

fn create_nodes_and_heartbeat<T: Config>(
    owner: T::AccountId,
    reward_period_index: RewardPeriodIndex,
    node_to_create: u32,
) -> Vec<NodeId<T>> {
    let mut registered_nodes = vec![];
    for i in 1..=node_to_create {
        let node: NodeId<T> = account("node", i, i);
        let _ = register_new_node::<T>(node.clone(), owner.clone());
        create_heartbeat::<T>(node.clone(), reward_period_index);
        registered_nodes.push(node);
    }
    registered_nodes
}

fn enable_rewards<T>()
where
    T: Config + pallet_timestamp::Config<Moment = u64>,
{
    <RewardEnabled<T>>::set(true);
    pallet_timestamp::Pallet::<T>::set_timestamp(10 * 12_000);
}

#[benchmarks(where T: pallet_timestamp::Config<Moment = u64>)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn register_node() {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);

        #[extrinsic_call]
        register_node(
            RawOrigin::Signed(registrar.clone()),
            node.clone(),
            owner.clone(),
            signing_key.clone(),
        );

        let _node_info = <NodeRegistry<T>>::get(&node).expect("Node must be registered");
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), node.clone()));
        assert_last_event::<T>(Event::NodeRegistered { owner, node }.into());
    }

    #[benchmark]
    fn register_reserved_node() {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);
        <ReservedNodes<T>>::insert(
            &node,
            ReservedNodeInfo::new(owner.clone(), signing_key.clone()),
        );
        <TotalReservedNodes<T>>::put(1u32);

        #[extrinsic_call]
        register_node(
            RawOrigin::Signed(registrar.clone()),
            node.clone(),
            owner.clone(),
            signing_key.clone(),
        );

        assert!(!<ReservedNodes<T>>::contains_key(&node));
        assert!(<TotalReservedNodes<T>>::get().is_zero());
        let reward_period = <RewardPeriod<T>>::get();
        assert!(<NodeUptime<T>>::contains_key(reward_period.current, &node));
    }

    #[benchmark]
    fn set_admin_config_registrar() {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());
        let new_registrar: T::AccountId = account("new_registrar", 0, 0);
        let config = AdminConfig::NodeRegistrar(new_registrar.clone());

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<NodeRegistrar<T>>::get() == Some(new_registrar));
    }

    #[benchmark]
    fn set_admin_config_reward_period() {
        let current_reward_period = <NextRewardPeriodLength<T>>::get();
        let new_reward_period = current_reward_period + 1u32;
        let config = AdminConfig::NextRewardPeriodLength(new_reward_period);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<NextRewardPeriodLength<T>>::get() == new_reward_period);
    }

    #[benchmark]
    fn set_admin_config_reward_batch_size() {
        let current_batch_size = <MaxBatchSize<T>>::get();
        let new_batch_size = current_batch_size + 1u32;
        let config = AdminConfig::BatchSize(new_batch_size);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<MaxBatchSize<T>>::get() == new_batch_size);
    }

    #[benchmark]
    fn set_admin_config_reward_heartbeat() {
        let current_heartbeat = <NextHeartbeatPeriod<T>>::get();
        let new_heartbeat = current_heartbeat + 1u32;
        let config = AdminConfig::NextHeartbeatPeriod(new_heartbeat);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<NextHeartbeatPeriod<T>>::get() == new_heartbeat);
    }

    // Worst case: a period awaiting its amount (as `on_initialize` always
    // leaves the one it just closed) gets its amount set.
    #[benchmark]
    fn set_reward_amount() {
        let reward_period = <RewardPeriod<T>>::get();
        let period_index = reward_period.current;
        let new_amount: BalanceOf<T> = 1_000_000u32.into();
        fund_reward_pot::<T>();
        <RewardPot<T>>::insert(
            period_index,
            RewardPotInfo::<BalanceOf<T>>::new(
                BalanceOf::<T>::zero(),
                reward_period.uptime_threshold,
                Pallet::<T>::time_now_sec(),
                false,
            ),
        );

        #[extrinsic_call]
        set_reward_amount(RawOrigin::Root, period_index, new_amount);

        let pot_info = <RewardPot<T>>::get(period_index).expect("pot must exist");
        assert_eq!(pot_info.total_reward, new_amount);
        assert!(pot_info.funded);
    }

    #[benchmark]
    fn set_admin_config_reward_enabled() {
        let current_flag = <RewardEnabled<T>>::get();
        let new_flag = !current_flag;
        let config = AdminConfig::RewardEnabled(new_flag);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<RewardEnabled<T>>::get() == new_flag);
    }

    #[benchmark]
    fn set_admin_config_min_threshold() {
        let new_threshold = Perbill::from_percent(80);
        let config = AdminConfig::MinUptimeThreshold(new_threshold);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<MinUptimeThreshold<T>>::get() == new_threshold);
    }

    #[benchmark]
    fn on_initialise_with_new_reward_period() {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> =
            reward_period.first + BlockNumberFor::<T>::from(reward_period.length) + 1u32.into();
        enable_rewards::<T>();

        #[block]
        {
            Pallet::<T>::on_initialize(block_number);
        }

        let new_reward_period_index = reward_period.current + 1u64;
        let new_reward_period = <RewardPeriod<T>>::get();
        assert!(new_reward_period_index == new_reward_period.current);
        assert_last_event::<T>(
            Event::NewRewardPeriodStarted {
                reward_period_index: new_reward_period_index,
                reward_period_length: reward_period.length,
                uptime_threshold: new_reward_period.uptime_threshold,
            }
            .into(),
        );
    }

    #[benchmark]
    fn on_initialise_no_reward_period() {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> =
            BlockNumberFor::<T>::from(reward_period.length) - 1u32.into();
        enable_rewards::<T>();

        #[block]
        {
            Pallet::<T>::on_initialize(block_number);
        }

        assert!(reward_period.current == <RewardPeriod<T>>::get().current);
    }

    #[benchmark]
    fn offchain_submit_heartbeat() {
        enable_rewards::<T>();

        // update the min threshold first
        RewardPeriod::<T>::mutate(|reward_period| {
            reward_period.uptime_threshold = 10;
        });

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let node: NodeId<T> = account("node", 0, 0);
        let owner: T::AccountId = account("owner", 0, 0);
        let signing_key: T::SignerId = register_new_node::<T>(node.clone(), owner.clone());
        create_heartbeat::<T>(node.clone(), reward_period_index);

        // Move forward to the next heartbeat period
        <frame_system::Pallet<T>>::set_block_number(
            frame_system::Pallet::<T>::block_number() +
                <NextHeartbeatPeriod<T>>::get().into() +
                1u32.into(),
        );

        let heartbeat_count = 1u64;
        let signature = signing_key
            .sign(&(HEARTBEAT_CONTEXT, heartbeat_count, reward_period_index).encode())
            .expect("Error signing");

        #[extrinsic_call]
        offchain_submit_heartbeat(
            RawOrigin::None,
            node.clone(),
            reward_period_index,
            heartbeat_count,
            signature,
        );

        let uptime_info = <NodeUptime<T>>::get(reward_period_index, &node).expect("No uptime info");
        assert!(uptime_info.count == heartbeat_count + 1);
        assert_last_event::<T>(Event::HeartbeatReceived { reward_period_index, node }.into());
    }

    #[benchmark]
    fn deregister_nodes(b: Linear<1, MAX_NODES_TO_DEREGISTER>) {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);

        let nodes_to_deregister =
            create_nodes_and_heartbeat::<T>(owner.clone(), reward_period_index, b);

        // Show that the nodes are registered
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), nodes_to_deregister[0].clone()));
        assert!(<NodeRegistry<T>>::contains_key(nodes_to_deregister[0].clone()));

        #[extrinsic_call]
        deregister_nodes(
            RawOrigin::Signed(registrar.clone()),
            owner.clone(),
            BoundedVec::truncate_from(nodes_to_deregister.clone()),
        );

        for node in &nodes_to_deregister {
            assert!(!<OwnedNodes<T>>::contains_key(owner.clone(), node));
            assert!(!<NodeRegistry<T>>::contains_key(node));
            assert!(!<NodeUptime<T>>::contains_key(reward_period_index, node));
        }
        // `create_nodes_and_heartbeat` gives every node exactly one heartbeat,
        // so the whole period's uptime is discarded with the batch.
        assert_eq!(<TotalUptime<T>>::get(reward_period_index).total_weight, 0);
        assert_last_event::<T>(
            Event::NodeUptimeDiscarded {
                reward_period_index,
                owner,
                heartbeats: b as u64,
                weight: HEARTBEAT_BASE_WEIGHT.saturating_mul(b as u128),
            }
            .into(),
        );
    }

    #[benchmark]
    fn update_signing_key() {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let _current_signing_key: T::SignerId = register_new_node::<T>(node.clone(), owner.clone());
        let new_signing_key: T::SignerId = account("new_signing_key", 3, 3);

        #[extrinsic_call]
        update_signing_key(RawOrigin::Signed(owner.clone()), node.clone(), new_signing_key.clone());

        let node_info = <NodeRegistry<T>>::get(&node).expect("Node must be registered");
        assert!(node_info.signing_key == new_signing_key);
        assert_last_event::<T>(Event::SigningKeyUpdated { owner, node }.into());
    }

    // Worst-case cost of paying one node in the `on_idle` drain: owner lookup,
    // reward transfer from the pot, and the `RewardPaid` event.
    #[benchmark]
    fn pay_one_node() {
        enable_rewards::<T>();
        fund_reward_pot::<T>();
        // Expired lock window (zero penalty) so the pay path takes the direct
        // transfer branch and credits the owner's free balance, exercising the
        // heavier of the two per-node payout paths.
        <LockSchedule<T>>::put(LockScheduleInfo::new(0u64, 0u32));

        let reward_period = <RewardPeriod<T>>::get();
        let period = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);
        let node: NodeId<T> = account("node", 1, 1);
        let _ = register_new_node::<T>(node.clone(), owner.clone());
        create_heartbeat::<T>(node.clone(), period);

        let uptime_info = <NodeUptime<T>>::get(period, &node).expect("uptime recorded");
        let total_weight = <TotalUptime<T>>::get(period).total_weight;
        let reward_amount: BalanceOf<T> = 1_000_000u32.into();
        let pot_info = RewardPotInfo::<BalanceOf<T>>::new(
            reward_amount,
            reward_period.uptime_threshold,
            Pallet::<T>::time_now_sec(),
            true,
        );

        #[block]
        {
            let _ = Pallet::<T>::pay_one_node(
                period,
                &pot_info,
                &total_weight,
                node.clone(),
                uptime_info,
            );
        }

        assert!(T::Currency::free_balance(&owner) > BalanceOf::<T>::zero());
    }

    #[benchmark]
    fn set_admin_config_lock_schedule() {
        let schedule = LockScheduleInfo::new(1_000_000u64, 52u32);
        let config = AdminConfig::LockSchedule(schedule);

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<LockSchedule<T>>::get() == Some(schedule));
    }

    #[benchmark]
    fn set_admin_config_forfeiture_destination() {
        let destination: T::AccountId = account("forfeiture", 0, 0);
        let config = AdminConfig::ForfeitureDestination(destination.clone());

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert!(<ForfeitureDestination<T>>::get() == Some(destination));
    }

    #[benchmark]
    fn set_admin_config_reserve_nodes(b: Linear<1, MAX_RESERVED_NODES_PER_CALL>) {
        let mut entries = Vec::new();
        for i in 0..b {
            entries.push(ReservedNodeEntry {
                node: account("reserved_node", i, i),
                owner: account("reserved_owner", i, i),
                signing_key: account("reserved_key", i, i),
            });
        }
        let config = AdminConfig::ReserveNodes(BoundedVec::truncate_from(entries));

        #[extrinsic_call]
        set_admin_config(RawOrigin::Root, config.clone());

        assert_eq!(<ReservedNodes<T>>::iter().count(), b as usize);
        assert_eq!(<TotalReservedNodes<T>>::get(), b);
    }

    #[benchmark]
    fn withdraw_rewards() {
        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let owner: T::AccountId = account("owner", 1, 1);
        let destination: T::AccountId = account("forfeiture", 0, 0);
        <ForfeitureDestination<T>>::put(destination.clone());
        // Active window at the week-one rate: worst case, both transfers run.
        <LockSchedule<T>>::put(LockScheduleInfo::new(0u64, 52u32));

        // A concrete amount: `minimum_balance()` can be zero (e.g. the mock),
        // which would make the locked claim vanish.
        let locked: BalanceOf<T> = 1_000_000_000u32.into();
        let reward_pot = Pallet::<T>::compute_reward_account_id();
        T::Currency::make_free_balance_be(
            &reward_pot,
            locked * 10u32.into() + T::Currency::minimum_balance(),
        );
        T::Currency::make_free_balance_be(&owner, T::Currency::minimum_balance());
        <LockedRewards<T>>::insert(&owner, locked);
        <TotalLockedRewards<T>>::put(locked);

        #[extrinsic_call]
        withdraw_rewards(RawOrigin::Signed(owner.clone()), None);

        assert!(<LockedRewards<T>>::get(&owner).is_zero());
        assert!(<TotalLockedRewards<T>>::get().is_zero());
        assert!(!T::Currency::free_balance(&destination).is_zero());
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality(),
        crate::tests::mock::TestRuntime,
    );
}
