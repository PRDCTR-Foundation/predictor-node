use super::*;
use pallet_prediction_markets::DispatchResultWithPostInfo;
use prediction_market_primitives::{test_helper::TestAccount, types::SignatureTest};
use sp_avn_common::Proof;
use sp_core::Pair;

struct SignedWithdrawContext {
    pub pool_id: MarketId,
    pub category_count: u16,
}
impl Default for SignedWithdrawContext {
    fn default() -> Self {
        let spot_prices = vec![_3_4, _1_4];
        let category_count = 2;
        let pool_id = create_market_and_deploy_pool(
            alice(),
            BASE_ASSET,
            MarketType::Categorical(category_count),
            _10,
            spot_prices.clone(),
            CENT_BASE,
        );
        Self { pool_id, category_count }
    }
}

impl SignedWithdrawContext {
    fn create_signed_withdraw_proof(
        &self,
        who: &TestAccount,
    ) -> Proof<SignatureTest, TestAccountIdPK> {
        let relayer = eve();
        let block_number = System::block_number();
        let encoded_payload =
            encode_signed_withdraw_fees_params::<Runtime>(&relayer, &self.pool_id, &block_number);

        let signature = SignatureTest::from(who.key_pair().sign(&encoded_payload));
        let proof = Proof { signer: who.key_pair().public(), relayer, signature };

        proof
    }

    fn test_signed_withdraw(
        &self,
        who: AccountIdOf<Runtime>,
        proof: Proof<SignatureTest, TestAccountIdPK>,
    ) -> DispatchResultWithPostInfo {
        let block_number = System::block_number();
        self.test_signed_withdraw_with_block_number(who, proof, block_number)
    }

    fn test_signed_withdraw_with_block_number(
        &self,
        who: AccountIdOf<Runtime>,
        proof: Proof<SignatureTest, TestAccountIdPK>,
        block_number: u32,
    ) -> DispatchResultWithPostInfo {
        let old_balance = <Runtime as Config>::MultiCurrency::free_balance(BASE_ASSET, &who);

        SignedNeoSwaps::signed_withdraw_fees(
            RuntimeOrigin::signed(who),
            proof,
            self.pool_id,
            block_number,
        )?;
        let new_balance = <Runtime as Config>::MultiCurrency::free_balance(BASE_ASSET, &who);
        let fees_withdrawn = new_balance - old_balance;
        assert!(fees_withdrawn > 0);
        System::assert_last_event(
            NeoSwapsEvent::FeesWithdrawn { who, pool_id: self.pool_id, amount: fees_withdrawn }
                .into(),
        );
        Ok(().into())
    }

    fn join(&self, who: AccountIdOf<Runtime>, amount: BalanceOf<Runtime>) {
        // Adding a little more to ensure that rounding doesn't cause issues.
        deposit_complete_set(self.pool_id, who, amount + CENT_BASE);
        assert_ok!(NeoSwaps::join(
            RuntimeOrigin::signed(who),
            self.pool_id,
            amount,
            vec![u128::MAX; self.category_count as usize],
        ));
    }

    fn accrue_fees(&self) {
        let outcomes = NeoSwaps::assets(self.pool_id).unwrap();
        let amount_in = _10;

        assert_ok!(AssetManager::deposit(BASE_ASSET, &dave(), amount_in));
        assert_ok!(NeoSwaps::buy(
            RuntimeOrigin::signed(dave()),
            self.pool_id,
            self.category_count,
            outcomes[0],
            amount_in,
            0,
        ));
    }
}

fn deposit(who: AccountIdOf<Runtime>) {
    // Make sure everybody's got at least the minimum deposit.
    assert_ok!(<Runtime as Config>::MultiCurrency::deposit(
        BASE_ASSET,
        &who,
        <Runtime as Config>::MultiCurrency::minimum_balance(BASE_ASSET)
    ));
}

#[test]
fn signed_withdraw_fees_works() {
    // Verify that fees are correctly distributed among LPs.
    ExtBuilder::default().build().execute_with(|| {
        let context = SignedWithdrawContext::default();

        context.join(bob(), _10);
        context.join(charlie(), _20);
        context.accrue_fees();

        // Alice seed is 0
        let alice = TestAccount::new([0; 32]);
        deposit(alice.account_id());
        assert_ok!(context.test_signed_withdraw(
            alice.account_id(),
            context.create_signed_withdraw_proof(&alice)
        ));
    });
}

mod fails_when {
    use super::*;
    #[test]
    fn proof_has_wrong_relayer() {
        ExtBuilder::default().build().execute_with(|| {
            let context = SignedWithdrawContext::default();

            context.join(bob(), _10);
            context.join(charlie(), _20);

            // Alice seed is 0
            let alice = TestAccount::new([0; 32]);
            deposit(alice.account_id());
            let bad_proof =
                Proof { relayer: dave(), ..context.create_signed_withdraw_proof(&alice) };
            assert_noop!(
                context.test_signed_withdraw(alice.account_id(), bad_proof),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test]
    fn proof_data_mismatch_relayer() {
        ExtBuilder::default().build().execute_with(|| {
            let context = SignedWithdrawContext::default();

            context.join(bob(), _10);
            context.join(charlie(), _20);

            // Alice seed is 0
            let alice = TestAccount::new([0; 32]);
            deposit(alice.account_id());
            let bad_proof =
                Proof { relayer: bob(), ..context.create_signed_withdraw_proof(&alice) };
            assert_noop!(
                context.test_signed_withdraw(alice.account_id(), bad_proof),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test]
    fn proof_data_mismatch_signature() {
        ExtBuilder::default().build().execute_with(|| {
            let context = SignedWithdrawContext::default();

            context.join(bob(), _10);
            context.join(charlie(), _20);

            // Alice seed is 0
            let alice = TestAccount::new([0; 32]);
            deposit(alice.account_id());
            let bad_proof = Proof {
                signature: alice.key_pair().sign(&[1u8; 10]),
                ..context.create_signed_withdraw_proof(&alice)
            };
            assert_noop!(
                context.test_signed_withdraw(alice.account_id(), bad_proof),
                Error::<Runtime>::UnauthorizedSignedTransaction
            );
        });
    }

    #[test]
    fn proof_data_mismatch_signer() {
        ExtBuilder::default().build().execute_with(|| {
            let context = SignedWithdrawContext::default();

            context.join(bob(), _10);
            context.join(charlie(), _20);

            // Alice seed is 0
            let alice = TestAccount::new([0; 32]);
            deposit(alice.account_id());
            let bad_proof =
                Proof { signer: dave(), ..context.create_signed_withdraw_proof(&alice) };
            assert_noop!(
                context.test_signed_withdraw(alice.account_id(), bad_proof),
                Error::<Runtime>::SenderIsNotSigner
            );
        });
    }

    #[test]
    fn proof_has_expired() {
        ExtBuilder::default().build().execute_with(|| {
            let context = SignedWithdrawContext::default();

            context.join(bob(), _10);
            context.join(charlie(), _20);

            // Alice seed is 0
            let alice = TestAccount::new([0; 32]);
            deposit(alice.account_id());

            let proof_blocknumber = System::block_number();
            let proof = context.create_signed_withdraw_proof(&alice);
            System::set_block_number(proof_blocknumber + 100);

            assert_noop!(
                context.test_signed_withdraw_with_block_number(
                    alice.account_id(),
                    proof,
                    proof_blocknumber
                ),
                Error::<Runtime>::SignedTransactionExpired
            );
        });
    }
}
