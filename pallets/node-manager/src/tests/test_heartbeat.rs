// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Modified for PRDCTR on 2026-07-07.

#![cfg(test)]

use crate::{
    tests::{mock, mock::*},
    *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    registrar: AccountId,
    owner: AccountId,
    node_id: AccountId,
    signing_key: <mock::TestRuntime as pallet::Config>::SignerId,
}

impl Default for Context {
    fn default() -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let node_id = TestAccount::new([202u8; 32]).account_id();
        let signing_key_id = 987;

        setup_registrar(&registrar);
        // Mock heartbeat period 5 gives an attempt interval of 2: key index 0 attempts on even
        // blocks.
        System::set_block_number(2);
        UintAuthorityId::set_all_keys(vec![UintAuthorityId(signing_key_id)]);

        Context {
            node_id,
            registrar,
            owner: TestAccount::new([101u8; 32]).account_id(),
            signing_key: UintAuthorityId(signing_key_id),
        }
    }
}

fn setup_registrar(registrar: &AccountId) {
    <NodeRegistrar<TestRuntime>>::set(Some(*registrar));
}

fn register_node(context: &Context) {
    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(context.registrar),
        context.node_id,
        context.owner,
        context.signing_key.clone(),
    ));
}

fn pop_tx_from_mempool(pool_state: Arc<RwLock<PoolState>>) -> Extrinsic {
    let tx = pool_state.write().transactions.pop().unwrap();
    Extrinsic::decode(&mut &*tx).unwrap()
}

fn move_to_attempt_block() {
    if System::block_number() % 2 != 0 {
        System::set_block_number(System::block_number() + 1);
    }
}

fn submit_multiple_heartbeats(n: u64, pool_state: Arc<RwLock<PoolState>>) {
    for _ in 0..n {
        move_to_attempt_block();
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state.clone());
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // Move forward
        System::set_block_number(
            System::block_number() +
                <RewardPeriod<TestRuntime>>::get().heartbeat_period as u64 +
                1u64,
        );
    }
}

mod given_a_reward_period {
    use super::*;
    use test_case::test_case;

    #[test]
    fn heartbeat_submission_succeeds() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());

            let tx = pop_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            assert_eq!(
                <RewardPeriod<TestRuntime>>::get().heartbeat_period,
                <NextHeartbeatPeriod<TestRuntime>>::get()
            );

            // Check if the transaction from the mempool is what we expected
            assert!(matches!(
                tx.call,
                RuntimeCall::NodeManager(crate::Call::offchain_submit_heartbeat {
                    node: _,
                    reward_period_index: _,
                    heartbeat_count: _,
                    signature: _,
                })
            ));

            // Ensure the tx has executed successfully
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            let uptime_info =
                <NodeUptime<TestRuntime>>::get(reward_period, context.node_id).unwrap();

            let expected_weight = HEARTBEAT_BASE_WEIGHT;

            assert_eq!(uptime_info.count, 1);
            assert_eq!(uptime_info.last_reported, System::block_number());
            assert_eq!(uptime_info.weight, expected_weight);
            assert_eq!(<TotalUptime<TestRuntime>>::get(reward_period).total_heartbeats, 1);
            System::assert_last_event(
                Event::HeartbeatReceived {
                    reward_period_index: reward_period,
                    node: context.node_id,
                }
                .into(),
            );
        });
    }

    #[test]
    fn heartbeat_is_not_resent_once_on_chain() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.dispatch(frame_system::RawOrigin::None.into()));

            // Next attempt slot, still inside the heartbeat period
            System::set_block_number(System::block_number() + 2);
            NodeManager::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn undelivered_heartbeat_is_retried_on_the_next_attempt_slot() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());
            // Drop the tx without dispatching it
            let _ = pop_tx_from_mempool(pool_state.clone());

            // Not this key's attempt slot
            System::set_block_number(System::block_number() + 1);
            NodeManager::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());

            System::set_block_number(System::block_number() + 1);
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert!(matches!(
                tx.call,
                RuntimeCall::NodeManager(crate::Call::offchain_submit_heartbeat {
                    heartbeat_count: 0,
                    ..
                })
            ));
        });
    }

    #[test]
    fn mutiple_heartbeat_submission_succeeds() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // Ensure the tx has executed successfully
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;

            // Move forward
            System::set_block_number(
                System::block_number() +
                    <RewardPeriod<TestRuntime>>::get().heartbeat_period as u64 +
                    1u64,
            );

            // Call OCW and send transactions
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            let uptime_info =
                <NodeUptime<TestRuntime>>::get(reward_period, context.node_id).unwrap();
            assert_eq!(uptime_info.count, 2);
            assert_eq!(uptime_info.last_reported, System::block_number());
            assert_eq!(<TotalUptime<TestRuntime>>::get(reward_period).total_heartbeats, 2);
            System::assert_last_event(
                Event::HeartbeatReceived {
                    reward_period_index: reward_period,
                    node: context.node_id,
                }
                .into(),
            );
        });
    }

    #[test]
    fn heartbeat_period_is_respected() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            // Heartbeat included at block 2: the next one is due from block 7
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.dispatch(frame_system::RawOrigin::None.into()));

            for _ in 0..2 {
                roll_forward(2u64);
                NodeManager::offchain_worker(System::block_number());
                // Attempt slot, but the heartbeat period has not passed
                assert!(pool_state.read().transactions.is_empty());
            }

            roll_forward(2u64);
            assert_eq!(System::block_number(), 8);
            NodeManager::offchain_worker(System::block_number());
            assert!(!pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn external_unsigned_calls_are_allowed() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());
            assert!(!pool_state.read().transactions.is_empty());
            let runtime_call = pop_tx_from_mempool(pool_state.clone());

            match runtime_call.call {
                RuntimeCall::NodeManager(call) => {
                    assert_ok!(<NodeManager as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::External,
                        &call
                    ));
                },
                _ => panic!("expected a NodeManager call"),
            }
        });
    }

    #[test]
    fn signing_key_can_be_rotated() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // Rotate signing key
            let new_signing_key = UintAuthorityId(999);
            assert_ok!(NodeManager::update_signing_key(
                RuntimeOrigin::signed(context.owner),
                context.node_id,
                new_signing_key.clone(),
            ));

            // Move forward
            System::set_block_number(
                System::block_number() +
                    <RewardPeriod<TestRuntime>>::get().heartbeat_period as u64 +
                    1u64,
            );

            NodeManager::offchain_worker(System::block_number());
            // Ensure heartbeat is NOT submitted with old signing key
            assert!(pool_state.read().transactions.is_empty());

            // Set the new key in OCW keystore
            UintAuthorityId::set_all_keys(vec![new_signing_key]);

            // Now we expect the heartbeat to be submitted successfully
            NodeManager::offchain_worker(System::block_number());
            assert!(!pool_state.read().transactions.is_empty());
            let runtime_call = pop_tx_from_mempool(pool_state.clone());

            match runtime_call.call {
                RuntimeCall::NodeManager(call) => {
                    assert_ok!(<NodeManager as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::External,
                        &call
                    ));
                },
                _ => panic!("expected a NodeManager call"),
            }
        });
    }

    #[test_case(1; "less than 2000")]
    #[test_case(2000; "exactly 2000")]
    #[test_case(5000; "exactly 5000")]
    #[test_case(2001; "former 50% bonus tier, now flat")]
    #[test_case(5001; "former 25% bonus tier, now flat")]
    #[test_case(10000; "exactly 10000")]
    #[test_case(10001; "over 10000")]
    fn weight_calculated_correctly(serial_num: u32) {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            NextNodeSerialNumber::<TestRuntime>::put(serial_num);
            register_node(&context);

            NodeManager::offchain_worker(System::block_number());

            let tx = pop_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            assert!(matches!(
                tx.call,
                RuntimeCall::NodeManager(crate::Call::offchain_submit_heartbeat {
                    node: _,
                    reward_period_index: _,
                    heartbeat_count: _,
                    signature: _,
                })
            ));

            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            let uptime_info =
                <NodeUptime<TestRuntime>>::get(reward_period, context.node_id).unwrap();

            // Every heartbeat now contributes exactly
            // HEARTBEAT_BASE_WEIGHT regardless of serial number.
            let expected_weight = HEARTBEAT_BASE_WEIGHT;

            assert_eq!(uptime_info.weight, expected_weight);
            assert_eq!(<TotalUptime<TestRuntime>>::get(reward_period).total_heartbeats, 1);
            System::assert_last_event(
                Event::HeartbeatReceived {
                    reward_period_index: reward_period,
                    node: context.node_id,
                }
                .into(),
            );
        });
    }
}

mod across_multiple_reward_periods {
    use super::*;

    #[test]
    fn mutiple_heartbeat_submissions_succeed() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let old_reward_period = reward_period.current;
            let reward_period_length = reward_period.length as u64;

            let old_heartbeat_count = 4u64;
            submit_multiple_heartbeats(old_heartbeat_count, pool_state.clone());

            if System::block_number() < reward_period_length {
                roll_forward((reward_period_length - System::block_number()) + 1);
            } else {
                roll_forward(1u64);
            }
            assert_eq!(<RewardPeriod<TestRuntime>>::get().current, old_reward_period + 1);

            let new_heartbeat_count = old_heartbeat_count + 1;
            submit_multiple_heartbeats(new_heartbeat_count, pool_state.clone());

            // Ensure the tx has executed successfully
            let new_reward_period = <RewardPeriod<TestRuntime>>::get().current;

            let uptime_info =
                <NodeUptime<TestRuntime>>::get(old_reward_period, context.node_id).unwrap();
            assert_eq!(uptime_info.count, old_heartbeat_count);
            assert_eq!(
                <TotalUptime<TestRuntime>>::get(old_reward_period).total_heartbeats,
                old_heartbeat_count
            );

            let uptime_info =
                <NodeUptime<TestRuntime>>::get(new_reward_period, context.node_id).unwrap();
            assert_eq!(uptime_info.count, new_heartbeat_count);
            assert_eq!(
                <TotalUptime<TestRuntime>>::get(new_reward_period).total_heartbeats,
                new_heartbeat_count
            );

            System::assert_last_event(
                Event::HeartbeatReceived {
                    reward_period_index: new_reward_period,
                    node: context.node_id,
                }
                .into(),
            );
        });
    }
}

mod fails_when {
    use super::*;

    #[test]
    fn duplicate_heartbeats_submitted() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            let signature =
                context.signing_key.sign(&("DummyProof").encode()).expect("Error signing");
            assert_noop!(
                NodeManager::offchain_submit_heartbeat(
                    RawOrigin::None.into(),
                    context.node_id,
                    reward_period,
                    1u64,
                    signature
                ),
                Error::<TestRuntime>::DuplicateHeartbeat
            );
        });
    }

    #[test]
    fn wrong_period_used() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            let signature =
                context.signing_key.sign(&("DummyProof").encode()).expect("Error signing");

            let bad_reward_period = reward_period + 1;
            assert_noop!(
                NodeManager::offchain_submit_heartbeat(
                    RawOrigin::None.into(),
                    context.node_id,
                    bad_reward_period,
                    1u64,
                    signature
                ),
                Error::<TestRuntime>::InvalidHeartbeat
            );
        });
    }

    #[test]
    fn submitter_is_not_registered() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            let signature =
                context.signing_key.sign(&("DummyProof").encode()).expect("Error signing");

            let bad_node = TestAccount::new([31u8; 32]).account_id();
            assert_noop!(
                NodeManager::offchain_submit_heartbeat(
                    RawOrigin::None.into(),
                    bad_node,
                    reward_period,
                    1u64,
                    signature
                ),
                Error::<TestRuntime>::NodeNotRegistered
            );
        });
    }

    #[test]
    fn wrong_uptime_count_is_used() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            let signature =
                context.signing_key.sign(&("DummyProof").encode()).expect("Error signing");

            let bad_uptime_count = 99u64;
            assert_noop!(
                NodeManager::offchain_submit_heartbeat(
                    RawOrigin::None.into(),
                    context.node_id,
                    reward_period,
                    bad_uptime_count,
                    signature
                ),
                Error::<TestRuntime>::InvalidHeartbeat
            );
        });
    }

    #[test]
    fn keystore_not_populated() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let mut context = Context::default();
            let key_id = 987;
            context.signing_key = UintAuthorityId(key_id);
            register_node(&context);

            // Clear keystore keys
            let keys: Vec<UintAuthorityId> = vec![];
            UintAuthorityId::set_all_keys(keys);

            NodeManager::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());

            // Prove that it can work if the keystore was populated
            UintAuthorityId::set_all_keys(vec![UintAuthorityId(key_id)]);
            NodeManager::offchain_worker(System::block_number());
            assert!(!pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn rewards_are_disabled() {
        let (mut ext, _ool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let mut context = Context::default();
            //Disable rewards
            RewardEnabled::<TestRuntime>::put(false);

            let key_id = 987;
            context.signing_key = UintAuthorityId(key_id);
            register_node(&context);

            let call = crate::Call::offchain_submit_heartbeat {
                node: context.node_id,
                reward_period_index: 1u64,
                heartbeat_count: 1u64,
                signature: context
                    .signing_key
                    .sign(&("DummyProof").encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &call
                ),
                InvalidTransaction::Custom(ERROR_CODE_REWARD_DISABLED)
            );
        });
    }

    #[test]
    fn heartbeat_threshold_reached() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            register_node(&context);

            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let min_heartbeats = reward_period.uptime_threshold;

            submit_multiple_heartbeats(min_heartbeats.into(), pool_state.clone());

            assert_noop!(
                NodeManager::offchain_submit_heartbeat(
                    RawOrigin::None.into(),
                    context.node_id,
                    reward_period.current,
                    min_heartbeats.into(),
                    context.signing_key.sign(&("DummyProof").encode()).expect("Error signing")
                ),
                Error::<TestRuntime>::HeartbeatThresholdReached
            );
        });
    }

    #[test]
    fn unsigned_calls_are_rejected_early() {
        let (mut ext, _ool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let mut context = Context::default();

            let key_id = 987;
            context.signing_key = UintAuthorityId(key_id);
            register_node(&context);

            let bad_reward_period = 90u64;
            let bad_heartbeat_count = 99u64;

            // Bad reward period will cause early rejection
            let call = crate::Call::offchain_submit_heartbeat {
                node: context.node_id,
                reward_period_index: bad_reward_period,
                heartbeat_count: 0u64,
                signature: context
                    .signing_key
                    .sign(&(HEARTBEAT_CONTEXT, 0u64, 0u64).encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &call
                ),
                InvalidTransaction::Custom(ERROR_CODE_INVALID_HEARTBEAT)
            );

            // Bad heartbeat count will cause early rejection
            let call = crate::Call::offchain_submit_heartbeat {
                node: context.node_id,
                reward_period_index: 0u64,
                heartbeat_count: bad_heartbeat_count,
                signature: context
                    .signing_key
                    .sign(&(HEARTBEAT_CONTEXT, 0u64, 0u64).encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &call
                ),
                InvalidTransaction::Custom(ERROR_CODE_INVALID_HEARTBEAT)
            );

            // Bad signature will cause early rejection
            let call = crate::Call::offchain_submit_heartbeat {
                node: context.node_id,
                reward_period_index: 0u64,
                heartbeat_count: 0u64,
                signature: context
                    .signing_key
                    .sign(&("DummyProof").encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &call
                ),
                InvalidTransaction::Custom(ERROR_CODE_INVALID_HEARTBEAT_SIGNATURE)
            );

            // Good params works
            let call = crate::Call::offchain_submit_heartbeat {
                node: context.node_id,
                reward_period_index: 0u64,
                heartbeat_count: 0u64,
                signature: context
                    .signing_key
                    .sign(&(HEARTBEAT_CONTEXT, 0u64, 0u64).encode())
                    .expect("Error signing"),
            };

            assert_ok!(<NodeManager as ValidateUnsigned>::validate_unsigned(
                TransactionSource::Local,
                &call
            ));
        });
    }
}

mod with_multiple_local_keys {
    use super::*;

    fn register_node_with_key(context: &Context, seed: u8, key: u64) -> AccountId {
        let node_id = TestAccount::new([seed; 32]).account_id();
        assert_ok!(NodeManager::register_node(
            RuntimeOrigin::signed(context.registrar),
            node_id,
            context.owner,
            UintAuthorityId(key),
        ));
        node_id
    }

    fn drain_heartbeats(pool_state: Arc<RwLock<PoolState>>) -> Vec<(AccountId, RuntimeCall)> {
        pool_state
            .write()
            .transactions
            .drain(..)
            .map(|tx| {
                let call = Extrinsic::decode(&mut &*tx).unwrap().call;
                match &call {
                    RuntimeCall::NodeManager(crate::Call::offchain_submit_heartbeat {
                        node,
                        ..
                    }) => (*node, call),
                    _ => panic!("expected a heartbeat call"),
                }
            })
            .collect()
    }

    fn submitted_nodes(pool_state: Arc<RwLock<PoolState>>) -> Vec<AccountId> {
        let mut nodes: Vec<AccountId> =
            drain_heartbeats(pool_state).into_iter().map(|(node, _)| node).collect();
        nodes.sort();
        nodes
    }

    fn sorted(mut nodes: Vec<AccountId>) -> Vec<AccountId> {
        nodes.sort();
        nodes
    }

    #[test]
    fn keys_are_spread_across_attempt_slots() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            let node_a = register_node_with_key(&context, 50, 10);
            let node_b = register_node_with_key(&context, 51, 11);
            let node_c = register_node_with_key(&context, 52, 12);
            UintAuthorityId::set_all_keys(vec![
                UintAuthorityId(12),
                UintAuthorityId(10),
                UintAuthorityId(11),
            ]);

            // Interval 2: sorted indexes 0 and 2 attempt on even blocks, index 1 on odd blocks
            System::set_block_number(2);
            NodeManager::offchain_worker(System::block_number());
            let even = drain_heartbeats(pool_state.clone());
            let even_nodes = sorted(even.iter().map(|(node, _)| *node).collect());
            assert_eq!(even_nodes, sorted(vec![node_a, node_c]));

            System::set_block_number(3);
            NodeManager::offchain_worker(System::block_number());
            let odd = drain_heartbeats(pool_state.clone());
            assert_eq!(odd.iter().map(|(node, _)| *node).collect::<Vec<_>>(), vec![node_b]);

            for (_, call) in even.into_iter().chain(odd) {
                assert_ok!(call.dispatch(frame_system::RawOrigin::None.into()));
            }
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
            for node in [node_a, node_b, node_c] {
                assert_eq!(<NodeUptime<TestRuntime>>::get(reward_period, node).unwrap().count, 1);
            }

            // The chain now blocks every key until the heartbeat period passes
            for block in [4, 5] {
                System::set_block_number(block);
                NodeManager::offchain_worker(System::block_number());
                assert!(pool_state.read().transactions.is_empty());
            }
        });
    }

    #[test]
    fn unregistered_local_key_is_skipped() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            let node_a = register_node_with_key(&context, 50, 10);
            UintAuthorityId::set_all_keys(vec![UintAuthorityId(5), UintAuthorityId(10)]);

            for block in [2, 3] {
                System::set_block_number(block);
                NodeManager::offchain_worker(System::block_number());
            }
            assert_eq!(submitted_nodes(pool_state), vec![node_a]);
        });
    }

    #[test]
    fn stale_signing_key_index_is_ignored() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::default();
            let node_a = register_node_with_key(&context, 50, 10);
            // Index points at a node registered with a different key
            SigningKeyToNodeId::<TestRuntime>::insert(UintAuthorityId(7), node_a);
            UintAuthorityId::set_all_keys(vec![UintAuthorityId(7)]);

            NodeManager::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());
        });
    }
}

mod attempt_slots {
    use crate::offchain::{
        heartbeat_attempt_interval, is_attempt_slot, HEARTBEAT_ATTEMPT_INTERVAL,
    };

    #[test]
    fn interval_is_capped_at_half_the_heartbeat_period() {
        assert_eq!(heartbeat_attempt_interval(300), HEARTBEAT_ATTEMPT_INTERVAL as u64);
        assert_eq!(heartbeat_attempt_interval(40), 20);
        assert_eq!(heartbeat_attempt_interval(30), 15);
        assert_eq!(heartbeat_attempt_interval(5), 2);
        assert_eq!(heartbeat_attempt_interval(1), 1);
        assert_eq!(heartbeat_attempt_interval(0), 1);
    }

    #[test]
    fn single_key_attempts_once_per_interval() {
        let attempts: Vec<u64> = (0..60).filter(|block| is_attempt_slot(0, 20, *block)).collect();
        assert_eq!(attempts, vec![0, 20, 40]);
    }

    #[test]
    fn keys_are_spread_round_robin() {
        // 1000 keys over an interval of 20: 50 keys per block, every key once per interval
        for block in 0..20u64 {
            let due = (0..1_000).filter(|index| is_attempt_slot(*index, 20, block)).count();
            assert_eq!(due, 50);
        }
        let key_attempts = (0..20u64).filter(|block| is_attempt_slot(137, 20, *block)).count();
        assert_eq!(key_attempts, 1);
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        assert!(is_attempt_slot(5, 0, 7));
        assert!(is_attempt_slot(usize::MAX, 1, u64::MAX));
        let _ = is_attempt_slot(usize::MAX, u64::MAX, u64::MAX);
    }
}
