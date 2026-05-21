// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, offchain::OCW_ID, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    registrar: AccountId,
    owner: AccountId,
    ocw_node: AccountId,
}

impl Context {
    fn new(num_of_nodes: u8) -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let owner = TestAccount::new([209u8; 32]).account_id();
        let reward_amount: BalanceOf<TestRuntime> = <NextRewardAmountPerPeriod<TestRuntime>>::get();

        <NumPeriodsToMint<TestRuntime>>::put(2u32);

        Balances::make_free_balance_be(
            &NodeManager::compute_reward_account_id(),
            reward_amount * 2u128,
        );
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));
        let ocw_node = register_nodes(registrar, owner, num_of_nodes);

        Context { registrar, owner, ocw_node }
    }
}

fn register_nodes(registrar: AccountId, owner: AccountId, num_of_nodes: u8) -> AccountId {
    let reward_period = <RewardPeriod<TestRuntime>>::get().current;

    for i in 0..num_of_nodes {
        register_node_and_send_heartbeat(registrar, owner.clone(), reward_period, i);
    }

    let this_node = TestAccount::new([0 as u8; 32]).account_id();
    let this_node_signing_key = 0;

    set_ocw_node_id(this_node);
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(this_node_signing_key)]);

    return this_node
}

fn register_node_and_send_heartbeat(
    registrar: AccountId,
    owner: AccountId,
    reward_period: RewardPeriodIndex,
    id: u8,
) -> AccountId {
    let node_id = TestAccount::new([id as u8; 32]).account_id();
    let signing_key_id = id + 1;

    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(registrar),
        node_id,
        owner,
        UintAuthorityId(signing_key_id as u64),
    ));

    incr_heartbeats(reward_period, vec![node_id], 1);
    node_id
}

fn incr_heartbeats(reward_period: RewardPeriodIndex, nodes: Vec<NodeId<TestRuntime>>, uptime: u64) {
    for node in nodes {
        let _ = <NodeRegistry<TestRuntime>>::get(&node).unwrap();
        let weight = HEARTBEAT_BASE_WEIGHT.saturating_mul(uptime.into());

        <NodeUptime<TestRuntime>>::mutate(&reward_period, &node, |maybe_info| {
            if let Some(info) = maybe_info.as_mut() {
                info.count = info.count.saturating_add(uptime);
                info.last_reported = System::block_number();
                info.weight = info.weight.saturating_add(weight);
            } else {
                *maybe_info = Some(UptimeInfo {
                    count: uptime,
                    last_reported: System::block_number(),
                    weight,
                });
            }
        });

        <TotalUptime<TestRuntime>>::mutate(&reward_period, |total| {
            total.total_heartbeats = total.total_heartbeats.saturating_add(uptime);
            total.total_weight = total.total_weight.saturating_add(weight);
        });
    }
}

fn pop_payment_tx_from_mempool(pool_state: Arc<RwLock<PoolState>>) -> Extrinsic {
    let mut found_tx = None;
    while !pool_state.read().transactions.is_empty() {
        let tx = pop_tx_from_mempool(pool_state.clone());
        if matches!(
            tx.call,
            RuntimeCall::NodeManager(crate::Call::offchain_pay_nodes {
                reward_period_index: _,
                author: _,
                signature: _,
            })
        ) {
            found_tx = Some(tx);
            break
        }
    }

    assert!(found_tx.is_some(), "No offchain_pay_nodes transaction found in mempool");

    found_tx.unwrap()
}

fn pop_tx_from_mempool(pool_state: Arc<RwLock<PoolState>>) -> Extrinsic {
    let tx = pool_state.write().transactions.pop().unwrap();
    Extrinsic::decode(&mut &*tx).unwrap()
}

fn set_ocw_node_id(node_id: AccountId) {
    let storage = StorageValueRef::persistent(REGISTERED_NODE_KEY);
    storage
        .mutate(|r: Result<Option<AccountId>, StorageRetrievalError>| match r {
            Ok(Some(_)) => Ok(node_id),
            Ok(None) => Ok(node_id),
            _ => Err(()),
        })
        .unwrap();
}

fn remove_ocw_run_lock() {
    let key = [OCW_ID.as_slice(), b"::last_run"].concat();
    let mut storage = StorageValueRef::persistent(&key);
    storage.clear();
}

mod reward {
    use super::*;

    #[test]
    fn payment_transaction_succeed() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // make sure the pot has the expected amount
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount * 2u128
            );

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            assert_eq!(
                <RewardPot<TestRuntime>>::get(reward_period_to_pay).unwrap().total_reward,
                reward_amount
            );
            assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), reward_amount);

            // mock finalised block response
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            // Trigger ocw and send the transaction
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // Check if the transaction from the mempool is what we expected
            assert!(matches!(
                tx.call,
                RuntimeCall::NodeManager(crate::Call::offchain_pay_nodes {
                    reward_period_index: _,
                    author: _,
                    signature: _,
                })
            ));

            assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
            assert_eq!(
                true,
                <NodeUptime<TestRuntime>>::iter_prefix(reward_period_to_pay).next().is_none()
            );
            assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_none());
            // The owner has received the reward in free balance (auto-stake retired in PR1).
            let reward_fee = <RewardFeePercentage<TestRuntime>>::get() * reward_amount;
            let net_reward = reward_amount - reward_fee;
            assert_eq!(Balances::free_balance(&context.owner), net_reward);
            // The pot has gone down by half
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount
            );
            // The outstanding rewards should be cleared
            assert_eq!(OutstandingRewardToPay::<TestRuntime>::get(), 0u128);

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn multiple_payments_can_be_triggered_in_the_same_block() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            // This takes 2 attempts to clear all the payments
            let node_count = <MaxBatchSize<TestRuntime>>::get() * 2;
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state.clone());
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // We should have processed the first batch of payments
            assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_some());
            let gross_owner_reward = reward_amount / 2;
            let owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_owner_reward;
            let expected_owner_reward = gross_owner_reward - owner_fee;
            assert_eq!(Balances::free_balance(&context.owner), expected_owner_reward);

            // This is a hack: we remove the lock to allow the offchain worker to run again for the
            // same block
            remove_ocw_run_lock();

            // Trigger another payment. In reality this can happy because authors can trigger
            // payments in parallel
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // This should complete the payment
            assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
            assert_eq!(
                true,
                <NodeUptime<TestRuntime>>::iter_prefix(reward_period_to_pay).next().is_none()
            );
            assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_none());
            let gross_owner_reward = reward_amount;
            let owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_owner_reward;
            let expected_owner_reward = gross_owner_reward - owner_fee;
            assert_eq!(Balances::free_balance(&context.owner), expected_owner_reward);
            // The pot has gone down by half
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount
            );

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn payment_is_based_on_uptime() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // make sure the pot has the expected amount
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount * 2u128
            );

            let new_owner = TestAccount::new([111u8; 32]).account_id();
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period_to_pay,
                199,
            );

            let total_expected_uptime = NodeManager::calculate_uptime_threshold(
                reward_period_length as u32,
                reward_period.heartbeat_period,
            );
            // The node falls below the min threshold to get the full rewards. They should still get
            // their share
            incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 2);

            let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);
            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            // Pay out
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));
            // The owner has received the reward
            // total_expected_uptime - 1 because we run the OCW
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128 - 1,
                total_uptime.total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Value {} and {} differs by more than 10",
                Balances::free_balance(&new_owner),
                expected_new_owner_reward
            );

            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <=
                    20,
                "Value {} differs by more than 20",
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
            );

            // The pot has gone down by half
            assert!(
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount) <=
                    20,
                "Value {} differs by more than 20",
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount)
            );

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn payment_works_when_uptime_is_threshold() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // make sure the pot has the expected amount
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount * 2u128
            );

            let new_owner = TestAccount::new([111u8; 32]).account_id();
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period_to_pay,
                199,
            );

            let total_expected_uptime = NodeManager::calculate_uptime_threshold(
                reward_period_length as u32,
                reward_period.heartbeat_period,
            );
            // The node's uptime is exactly the threshold, so they should get the full rewards
            incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 1);

            let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            // Pay out
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime.total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Values {} differ by more than 10",
                Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward)
            );
            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <=
                    100,
                "Value {}  differs by more than 100",
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
            );

            // The pot has gone down by half
            assert!(
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount) <=
                    100,
                "Value {} differs by more than 100",
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount)
            );

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn payment_works_even_when_uptime_is_over_threshold() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            let initial_pot = reward_amount * 2u128;
            // make sure the pot has the expected amount
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                initial_pot
            );

            let new_owner = TestAccount::new([111u8; 32]).account_id();
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period_to_pay,
                199,
            );

            let total_expected_uptime = NodeManager::calculate_uptime_threshold(
                reward_period_length as u32,
                reward_period.heartbeat_period,
            );
            // The node's uptime is over the threshold. This is unexpected but handled
            incr_heartbeats(
                reward_period_to_pay,
                vec![new_node],
                total_expected_uptime as u64 + 1u64,
            );

            let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

            // Complete a reward period
            roll_forward(reward_period_length - System::block_number());

            // Pay out
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            // The system limits the reward to the expected uptime
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime.total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 1,
                "Values {} and {} differ by more than 1",
                Balances::free_balance(&new_owner),
                expected_new_owner_reward,
            );
            //The old owner gets a smaller share of the rewards because the total_uptime has now
            // increased by the extra uptime
            let gross_old_owner_reward =
                Perquintill::from_rational(1u128, total_uptime.total_heartbeats as u128) *
                    reward_amount *
                    (node_count as u128);
            let old_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) < 1,
                "Value {} differs by more than 1",
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
            );

            // The pot should have gone down by half (because we started with reward_amount * 2),
            // but it hasn't because it didn't pay out the full reward.
            // This is because one of the nodes went over the expected uptime, which increased the
            // total uptime But we limit how much a node can get paid based on the
            // expected uptime. This is a safeguard against paying out more than the
            // expected amount if nodes somehow manipulate their uptime.
            assert!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()) > reward_amount
            );

            // Make sure the pot has gone down by the expected amount
            assert!(
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(initial_pot - (gross_new_owner_reward + gross_old_owner_reward)) <
                    10,
                "Value {} and {} differs by more than 10",
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                initial_pot - (gross_new_owner_reward + gross_old_owner_reward)
            );

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn threshold_update_is_respected() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
            let context = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = reward_period.reward_amount;
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // make sure the pot has the expected amount
            assert_eq!(
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                reward_amount * 2u128
            );

            let new_owner = TestAccount::new([111u8; 32]).account_id();
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period_to_pay,
                199,
            );
            let total_expected_uptime = NodeManager::calculate_uptime_threshold(
                reward_period_length as u32,
                reward_period.heartbeat_period,
            );
            // Increase the uptime of the node by 4 (total 5) to change the rewards
            incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 1);

            let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

            // Set a new threshold before rolling forward. This updates config for the next period
            // only and must not affect payout for the current snapshotted period.
            MinUptimeThreshold::<TestRuntime>::put(Perbill::from_percent(5));

            assert_eq!(RewardPeriod::<TestRuntime>::get().uptime_threshold, total_expected_uptime);

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            // Pay out
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_payment_tx_from_mempool(pool_state);
            assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime.total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Values {} and {} differ by more than 10",
                Balances::free_balance(&new_owner),
                expected_new_owner_reward
            );
            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <RewardFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <=
                    100,
                "Value {} differs by more than 100",
                Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
            );

            // The pot has gone down by half
            assert!(
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount) <=
                    100,
                "Value {} differs by more than 100",
                Balances::free_balance(&NodeManager::compute_reward_account_id())
                    .abs_diff(reward_amount)
            );

            System::assert_last_event(
                Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
            );
        });
    }

    #[test]
    fn threshold_update_applies_to_next_period_only() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let current_reward_period = <RewardPeriod<TestRuntime>>::get();
            let current_period_index = current_reward_period.current;
            let current_period_length = current_reward_period.length;
            let current_uptime_threshold = current_reward_period.uptime_threshold;

            let new_min_threshold = Perbill::from_percent(5);

            // Change the configured min threshold during the current period.
            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::MinUptimeThreshold(new_min_threshold),
            ));

            // The stored config changes immediately...
            assert_eq!(MinUptimeThreshold::<TestRuntime>::get(), Some(new_min_threshold));

            // ...but the current reward period snapshot must stay unchanged.
            let reward_period_after_config = <RewardPeriod<TestRuntime>>::get();
            assert_eq!(reward_period_after_config.current, current_period_index);
            assert_eq!(reward_period_after_config.length, current_period_length);
            assert_eq!(reward_period_after_config.uptime_threshold, current_uptime_threshold);

            // Roll into the next period.
            roll_forward((current_period_length as u64 - System::block_number()) + 1);

            let next_reward_period = <RewardPeriod<TestRuntime>>::get();
            let expected_next_uptime_threshold = NodeManager::calculate_uptime_threshold(
                next_reward_period.length,
                next_reward_period.heartbeat_period,
            );

            assert_eq!(next_reward_period.current, current_period_index + 1);
            assert_eq!(next_reward_period.length, current_period_length);
            assert_eq!(next_reward_period.uptime_threshold, expected_next_uptime_threshold);

            // And the threshold should actually have changed for the new period.
            assert_ne!(next_reward_period.uptime_threshold, current_uptime_threshold);
        });
    }

    // PR1 retires staking and the genesis-bonus weight multiplier. The original
    // `reward_share_increases_with_genesis_and_stake_bonus` test (which asserted a
    // 4.5x weight from 50% genesis bonus + 3x stake) is gone with those features.

    #[test]
    fn zero_reward_works() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let context = Context::new(1 as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            let signature =
                UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
            let author = mock::AVN::active_validators()[0].clone();
            // Remove uptime for the node to make the reward 0
            let node_id = context.ocw_node;

            <NodeUptime<TestRuntime>>::mutate(&reward_period_to_pay, &node_id, |maybe_info| {
                if let Some(info) = maybe_info.as_mut() {
                    info.count = 0;
                    info.last_reported = 0;
                    info.weight = 0;
                }
            });

            assert_ok!(NodeManager::offchain_pay_nodes(
                RawOrigin::None.into(),
                reward_period_to_pay,
                author,
                signature
            ));

            System::assert_has_event(
                Event::RewardPaid {
                    reward_period: reward_period_to_pay,
                    owner: context.owner,
                    node: context.ocw_node,
                    amount: 0,
                }
                .into(),
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn when_period_is_wrong() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_authors()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let node_count = <MaxBatchSize<TestRuntime>>::get();
                let _ = Context::new(node_count as u8);
                let reward_period = <RewardPeriod<TestRuntime>>::get();
                let reward_period_length = reward_period.length as u64;
                let bad_reward_period_to_pay = reward_period.current + 10;

                // Complete a reward period
                roll_forward((reward_period_length - System::block_number()) + 1);

                let signature =
                    UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
                let author = mock::AVN::active_validators()[0].clone();
                assert_noop!(
                    NodeManager::offchain_pay_nodes(
                        RawOrigin::None.into(),
                        bad_reward_period_to_pay,
                        author,
                        signature
                    ),
                    Error::<TestRuntime>::InvalidRewardPaymentRequest
                );
            });
        }

        #[test]
        fn when_pot_balance_is_not_enough() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_authors()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let node_count = <MaxBatchSize<TestRuntime>>::get();
                let _ = Context::new(node_count as u8);
                let reward_period = <RewardPeriod<TestRuntime>>::get();
                let reward_amount = reward_period.reward_amount;
                let reward_period_length = reward_period.length as u64;
                let reward_period_to_pay = reward_period.current;

                // Complete a reward period
                roll_forward((reward_period_length - System::block_number()) + 1);

                let signature =
                    UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
                let author = mock::AVN::active_validators()[0].clone();
                // ensure there isn't enough to pay out
                Balances::make_free_balance_be(
                    &NodeManager::compute_reward_account_id(),
                    reward_amount - 10000u128,
                );

                assert_noop!(
                    NodeManager::offchain_pay_nodes(
                        RawOrigin::None.into(),
                        reward_period_to_pay,
                        author,
                        signature
                    ),
                    Error::<TestRuntime>::InsufficientBalanceForReward
                );
            });
        }

        #[test]
        fn rewards_are_disabled() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_authors()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let node_count = <MaxBatchSize<TestRuntime>>::get();
                let _ = Context::new(node_count as u8);

                //Disable rewards
                RewardEnabled::<TestRuntime>::put(false);

                let reward_period = <RewardPeriod<TestRuntime>>::get();
                let reward_period_length = reward_period.length as u64;

                // Complete a reward period
                roll_forward((reward_period_length - System::block_number()) + 1);

                let call = crate::Call::offchain_pay_nodes {
                    reward_period_index: 1u64,
                    author: mock::AVN::active_validators()[0].clone(),
                    signature: UintAuthorityId(1u64)
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
        fn unsigned_calls_are_not_local() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_authors()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let reward_period = <RewardPeriod<TestRuntime>>::get();
                let reward_period_length = reward_period.length as u64;

                // Complete a reward period
                roll_forward((reward_period_length - System::block_number()) + 1);

                let call = crate::Call::offchain_pay_nodes {
                    reward_period_index: 1u64,
                    author: mock::AVN::active_validators()[0].clone(),
                    signature: UintAuthorityId(1u64)
                        .sign(&("DummyProof").encode())
                        .expect("Error signing"),
                };

                assert_noop!(
                    <NodeManager as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::External,
                        &call
                    ),
                    InvalidTransaction::Call
                );
            });
        }

        #[test]
        fn fails_when_reward_pot_not_found() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_authors()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let node_count = <MaxBatchSize<TestRuntime>>::get();
                let _ = Context::new(node_count as u8);
                let reward_period = <RewardPeriod<TestRuntime>>::get();
                let reward_period_length = reward_period.length as u64;
                let reward_period_to_pay = reward_period.current;

                // Complete a reward period
                roll_forward((reward_period_length - System::block_number()) + 1);

                let signature =
                    UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
                let author = mock::AVN::active_validators()[0].clone();
                // Remove the reward pot to simulate the error condition
                <RewardPot<TestRuntime>>::remove(reward_period_to_pay);

                assert_noop!(
                    NodeManager::offchain_pay_nodes(
                        RawOrigin::None.into(),
                        reward_period_to_pay,
                        author,
                        signature
                    ),
                    Error::<TestRuntime>::RewardPotNotFound
                );
            });
        }
    }
}

mod end_2_end {
    use super::*;

    fn complete_reward_period_and_pay(
        pool_state: Arc<RwLock<PoolState>>,
        offchain_state: Arc<RwLock<OffchainState>>,
    ) {
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;

        // Complete a reward period
        roll_forward(reward_period_length + 1);

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_payment_tx_from_mempool(pool_state.clone());
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));
    }

    fn increase_timestamp_by(seconds: u64) {
        let now: u64 = Timestamp::now().as_secs();
        Timestamp::set_timestamp((now + seconds) * 1000);
    }

    fn set_timestamp(target_sec: u64) -> Result<(), ()> {
        let now = Timestamp::now().as_secs();
        if target_sec < now {
            return Err(())
        }
        Timestamp::set_timestamp(target_sec * 1000);
        Ok(())
    }

}

mod next_mint_amount_to_request {
    use super::*;

    const REWARD: u128 = 100;
    // num_periods_to_mint. window = N * REWARD = 200
    const N: u32 = 2;

    fn setup(pot_balance: u128, outstanding: u128) {
        <NextRewardAmountPerPeriod<TestRuntime>>::put(REWARD);
        <NumPeriodsToMint<TestRuntime>>::put(N);
        <OutstandingRewardToPay<TestRuntime>>::put(outstanding);
        Balances::make_free_balance_be(&NodeManager::compute_reward_account_id(), pot_balance);
    }

    #[test]
    fn returns_none_when_pending_mint_request_exists() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                setup(0, 0);
                PendingMintRequestState::<TestRuntime>::put(PendingMintRequest {
                    tx_id: 1u32,
                    amount: 100u128,
                    bridge_confirmed: false,
                    credit_received: false,
                });

                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }

    #[test]
    fn returns_none_when_num_periods_to_mint_is_zero() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                setup(0, 0);
                <NumPeriodsToMint<TestRuntime>>::put(0u32);

                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }

    #[test]
    fn returns_none_when_reward_amount_per_period_is_zero() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                setup(0, 0);
                <NextRewardAmountPerPeriod<TestRuntime>>::put(0u128);

                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }

    #[test]
    fn returns_none_when_pot_equals_window_with_no_outstanding() {
        // pot == window: exactly funded for N periods, no mint needed
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                setup(window, 0);

                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }

    #[test]
    fn returns_none_when_pot_exceeds_window_with_no_outstanding() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                setup(window + 1, 0);

                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }

    // on_initialize adds the just-ended period's reward to
    // outstanding_to_pay before the OCW calls this function. Outstanding obligations
    // raise the refill_threshold, so even a fully-funded pot triggers a mint when there
    // is unpaid outstanding.
    #[test]
    fn triggers_mint_at_period_boundary_when_pot_is_funded_but_outstanding_exists() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                let outstanding = REWARD; // 100
                                          // Simulate: on_initialize just added one period to outstanding, pot unchanged
                                          // refill_threshold = outstanding + window = 300 > pot (200) → mint triggered
                setup(window, outstanding);

                // target = outstanding + 2*window = 500; mint = 500 - 200 = 300
                assert_eq!(NodeManager::next_mint_amount_to_request(), Some(outstanding + window));
            });
    }

    #[test]
    fn triggers_mint_when_pot_exceeds_window_but_outstanding_raises_threshold() {
        // pot > window but refill_threshold = outstanding + window > pot, so mint is still needed
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                let pot_balance = window + 50; // 250
                let outstanding = window; // 200 — many periods unpaid
                setup(pot_balance, outstanding);

                // target = outstanding + 2*window = 600; mint = 600 - 250 = 350
                let expected_mint = outstanding + 2 * window - pot_balance;
                assert_eq!(NodeManager::next_mint_amount_to_request(), Some(expected_mint));
            });
    }

    #[test]
    fn returns_mint_amount_when_pot_is_below_window_with_no_outstanding() {
        // pot < window -> mint to 0 + 2*window; amount = 2*window - pot
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                let pot = window - 50; // 150
                setup(pot, 0);

                let expected_mint = 2 * window - pot; // 250
                assert_eq!(NodeManager::next_mint_amount_to_request(), Some(expected_mint));
            });
    }

    #[test]
    fn mint_amount_covers_outstanding_plus_two_windows_when_pot_is_below_window() {
        // When a mint is needed the target is outstanding + 2*window, so after outstanding
        // obligations drain the pot the N-period buffer is fully restored.
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                let outstanding = REWARD; // 100
                let pot = window - 50; // 150 — below window, so mint is triggered
                setup(pot, outstanding);

                // (100 + 400) - 150 = 350
                let expected_mint = outstanding + 2 * window - pot;
                assert_eq!(NodeManager::next_mint_amount_to_request(), Some(expected_mint));
            });
    }

    #[test]
    fn mint_amount_accounts_for_all_accumulated_outstanding_periods() {
        // Multiple accumulated unpaid periods must all be reflected in the mint target.
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let window = REWARD * N as u128; // 200
                let outstanding = REWARD * 3; // 300 – three periods unpaid
                let pot = window - 1; // 199 — just below window
                setup(pot, outstanding);

                // (300 + 400) - 199 = 501
                let expected_mint = outstanding + 2 * window - pot;
                assert_eq!(NodeManager::next_mint_amount_to_request(), Some(expected_mint));
            });
    }

    #[test]
    fn returns_none_when_mint_amount_exceeds_safety_cap() {
        // Safety cap: mint_amount must not exceed MINT_SAFETY_CAP_MULTIPLIER * runway.
        // This triggers when outstanding >> pot (e.g. bridge has stalled for many periods).
        // mint_amount = outstanding + 2*runway - pot
        // max_mint = MINT_SAFETY_CAP_MULTIPLIER * runway
        // Exceeds cap when: outstanding > (MINT_SAFETY_CAP_MULTIPLIER - 2) * runway + pot
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let runway = REWARD * N as u128; // 200
                let max_mint = runway * MINT_SAFETY_CAP_MULTIPLIER as u128; // 4 * 200 = 800
                let outstanding = max_mint + 1; // just above the cap to trigger the safety check
                let pot = 0;
                setup(pot, outstanding);

                // mint_amount = outstanding + 2*runway - pot = 801 + 400 - 0 = 1201, but max_mint =
                // 800
                assert_eq!(NodeManager::next_mint_amount_to_request(), None);
            });
    }
}
