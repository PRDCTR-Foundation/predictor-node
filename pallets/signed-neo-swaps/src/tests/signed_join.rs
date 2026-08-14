use super::*;
use prediction_market_primitives::{test_helper::TestAccount, types::SignatureTest};
use sp_avn_common::Proof;
use sp_core::Pair;

fn create_signed_join_proof(
    who: &TestAccount,
    pool_id: &MarketId,
    pool_shares: &BalanceOf<Runtime>,
    max_amounts_in: &Vec<BalanceOf<Runtime>>,
) -> Proof<SignatureTest, TestAccountIdPK> {
    let relayer = eve();
    let block_number = System::block_number();
    let encoded_payload = encode_signed_join_params::<Runtime>(
        &relayer,
        pool_id,
        pool_shares,
        max_amounts_in,
        &block_number,
    );

    let signature = SignatureTest::from(who.key_pair().sign(&encoded_payload));
    Proof { signer: who.key_pair().public(), relayer, signature }
}

struct SignedJoinContext {
    pub pool_id: MarketId,
    pub pool_shares_amount: u128,
    pub outcomes: Vec<AssetOf<Runtime>>,
}

impl Default for SignedJoinContext {
    fn default() -> Self {
        let liquidity = _5;
        let spot_prices = vec![_1_6, _5_6 + 1];
        let pool_id = create_market_and_deploy_pool(
            alice(),
            BASE_ASSET,
            MarketType::Scalar(0..=1),
            liquidity,
            spot_prices.clone(),
            CENT_BASE,
        );

        Self {
            pool_id,
            pool_shares_amount: _4, // Add 40% to the pool
            outcomes: vec![],
        }
    }
}

impl SignedJoinContext {
    fn setup_market(&mut self) {
        // Make sure the market is Active for pool operations to work
        MarketCommons::mutate_market(&self.pool_id, |market| {
            market.status = MarketStatus::Active;
            Ok(())
        })
        .unwrap();

        self.outcomes = NeoSwaps::assets(self.pool_id).unwrap();
    }

    fn prepare_outcome_tokens(&self, who: &TestAccountIdPK, amount: BalanceOf<Runtime>) {
        // Acquire outcome tokens for the test account to deposit into the pool
        deposit_complete_set(self.pool_id, *who, amount);
    }
}

#[test]
fn signed_join_works() {
    ExtBuilder::default().build().execute_with(|| {
        let mut context = SignedJoinContext::default();
        let bob_account = TestAccount::new([1; 32]);

        let pool_id = context.pool_id;
        let pool_shares_amount = _4;

        context.setup_market();

        // Prepare outcome tokens for Bob to join the pool
        context.prepare_outcome_tokens(&bob(), pool_shares_amount * 2);

        // Calculate expected amounts in based on pool state
        // Max amounts set high to ensure the test passes
        let max_amounts_in = vec![u128::MAX, u128::MAX];

        let proof_blocknumber = System::block_number();
        let proof =
            create_signed_join_proof(&bob_account, &pool_id, &pool_shares_amount, &max_amounts_in);

        // Initial balances to verify later
        let bob_initial_balances = [
            <Runtime as Config>::MultiCurrency::free_balance(context.outcomes[0], &bob()),
            <Runtime as Config>::MultiCurrency::free_balance(context.outcomes[1], &bob()),
        ];

        // Execute signed join
        assert_ok!(SignedNeoSwaps::signed_join(
            RuntimeOrigin::signed(bob()),
            proof,
            pool_id,
            pool_shares_amount,
            max_amounts_in,
            proof_blocknumber
        ));

        // Check that Bob's outcome tokens were reduced
        let bob_final_balances = [
            <Runtime as Config>::MultiCurrency::free_balance(context.outcomes[0], &bob()),
            <Runtime as Config>::MultiCurrency::free_balance(context.outcomes[1], &bob()),
        ];

        // Bob's balances should be less after joining
        assert!(bob_final_balances[0] < bob_initial_balances[0]);
        assert!(bob_final_balances[1] < bob_initial_balances[1]);

        let expected_amounts_in = vec![
            bob_initial_balances[0] - bob_final_balances[0],
            bob_initial_balances[1] - bob_final_balances[1],
        ];
        assert!(System::events().iter().any(|record| {
            matches!(
                &record.event,
                RuntimeEvent::NeoSwaps(NeoSwapsEvent::JoinExecuted {
                    who,
                    pool_id: event_pool_id,
                    pool_shares_amount: event_pool_shares_amount,
                    amounts_in,
                    ..
                }) if *who == bob()
                    && *event_pool_id == pool_id
                    && *event_pool_shares_amount == pool_shares_amount
                    && amounts_in == &expected_amounts_in
            )
        }));
    });
}

mod fails_when {
    use super::*;

    #[test]
    fn proof_has_wrong_relayer() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();

            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            // Create proof with wrong relayer (dave instead of eve)
            let proof = Proof {
                relayer: dave(),
                ..create_signed_join_proof(
                    &bob_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &max_amounts_in,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test]
    fn proof_data_mismatch_signature() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();

            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            // Create proof with incorrect signature
            let proof = Proof {
                signature: bob_account.key_pair().sign(&[1u8; 10]),
                ..create_signed_join_proof(
                    &bob_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &max_amounts_in,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test]
    fn proof_data_mismatch_signer() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();

            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            // Create proof with wrong signer
            let proof = Proof {
                signer: dave(),
                ..create_signed_join_proof(
                    &bob_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &max_amounts_in,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                Error::<Runtime>::SenderIsNotSigner
            );
        });
    }

    #[test]
    fn proof_has_expired() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();

            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            let proof = create_signed_join_proof(
                &bob_account,
                &pool_id,
                &context.pool_shares_amount,
                &max_amounts_in,
            );

            // Advance blocks to expire the transaction
            System::set_block_number(proof_blocknumber + 100);

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                Error::<Runtime>::SignedTransactionExpired
            );
        });
    }

    #[test]
    fn insufficient_outcome_tokens() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();

            // Don't prepare enough outcome tokens (only prepare half of what's needed)
            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount / 2);

            // Set max_amounts_in to very small values that will cause the test to fail
            let max_amounts_in = vec![1, 1];
            let proof_blocknumber = System::block_number();

            let proof = create_signed_join_proof(
                &bob_account,
                &pool_id,
                &context.pool_shares_amount,
                &max_amounts_in,
            );

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                NeoSwapsError::<Runtime>::AmountInAboveMax
            );
        });
    }

    #[test]
    fn market_not_active() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;

            // First set up with Active status to ensure pool is properly initialized
            context.setup_market();

            // Prepare outcome tokens while the market is still active
            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            // Then change market status to a non-active status
            MarketCommons::mutate_market(&pool_id, |market| {
                market.status = MarketStatus::Disputed;
                Ok(())
            })
            .unwrap();

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            let proof = create_signed_join_proof(
                &bob_account,
                &pool_id,
                &context.pool_shares_amount,
                &max_amounts_in,
            );

            // Test that it fails with MarketNotActive error
            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    max_amounts_in,
                    proof_blocknumber
                ),
                NeoSwapsError::<Runtime>::MarketNotActive
            );
        });
    }

    #[test]
    fn zero_pool_shares_amount() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();
            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            // Create proof with zero pool shares
            let zero_pool_shares: BalanceOf<Runtime> = 0;
            let proof = create_signed_join_proof(
                &bob_account,
                &pool_id,
                &zero_pool_shares,
                &max_amounts_in,
            );

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    zero_pool_shares,
                    max_amounts_in,
                    proof_blocknumber
                ),
                NeoSwapsError::<Runtime>::ZeroAmount
            );
        });
    }

    #[test]
    fn position_too_small() {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedJoinContext::default();
            let bob_account = TestAccount::new([1; 32]);

            let pool_id = context.pool_id;
            context.setup_market();
            context.prepare_outcome_tokens(&bob(), context.pool_shares_amount * 2);

            let max_amounts_in = vec![u128::MAX, u128::MAX];
            let proof_blocknumber = System::block_number();

            // Create a very small position
            let tiny_position: BalanceOf<Runtime> = 1;
            let proof =
                create_signed_join_proof(&bob_account, &pool_id, &tiny_position, &max_amounts_in);

            assert_noop!(
                SignedNeoSwaps::signed_join(
                    RuntimeOrigin::signed(bob()),
                    proof,
                    pool_id,
                    tiny_position,
                    max_amounts_in,
                    proof_blocknumber
                ),
                NeoSwapsError::<Runtime>::MinRelativeLiquidityThresholdViolated
            );
        });
    }
}
