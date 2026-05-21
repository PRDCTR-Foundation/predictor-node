//! # Node manager benchmarks
// Copyright 2026 Aventus DAO.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};
use sp_avn_common::Proof;

// Inlined from sp_avn_common::benchmarking on the avn-parachain main branch.
// The helper isn't on the published feat/create-stable-2409-branch this
// workspace consumes; once it lands upstream, replace this with a
// `use sp_avn_common::benchmarking::convert_sr25519_signature;`.
fn convert_sr25519_signature<Signature>(signature: sp_core::sr25519::Signature) -> Signature
where
    Signature: parity_scale_codec::Decode + parity_scale_codec::Encode + 'static,
{
    use core::any::TypeId;
    use parity_scale_codec::Encode;
    use sp_runtime::MultiSignature;

    if TypeId::of::<Signature>() == TypeId::of::<MultiSignature>() {
        let multi_sig = MultiSignature::from(signature);
        Signature::decode(&mut &multi_sig.encode()[..]).expect("MultiSignature decodes")
    } else if TypeId::of::<Signature>() == TypeId::of::<sp_core::sr25519::Signature>() {
        Signature::decode(&mut &signature.encode()[..]).expect("sr25519 signature decodes")
    } else {
        Signature::decode(&mut &signature.encode()[..]).expect("signature bytes decode")
    }
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

fn set_registrar<T: Config>(registrar: T::AccountId) {
    <NodeRegistrar<T>>::set(Some(registrar.clone()));
}

fn register_new_node<T: Config>(node: NodeId<T>, owner: T::AccountId) -> T::SignerId {
    let key = T::SignerId::generate_pair(None);
    <NodeRegistry<T>>::insert(
        node.clone(),
        NodeInfo::new(owner.clone(), key.clone(), 0u32),
    );
    <OwnedNodes<T>>::insert(owner.clone(), node, ());
    <OwnedNodesCount<T>>::mutate(owner, |count| *count += 1);

    key
}

fn create_heartbeat<T: Config>(node: NodeId<T>, reward_period_index: RewardPeriodIndex) {
    let uptime = 1u64;
    let weight = HEARTBEAT_BASE_WEIGHT.saturating_mul(uptime.into());

    <NodeUptime<T>>::mutate(&reward_period_index, &node, |maybe_info| {
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

    <TotalUptime<T>>::mutate(&reward_period_index, |total| {
        total.total_heartbeats = total.total_heartbeats.saturating_add(1u64);
        total.total_weight = total.total_weight.saturating_add(weight);
    });
}

fn fund_reward_pot<T: Config>() {
    let reward_amount = NextRewardAmountPerPeriod::<T>::get() * 2000u32.into();
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

fn get_proof<T: Config>(
    relayer: &T::AccountId,
    signer: &T::AccountId,
    signature: sp_core::sr25519::Signature,
) -> Proof<T::Signature, T::AccountId> {
    return Proof {
        signer: signer.clone(),
        relayer: relayer.clone(),
        signature: convert_sr25519_signature::<T::Signature>(signature),
    }
}

fn enable_rewards<T: Config>()
where
    T: pallet_timestamp::Config<Moment = u64>,
{
    <RewardEnabled<T>>::set(true);
    pallet_timestamp::Pallet::<T>::set_timestamp(10 * 12_000);
}

benchmarks! {
    where_clause {
        where T: pallet_timestamp::Config<Moment = u64>
    }

    register_node {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);
    }: register_node(RawOrigin::Signed(registrar.clone()), node.clone(), owner.clone(), signing_key.clone())
    verify {
        let _node_info = <NodeRegistry<T>>::get(&node).expect("Node must be registered");
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), node.clone()));
        assert_last_event::<T>(Event::NodeRegistered {owner, node}.into());
    }

    set_admin_config_registrar {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());
        let new_registrar: T::AccountId = account("new_registrar", 0, 0);
        let config = AdminConfig::NodeRegistrar(new_registrar.clone());

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<NodeRegistrar<T>>::get() == Some(new_registrar));
    }

    set_admin_config_reward_period {
        let current_reward_period = <NextRewardPeriodLength<T>>::get();
        let new_reward_period = current_reward_period + 1u32;
        let config = AdminConfig::NextRewardPeriodLength(new_reward_period);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<NextRewardPeriodLength<T>>::get() == new_reward_period);
    }

    set_admin_config_reward_batch_size {
        let current_batch_size = <MaxBatchSize<T>>::get();
        let new_batch_size = current_batch_size + 1u32;
        let config = AdminConfig::BatchSize(new_batch_size);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<MaxBatchSize<T>>::get() == new_batch_size);
    }

    set_admin_config_reward_heartbeat {
        let current_heartbeat = <NextHeartbeatPeriod<T>>::get();
        let new_heartbeat = current_heartbeat + 1u32;
        let config = AdminConfig::NextHeartbeatPeriod(new_heartbeat);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<NextHeartbeatPeriod<T>>::get() == new_heartbeat);
    }

    set_admin_config_reward_amount {
        let current_amount = <NextRewardAmountPerPeriod<T>>::get();
        let new_amount = current_amount + 1u32.into();
        let config = AdminConfig::NextRewardAmountPerPeriod(new_amount);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<NextRewardAmountPerPeriod<T>>::get() == new_amount);
    }

    set_admin_config_reward_enabled {
        let current_flag = <RewardEnabled<T>>::get();
        let new_flag = !current_flag;
        let config = AdminConfig::RewardEnabled(new_flag);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<RewardEnabled<T>>::get() == new_flag);
    }

    set_admin_config_min_threshold {
        let new_threshold = Perbill::from_percent(80);
        let config = AdminConfig::MinUptimeThreshold(new_threshold);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<MinUptimeThreshold<T>>::get() == Some(new_threshold));
    }

    on_initialise_with_new_reward_period {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> = reward_period.first + BlockNumberFor::<T>::from(reward_period.length) + 1u32.into();
        enable_rewards::<T>();
    }: { Pallet::<T>::on_initialize(block_number) }
    verify {
        let new_reward_period_index = reward_period.current + 1u64;
        let new_reward_period = <RewardPeriod<T>>::get();
        assert!(new_reward_period_index== new_reward_period.current);
        assert_last_event::<T>(Event::NewRewardPeriodStarted {
            reward_period_index: new_reward_period_index,
            reward_period_length: reward_period.length,
            uptime_threshold: new_reward_period.uptime_threshold,
            previous_period_reward: reward_period.reward_amount}.into());
    }

    on_initialise_no_reward_period {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> =
            BlockNumberFor::<T>::from(reward_period.length) - 1u32.into();
        enable_rewards::<T>();
    }: { Pallet::<T>::on_initialize(block_number) }
    verify {
        assert!(reward_period.current == <RewardPeriod<T>>::get().current);
    }

    offchain_submit_heartbeat {
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
            frame_system::Pallet::<T>::block_number() + <NextHeartbeatPeriod<T>>::get().into() + 1u32.into()
        );

        let heartbeat_count = 1u64;
        let signature = signing_key.sign(
            &(HEARTBEAT_CONTEXT, heartbeat_count, reward_period_index).encode()
        ).expect("Error signing");
    }: offchain_submit_heartbeat(RawOrigin::None, node.clone(), reward_period_index, heartbeat_count, signature)
    verify {
        let uptime_info = <NodeUptime<T>>::get(reward_period_index, &node).expect("No uptime info");
        assert!(uptime_info.count == heartbeat_count + 1);
        assert_last_event::<T>(Event::HeartbeatReceived {reward_period_index, node}.into());
    }

    signed_register_node {
        enable_rewards::<T>();
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");
        set_registrar::<T>(registrar.clone());

        let relayer: T::AccountId = account("relayer", 11, 11);
        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);
        let now = frame_system::Pallet::<T>::block_number();

        let signed_payload = encode_signed_register_node_params::<T>(
            &relayer.clone(),
            &node,
            &owner,
            &signing_key,
            &now.clone(),
        );

        let signature = registrar_key.sign(&signed_payload).ok_or("Error signing proof")?;
        let proof = get_proof::<T>(&relayer.clone(), &registrar, signature.into());
    }: signed_register_node(RawOrigin::Signed(registrar.clone()), proof.clone(), node.clone(), owner.clone(), signing_key.clone(), now)
    verify {
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), node.clone()));
        assert!(<NodeRegistry<T>>::contains_key(node.clone()));
        assert_last_event::<T>(Event::NodeRegistered{owner, node}.into());
    }

    deregister_nodes {
        let b in 1 .. MAX_NODES_TO_DEREGISTER;
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);

        let nodes_to_deregister = create_nodes_and_heartbeat::<T>(owner.clone(), reward_period_index, b);

        // Show that the nodes are registered
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), nodes_to_deregister[0].clone()));
        assert!(<NodeRegistry<T>>::contains_key(nodes_to_deregister[0].clone()));

    }: deregister_nodes(
        RawOrigin::Signed(registrar.clone()),
        owner.clone(),
        BoundedVec::truncate_from(nodes_to_deregister.clone()))
    verify {
        for node in &nodes_to_deregister {
            assert!(!<OwnedNodes<T>>::contains_key(owner.clone(), node));
            assert!(!<NodeRegistry<T>>::contains_key(node));
        }
        assert_last_event::<T>(Event::NodeDeregistered{
            owner,
            node: nodes_to_deregister[nodes_to_deregister.len() - 1].clone()}.into());
    }

    signed_deregister_nodes {
        let b in 1 .. MAX_NODES_TO_DEREGISTER;
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");

        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);

        let nodes_to_deregister = create_nodes_and_heartbeat::<T>(owner.clone(), reward_period_index, b);

        // Show that at least some of the nodes are registered
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), nodes_to_deregister[0].clone()));
        assert!(<NodeRegistry<T>>::contains_key(nodes_to_deregister[0].clone()));

        let relayer: T::AccountId = account("relayer", 11, 11);
        let now = frame_system::Pallet::<T>::block_number();

        let bounded_nodes_to_deregister = BoundedVec::truncate_from(nodes_to_deregister.clone());
        let signed_payload = encode_signed_deregister_node_params::<T>(
            &relayer.clone(),
            &owner,
            &bounded_nodes_to_deregister,
            &(nodes_to_deregister.len() as u32),
            &now.clone(),
        );

        let signature = registrar_key.sign(&signed_payload).ok_or("Error signing proof")?;
        let proof = get_proof::<T>(&relayer.clone(), &registrar, signature.into());
    }: signed_deregister_nodes(RawOrigin::Signed(registrar.clone()), proof, owner.clone(), bounded_nodes_to_deregister, now)
    verify {
        for node in &nodes_to_deregister {
            assert!(!<OwnedNodes<T>>::contains_key(owner.clone(), node));
            assert!(!<NodeRegistry<T>>::contains_key(node));
        }
        assert_last_event::<T>(Event::NodeDeregistered{
            owner,
            node: nodes_to_deregister[nodes_to_deregister.len() - 1].clone()}.into());
    }

    update_signing_key {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let _current_signing_key: T::SignerId = register_new_node::<T>(node.clone(), owner.clone());
        let new_signing_key: T::SignerId = account("new_signing_key", 3, 3);
    }: update_signing_key(RawOrigin::Signed(owner.clone()), node.clone(), new_signing_key.clone())
    verify {
        let node_info = <NodeRegistry<T>>::get(&node).expect("Node must be registered");
        assert!(node_info.signing_key == new_signing_key);
        assert_last_event::<T>(Event::SigningKeyUpdated {owner, node}.into());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().with_genesis_config().as_externality(),
    crate::mock::TestRuntime,
);
