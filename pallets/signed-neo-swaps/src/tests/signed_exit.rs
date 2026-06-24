use super::*;
use prediction_market_primitives::{test_helper::TestAccount, types::SignatureTest};
use sp_avn_common::Proof;
use sp_core::Pair;
use test_case::test_case;

fn create_signed_exit_proof(
    who: &TestAccount,
    pool_id: &MarketId,
    pool_shares: &BalanceOf<Runtime>,
    min_amounts_out: &Vec<BalanceOf<Runtime>>,
) -> Proof<SignatureTest, TestAccountIdPK> {
    let relayer = eve();
    let block_number = System::block_number();
    let encoded_payload = encode_signed_exit_params::<Runtime>(
        &relayer,
        pool_id,
        pool_shares,
        min_amounts_out,
        &block_number,
    );

    let signature = SignatureTest::from(who.key_pair().sign(&encoded_payload));
    Proof { signer: who.key_pair().public(), relayer, signature }
}

struct SignedExitContext {
    pub liquidity: u128,
    pub pool_id: MarketId,
    pub pool_shares_amount: u128,
    pub outcomes: Vec<AssetOf<Runtime>>,
}

impl Default for SignedExitContext {
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
            liquidity,
            pool_id,
            pool_shares_amount: _4, // Remove 40% to the pool.
            outcomes: vec![],
        }
    }
}

impl SignedExitContext {
    fn setup_market(&mut self, market_status: &MarketStatus) {
        // Add a second LP to create a more generic situation, bringing the total of shares to _10.
        deposit_complete_set(self.pool_id, bob(), self.liquidity);
        assert_ok!(NeoSwaps::join(
            RuntimeOrigin::signed(bob()),
            self.pool_id,
            self.liquidity,
            vec![u128::MAX, u128::MAX],
        ));
        MarketCommons::mutate_market(&self.pool_id, |market| {
            market.status = *market_status;
            Ok(())
        })
        .unwrap();
        self.outcomes = NeoSwaps::assets(self.pool_id).unwrap();

        let alice_balances = [0, 44_912_220_089];
        assert_balances!(alice(), self.outcomes, alice_balances);
    }
}

#[test_case(MarketStatus::Active, vec![39_960_000_000, 4_066_153_704], 33_508_962_010)]
#[test_case(MarketStatus::Resolved, vec![40_000_000_000, 4_070_223_928], 33_486_637_585)]
fn signed_exit_works(
    market_status: MarketStatus,
    amounts_out: Vec<BalanceOf<Runtime>>,
    new_liquidity_parameter: BalanceOf<Runtime>,
) {
    ExtBuilder::default().build().execute_with(|| {
        let mut context = SignedExitContext::default();
        let alice_account = TestAccount::new([0; 32]);

        let pool_id = context.pool_id;

        context.setup_market(&market_status);

        let alice_balances = [0, 44_912_220_089];

        let proof_blocknumber = System::block_number();
        let min_amounts_out = vec![0, 0];
        let proof = create_signed_exit_proof(
            &alice_account,
            &pool_id,
            &context.pool_shares_amount,
            &min_amounts_out,
        );

        assert_ok!(SignedNeoSwaps::signed_exit(
            RuntimeOrigin::signed(alice()),
            proof,
            pool_id,
            context.pool_shares_amount,
            min_amounts_out,
            proof_blocknumber
        ));

        let new_alice_balances = alice_balances
            .iter()
            .zip(amounts_out.iter())
            .map(|(b, a)| b + a)
            .collect::<Vec<_>>();

        assert_balances!(alice(), context.outcomes, new_alice_balances);
        let pool_shares_amount = context.pool_shares_amount;
        System::assert_last_event(
            NeoSwapsEvent::ExitExecuted {
                who: alice(),
                pool_id,
                pool_shares_amount,
                amounts_out,
                new_liquidity_parameter,
            }
            .into(),
        );
    });
}

mod fails_when {
    use super::*;
    use test_case::test_case;

    #[test_case(MarketStatus::Active)]
    #[test_case(MarketStatus::Resolved)]
    fn proof_has_wrong_relayer(market_status: MarketStatus) {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedExitContext::default();
            let alice_account = TestAccount::new([0; 32]);

            let pool_id = context.pool_id;

            context.setup_market(&market_status);

            let proof_blocknumber = System::block_number();
            let min_amounts_out = vec![0, 0];
            let proof = Proof {
                relayer: dave(),
                ..create_signed_exit_proof(
                    &alice_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &min_amounts_out,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_exit(
                    RuntimeOrigin::signed(alice()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    min_amounts_out,
                    proof_blocknumber
                ),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test_case(MarketStatus::Active)]
    #[test_case(MarketStatus::Resolved)]
    fn proof_data_mismatch_signature(market_status: MarketStatus) {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedExitContext::default();
            let alice_account = TestAccount::new([0; 32]);

            let pool_id = context.pool_id;

            context.setup_market(&market_status);

            let proof_blocknumber = System::block_number();
            let min_amounts_out = vec![0, 0];
            let proof = Proof {
                signature: alice_account.key_pair().sign(&[1u8; 10]),
                ..create_signed_exit_proof(
                    &alice_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &min_amounts_out,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_exit(
                    RuntimeOrigin::signed(alice()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    min_amounts_out,
                    proof_blocknumber
                ),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test_case(MarketStatus::Active)]
    #[test_case(MarketStatus::Resolved)]
    fn proof_data_mismatch_signer(market_status: MarketStatus) {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedExitContext::default();
            let alice_account = TestAccount::new([0; 32]);

            let pool_id = context.pool_id;

            context.setup_market(&market_status);

            let proof_blocknumber = System::block_number();
            let min_amounts_out = vec![0, 0];
            let bad_proof = Proof {
                signer: dave(),
                ..create_signed_exit_proof(
                    &alice_account,
                    &pool_id,
                    &context.pool_shares_amount,
                    &min_amounts_out,
                )
            };

            assert_noop!(
                SignedNeoSwaps::signed_exit(
                    RuntimeOrigin::signed(alice()),
                    bad_proof,
                    pool_id,
                    context.pool_shares_amount,
                    min_amounts_out,
                    proof_blocknumber
                ),
                Error::<Runtime>::SenderIsNotSigner
            );
        });
    }

    #[test_case(MarketStatus::Active)]
    #[test_case(MarketStatus::Resolved)]
    fn proof_has_expired(market_status: MarketStatus) {
        ExtBuilder::default().build().execute_with(|| {
            let mut context = SignedExitContext::default();
            let alice_account = TestAccount::new([0; 32]);

            let pool_id = context.pool_id;

            context.setup_market(&market_status);

            let proof_blocknumber = System::block_number();
            let min_amounts_out = vec![0, 0];
            let proof = create_signed_exit_proof(
                &alice_account,
                &pool_id,
                &context.pool_shares_amount,
                &min_amounts_out,
            );

            System::set_block_number(proof_blocknumber + 100);
            assert_noop!(
                SignedNeoSwaps::signed_exit(
                    RuntimeOrigin::signed(alice()),
                    proof,
                    pool_id,
                    context.pool_shares_amount,
                    min_amounts_out,
                    proof_blocknumber
                ),
                Error::<Runtime>::SignedTransactionExpired
            );
        });
    }
}
