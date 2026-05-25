// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok, BoundedVec};
use frame_system::RawOrigin;
use sp_avn_common::Proof;
use sp_runtime::traits::IdentifyAccount;

const SIGNED_TX_LIFETIME: u32 = 64;

fn setup_registrar() -> AccountId {
    let registrar = TestAccount::new([1u8; 32]).account_id();
    NodeRegistrar::<TestRuntime>::set(Some(registrar));
    registrar
}

fn register_node_for(
    registrar: AccountId,
    owner: AccountId,
    node_seed: u8,
    key_seed: u8,
) -> AccountId {
    let node = TestAccount::new([node_seed; 32]).account_id();
    let key = UintAuthorityId(key_seed as u64);
    assert_ok!(NodeManager::register_node(
        RawOrigin::Signed(registrar).into(),
        node,
        owner,
        key,
    ));
    node
}

/// Build a Proof whose `signer` field holds the prover NodeId. The mock's
/// `Signature` is `sr25519::Signature`; verify_signature relies on the
/// signature matching the signer over the encoded payload. For tests we
/// construct a real sr25519 signature from a `TestAccount` so the validation
/// path runs end-to-end.
fn build_proof(prover_seed: u8, relayer: AccountId, payload: &[u8]) -> Proof<Signature, AccountId> {
    let prover = TestAccount::new([prover_seed; 32]);
    let signer = prover.account_id();
    let signature = prover.key_pair().sign(payload);
    Proof { signer, relayer, signature }
}

fn payload(
    relayer: &AccountId,
    nodes: &BoundedVec<NodeId<TestRuntime>, MaxNodesPerAggregateHeartbeat>,
    block_number: BlockNumberFor<TestRuntime>,
) -> Vec<u8> {
    encode_aggregate_heartbeat_params::<TestRuntime>(
        relayer,
        nodes,
        &(nodes.len() as u32),
        &block_number,
    )
}

#[test]
fn happy_path_one_node() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([10u8; 32]).account_id();

        // Owner has a single registered node which is also the prover.
        let prover_seed = 11u8;
        let prover = register_node_for(registrar, owner, prover_seed, 21);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_ok!(NodeManager::heartbeat_for_owned_nodes(
            RawOrigin::Signed(prover).into(),
            proof,
            nodes,
            bn,
        ));

        let period = RewardPeriod::<TestRuntime>::get().current;
        let info = NodeUptime::<TestRuntime>::get(period, prover).expect("uptime recorded");
        assert_eq!(info.count, 1);
        assert_eq!(info.weight, HEARTBEAT_BASE_WEIGHT);

        let total = TotalUptime::<TestRuntime>::get(period);
        assert_eq!(total.total_heartbeats, 1);
        assert_eq!(total.total_weight, HEARTBEAT_BASE_WEIGHT);
    });
}

#[test]
fn happy_path_three_nodes_same_owner() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([20u8; 32]).account_id();

        let prover_seed = 30u8;
        let prover = register_node_for(registrar, owner, prover_seed, 40);
        let n2 = register_node_for(registrar, owner, 31, 41);
        let n3 = register_node_for(registrar, owner, 32, 42);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover, n2, n3]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_ok!(NodeManager::heartbeat_for_owned_nodes(
            RawOrigin::Signed(prover).into(),
            proof,
            nodes,
            bn,
        ));

        let period = RewardPeriod::<TestRuntime>::get().current;
        for n in [prover, n2, n3] {
            let info = NodeUptime::<TestRuntime>::get(period, n).expect("uptime recorded");
            assert_eq!(info.count, 1);
        }
        let total = TotalUptime::<TestRuntime>::get(period);
        assert_eq!(total.total_heartbeats, 3);
        assert_eq!(total.total_weight, HEARTBEAT_BASE_WEIGHT.saturating_mul(3));
    });
}

#[test]
fn duplicate_nodes_in_batch_are_silently_deduped() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([50u8; 32]).account_id();
        let prover_seed = 60u8;
        let prover = register_node_for(registrar, owner, prover_seed, 70);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover, prover, prover]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_ok!(NodeManager::heartbeat_for_owned_nodes(
            RawOrigin::Signed(prover).into(),
            proof,
            nodes,
            bn,
        ));

        let period = RewardPeriod::<TestRuntime>::get().current;
        let info = NodeUptime::<TestRuntime>::get(period, prover).expect("uptime recorded");
        assert_eq!(info.count, 1, "duplicates should be deduped");
        let total = TotalUptime::<TestRuntime>::get(period);
        assert_eq!(total.total_heartbeats, 1);
    });
}

#[test]
fn rejects_prover_not_registered() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        setup_registrar();
        // No registration for prover_seed; lookup will return None.
        let prover_seed = 80u8;
        let prover = TestAccount::new([prover_seed; 32]).account_id();

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(prover).into(),
                proof,
                nodes,
                bn,
            ),
            Error::<TestRuntime>::ProverNotRegistered
        );
    });
}

#[test]
fn rejects_node_owned_by_different_account() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner_a = TestAccount::new([90u8; 32]).account_id();
        let owner_b = TestAccount::new([91u8; 32]).account_id();

        let prover_seed = 92u8;
        let prover = register_node_for(registrar, owner_a, prover_seed, 100);
        // A node owned by a different account.
        let foreign = register_node_for(registrar, owner_b, 93, 101);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover, foreign]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(prover).into(),
                proof,
                nodes,
                bn,
            ),
            Error::<TestRuntime>::NodeNotOwnedByProver
        );
    });
}

#[test]
fn rejects_unregistered_node_in_batch() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([110u8; 32]).account_id();
        let prover_seed = 111u8;
        let prover = register_node_for(registrar, owner, prover_seed, 120);
        // Made up an account that was never registered.
        let phantom = TestAccount::new([200u8; 32]).account_id();

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover, phantom]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(prover).into(),
                proof,
                nodes,
                bn,
            ),
            Error::<TestRuntime>::NodeNotOwnedByProver
        );
    });
}

#[test]
fn rejects_sender_mismatch() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([130u8; 32]).account_id();
        let prover_seed = 131u8;
        let prover = register_node_for(registrar, owner, prover_seed, 140);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        // Send from a different account than proof.signer.
        let imposter = TestAccount::new([200u8; 32]).account_id();
        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(imposter).into(),
                proof,
                nodes,
                bn,
            ),
            Error::<TestRuntime>::SenderIsNotSigner
        );
    });
}

#[test]
fn rejects_bad_signature() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([150u8; 32]).account_id();
        let prover_seed = 151u8;
        let prover = register_node_for(registrar, owner, prover_seed, 160);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        let bn = System::block_number();

        // Sign a payload that doesn't match what the runtime will reconstruct
        // (different relayer field).
        let wrong_relayer = TestAccount::new([88u8; 32]).account_id();
        let bad_payload = payload(&wrong_relayer, &nodes, bn);
        let proof = build_proof(prover_seed, relayer, &bad_payload);

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(prover).into(),
                proof,
                nodes,
                bn,
            ),
            Error::<TestRuntime>::UnauthorizedSignedTransaction
        );
    });
}

#[test]
fn rejects_expired_block_number() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([170u8; 32]).account_id();
        let prover_seed = 171u8;
        let prover = register_node_for(registrar, owner, prover_seed, 180);

        // Advance well past the SignedTxLifetime window.
        roll_forward(2 * SIGNED_TX_LIFETIME as u64 + 10);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        // Sign a block_number that's now far in the past.
        let stale_bn = 1u64;
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, stale_bn));

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::Signed(prover).into(),
                proof,
                nodes,
                stale_bn,
            ),
            Error::<TestRuntime>::SignedTransactionExpired
        );
    });
}

#[test]
fn rejects_unsigned_origin() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([190u8; 32]).account_id();
        let prover_seed = 191u8;
        let prover = register_node_for(registrar, owner, prover_seed, 200);

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(vec![prover]).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_noop!(
            NodeManager::heartbeat_for_owned_nodes(
                RawOrigin::None.into(),
                proof,
                nodes,
                bn,
            ),
            sp_runtime::DispatchError::BadOrigin,
        );
    });
}

/// Smoke-test the 1000-node path end-to-end in unit-test land. Validates the
/// O(N log N) BTreeSet dedup and N storage writes complete within a sane
/// timeframe (unit tests timeout at 60s by default).
#[test]
fn happy_path_one_thousand_nodes() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let registrar = setup_registrar();
        let owner = TestAccount::new([220u8; 32]).account_id();

        let prover_seed = 221u8;
        let prover = register_node_for(registrar, owner, prover_seed, 0);

        // Register 999 more nodes owned by the same account.
        let mut all_nodes: Vec<AccountId> = vec![prover];
        for i in 1..1000u32 {
            // Distinct seed-byte tuples to keep AccountIds unique.
            let lo = (i & 0xff) as u8;
            let hi = ((i >> 8) & 0xff) as u8;
            let seed = [222, lo, hi, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let key_seed = (i as u64) + 1_000_000;
            let node = AccountId::from_raw(TestAccount::new(seed).key_pair().public().0);
            let _ = node;
            // Use the helper that returns AccountId for the seed pattern we used elsewhere.
            let node = TestAccount::new(seed).account_id();
            assert_ok!(NodeManager::register_node(
                RawOrigin::Signed(registrar).into(),
                node,
                owner,
                UintAuthorityId(key_seed),
            ));
            all_nodes.push(node);
        }

        let relayer = TestAccount::new([99u8; 32]).account_id();
        let nodes: BoundedVec<_, MaxNodesPerAggregateHeartbeat> =
            BoundedVec::try_from(all_nodes.clone()).unwrap();
        let bn = System::block_number();
        let proof = build_proof(prover_seed, relayer, &payload(&relayer, &nodes, bn));

        assert_ok!(NodeManager::heartbeat_for_owned_nodes(
            RawOrigin::Signed(prover).into(),
            proof,
            nodes,
            bn,
        ));

        let period = RewardPeriod::<TestRuntime>::get().current;
        let total = TotalUptime::<TestRuntime>::get(period);
        assert_eq!(total.total_heartbeats, 1000);
        assert_eq!(total.total_weight, HEARTBEAT_BASE_WEIGHT.saturating_mul(1000));

        // Spot-check a few nodes in storage.
        for &node in &[all_nodes[0], all_nodes[250], all_nodes[500], all_nodes[999]] {
            let info = NodeUptime::<TestRuntime>::get(period, node).expect("uptime recorded");
            assert_eq!(info.count, 1);
        }
    });
}
