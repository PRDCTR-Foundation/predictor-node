// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-09-15.

#![cfg(test)]

use crate::{tests::mock::*, *};
use frame_support::{assert_noop, assert_ok, BoundedVec};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

type SignerId = <TestRuntime as pallet::Config>::SignerId;

fn setup_registrar() -> AccountId {
    let registrar = TestAccount::new([44u8; 32]).account_id();
    <NodeRegistrar<TestRuntime>>::set(Some(registrar));
    registrar
}

fn entry(
    node: AccountId,
    owner: AccountId,
    signing_key: SignerId,
) -> ReservedNodeEntry<AccountId, SignerId> {
    ReservedNodeEntry { node, owner, signing_key }
}

fn reserve(entries: Vec<ReservedNodeEntry<AccountId, SignerId>>) {
    assert_ok!(NodeManager::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::ReserveNodes(BoundedVec::truncate_from(entries)),
    ));
}

mod reserve_nodes {
    use super::*;

    #[test]
    fn root_can_reserve_a_batch() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let node = TestAccount::new([1u8; 32]).account_id();
            let owner = TestAccount::new([2u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            reserve(vec![entry(node, owner, signing_key.clone())]);

            let reserved = ReservedNodes::<TestRuntime>::get(node).expect("node must be reserved");
            assert_eq!(reserved.owner, owner);
            assert_eq!(reserved.signing_key, signing_key);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 1);
            System::assert_last_event(Event::NodesReserved { count: 1 }.into());
        });
    }

    #[test]
    fn root_can_reserve_multiple_entries_in_one_call() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let entries: Vec<_> = (0..5u8)
                .map(|i| {
                    entry(
                        TestAccount::new([i; 32]).account_id(),
                        TestAccount::new([i + 100; 32]).account_id(),
                        SignerId::generate_pair(None),
                    )
                })
                .collect();
            let nodes: Vec<_> = entries.iter().map(|e| e.node).collect();

            reserve(entries);

            for node in nodes {
                assert!(ReservedNodes::<TestRuntime>::get(node).is_some());
            }
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 5);
            System::assert_last_event(Event::NodesReserved { count: 5 }.into());
        });
    }

    #[test]
    fn re_reserving_an_already_pending_node_does_not_double_count_it() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let node = TestAccount::new([3u8; 32]).account_id();
            let owner = TestAccount::new([4u8; 32]).account_id();
            let first_key = SignerId::generate_pair(None);
            let second_key = SignerId::generate_pair(None);

            // Reserve once, then again (e.g. correcting the signing key)
            // across two separate calls.
            reserve(vec![entry(node, owner, first_key)]);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 1);

            reserve(vec![entry(node, owner, second_key.clone())]);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 1);
            assert_eq!(
                ReservedNodes::<TestRuntime>::get(node).expect("still reserved").signing_key,
                second_key
            );

            // Repeating the same node twice within one batch is likewise
            // only one net-new reservation.
            let other_node = TestAccount::new([5u8; 32]).account_id();
            reserve(vec![
                entry(other_node, owner, SignerId::generate_pair(None)),
                entry(other_node, owner, SignerId::generate_pair(None)),
            ]);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 2);
        });
    }

    #[test]
    fn non_root_cannot_reserve() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let caller = TestAccount::new([9u8; 32]).account_id();
            let entries: BoundedVec<
                ReservedNodeEntry<AccountId, SignerId>,
                MaxReservedNodesPerCall,
            > = BoundedVec::truncate_from(vec![]);
            assert_noop!(
                NodeManager::set_admin_config(
                    RuntimeOrigin::signed(caller),
                    AdminConfig::ReserveNodes(entries),
                ),
                DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn more_than_the_max_cannot_be_bounded_into_one_call() {
        let too_many: Vec<_> = (0..(MAX_RESERVED_NODES_PER_CALL + 1) as u16)
            .map(|i| {
                entry(
                    TestAccount::new([(i % 256) as u8; 32]).account_id(),
                    TestAccount::new([((i + 1) % 256) as u8; 32]).account_id(),
                    SignerId::generate_pair(None),
                )
            })
            .collect();

        assert!(
            BoundedVec::<ReservedNodeEntry<AccountId, SignerId>, MaxReservedNodesPerCall>::try_from(
                too_many
            )
            .is_err(),
            "a batch above MaxReservedNodesPerCall must not fit in the bounded call argument"
        );
    }
}

mod migration {
    use super::*;

    #[test]
    fn registering_a_reserved_node_credits_a_full_period_of_uptime() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let owner = TestAccount::new([10u8; 32]).account_id();
            let node = TestAccount::new([20u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            reserve(vec![entry(node, owner, signing_key.clone())]);

            assert_ok!(NodeManager::register_node(
                RuntimeOrigin::signed(registrar),
                node,
                owner,
                signing_key,
            ));

            // The reservation is consumed.
            assert!(ReservedNodes::<TestRuntime>::get(node).is_none());

            let reward_period = RewardPeriod::<TestRuntime>::get();
            let threshold = reward_period.uptime_threshold;
            assert!(threshold > 0, "genesis params must yield a non-zero uptime threshold");

            let uptime = NodeUptime::<TestRuntime>::get(reward_period.current, node)
                .expect("uptime must be seeded for the current period");
            assert_eq!(uptime.count, threshold as u64);
            assert_eq!(uptime.weight, HEARTBEAT_BASE_WEIGHT.saturating_mul(threshold as u128));

            let total = TotalUptime::<TestRuntime>::get(reward_period.current);
            assert_eq!(total.total_heartbeats, threshold as u64);
            assert_eq!(total.total_weight, HEARTBEAT_BASE_WEIGHT.saturating_mul(threshold as u128));

            System::assert_last_event(
                Event::NodeMigrated { owner, node, credited_heartbeats: threshold }.into(),
            );
        });
    }

    #[test]
    fn a_brand_new_unreserved_registration_gets_no_extra_credit() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let owner = TestAccount::new([11u8; 32]).account_id();
            let node = TestAccount::new([21u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            assert_ok!(NodeManager::register_node(
                RuntimeOrigin::signed(registrar),
                node,
                owner,
                signing_key,
            ));

            let reward_period = RewardPeriod::<TestRuntime>::get();
            assert!(NodeUptime::<TestRuntime>::get(reward_period.current, node).is_none());
            System::assert_last_event(Event::NodeRegistered { owner, node }.into());
        });
    }

    #[test]
    fn mismatched_owner_is_rejected() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let reserved_owner = TestAccount::new([12u8; 32]).account_id();
            let different_owner = TestAccount::new([13u8; 32]).account_id();
            let node = TestAccount::new([22u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            reserve(vec![entry(node, reserved_owner, signing_key.clone())]);

            assert_noop!(
                NodeManager::register_node(
                    RuntimeOrigin::signed(registrar),
                    node,
                    different_owner,
                    signing_key,
                ),
                Error::<TestRuntime>::ReservedNodeMismatch
            );

            // The reservation and node stay untouched after the rejected attempt.
            assert!(ReservedNodes::<TestRuntime>::get(node).is_some());
            assert!(NodeRegistry::<TestRuntime>::get(node).is_none());
        });
    }

    #[test]
    fn mismatched_signing_key_is_rejected() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let owner = TestAccount::new([14u8; 32]).account_id();
            let node = TestAccount::new([23u8; 32]).account_id();
            let reserved_key = SignerId::generate_pair(None);
            let different_key = SignerId::generate_pair(None);

            reserve(vec![entry(node, owner, reserved_key)]);

            assert_noop!(
                NodeManager::register_node(
                    RuntimeOrigin::signed(registrar),
                    node,
                    owner,
                    different_key,
                ),
                Error::<TestRuntime>::ReservedNodeMismatch
            );
        });
    }

    #[test]
    fn reservation_is_consumed_after_one_successful_registration() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let owner = TestAccount::new([15u8; 32]).account_id();
            let node = TestAccount::new([24u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            reserve(vec![entry(node, owner, signing_key.clone())]);

            assert_ok!(NodeManager::register_node(
                RuntimeOrigin::signed(registrar),
                node,
                owner,
                signing_key.clone(),
            ));

            // Deregister and re-register the same node ID: this is a fresh
            // registration now that the reservation has been consumed, so it
            // must not be credited (or emit `NodeMigrated`) a second time.
            assert_ok!(NodeManager::deregister_nodes(
                RuntimeOrigin::signed(registrar),
                owner,
                BoundedVec::truncate_from(vec![node]),
            ));

            let new_signing_key = SignerId::generate_pair(None);
            assert_ok!(NodeManager::register_node(
                RuntimeOrigin::signed(registrar),
                node,
                owner,
                new_signing_key,
            ));

            System::assert_last_event(Event::NodeRegistered { owner, node }.into());
        });
    }

    #[test]
    fn reserved_nodes_count_toward_the_network_capacity() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let registrar = setup_registrar();
            let owner = TestAccount::new([16u8; 32]).account_id();
            let reserved_node = TestAccount::new([25u8; 32]).account_id();
            let signing_key = SignerId::generate_pair(None);

            reserve(vec![entry(reserved_node, owner, signing_key.clone())]);

            // Pretend the rest of the network - registered nodes plus this
            // one pending reservation - has already filled the cap.
            let cap = <TestRuntime as pallet::Config>::MaxRegisteredNodes::get();
            TotalRegisteredNodes::<TestRuntime>::put(cap - 1);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 1);

            // A brand-new (unreserved) registration is blocked: the reserved
            // slot already accounts for the last bit of capacity.
            let new_node = TestAccount::new([26u8; 32]).account_id();
            let new_key = SignerId::generate_pair(None);
            assert_noop!(
                NodeManager::register_node(
                    RuntimeOrigin::signed(registrar),
                    new_node,
                    owner,
                    new_key,
                ),
                Error::<TestRuntime>::MaxNodesReached
            );

            // Migrating the reserved node succeeds regardless: it converts
            // an already-counted reserved slot into a registered one rather
            // than claiming a new one.
            assert_ok!(NodeManager::register_node(
                RuntimeOrigin::signed(registrar),
                reserved_node,
                owner,
                signing_key,
            ));
            assert_eq!(TotalRegisteredNodes::<TestRuntime>::get(), cap);
            assert_eq!(TotalReservedNodes::<TestRuntime>::get(), 0);
        });
    }
}
