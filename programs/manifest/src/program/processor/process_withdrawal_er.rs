//! ProcessWithdrawalEr — runs on the ER as the post-delegation action
//! fired by RequestWithdrawal. Phase 9 withdrawal Path A, step [2].
//!
//! Validator-signed (NOT trader). Trader pubkey verified vs receipt.trader.
//!
//! Behaviour:
//!   1. Validate receipt + delegated market.
//!   2. Look up trader's seat. Compute processed = min(requested,
//!      available_for_this_side). Debit the seat.
//!   3. Write processed_amount into the receipt.
//!   4. MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!        .add_post_undelegate_actions([ExecuteWithdrawalBaseChain])
//!
//! Account list (matches what RequestWithdrawal embedded in the action):
//!   [0] trader        (signer per the action's signer list)
//!   [1] market        (writable, delegated)
//!   [2] receipt       (writable, delegated)
//!   [3] magic_program
//!   [4] magic_context (writable)
//!   [5] market_vault     (passed through to ExecuteWithdrawalBaseChain)
//!   [6] trader_token     (passed through)
//!   [7] mint             (passed through)
//!   [8] token_program    (passed through)

use std::cell::{Ref, RefMut};

use ephemeral_rollups_sdk::ephem::{
    CallHandler, FoldableIntentBuilder, MagicIntentBundleBuilder,
};
use hypertree::{get_mut_helper, NIL};
use magicblock_magic_program_api::args::{ActionArgs, ShortAccountMeta};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::{
    program::{get_mut_dynamic_account, ManifestError},
    require,
    state::{get_withdrawal_receipt_address, MarketFixed, MarketRefMut, WithdrawalReceiptFixed},
    validation::{ManifestAccountInfo, Signer},
};

pub(crate) fn process_process_withdrawal_er(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let magic_program = next_account_info(account_iter)?;
    let magic_context = next_account_info(account_iter)?;
    let market_vault_info = next_account_info(account_iter)?;
    let trader_token_info = next_account_info(account_iter)?;
    let mint_info = next_account_info(account_iter)?;
    let token_program_info = next_account_info(account_iter)?;

    let trader: Signer = Signer::new(trader_info)?;

    require!(
        magic_program.key.to_bytes()
            == ephemeral_rollups_sdk::consts::MAGIC_PROGRAM_ID.to_bytes(),
        ManifestError::InvalidMagicProgramId,
        "Invalid MagicBlock program id",
    )?;
    require!(
        magic_context.key.to_bytes()
            == ephemeral_rollups_sdk::consts::MAGIC_CONTEXT_ID.to_bytes(),
        ManifestError::InvalidMagicContextId,
        "Invalid MagicBlock context id",
    )?;

    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;
    let receipt: ManifestAccountInfo<WithdrawalReceiptFixed> =
        ManifestAccountInfo::<WithdrawalReceiptFixed>::new_delegated(receipt_info)?;

    let (requested_amount, is_base) = {
        let r: Ref<WithdrawalReceiptFixed> = receipt.get_fixed()?;
        require!(
            r.trader == *trader.key,
            ManifestError::IncorrectAccount,
            "Receipt trader != action signer",
        )?;
        require!(
            r.market == *market_info.key,
            ManifestError::IncorrectAccount,
            "Receipt market != market account",
        )?;
        require!(
            r.processed_amount == 0,
            ManifestError::WithdrawalAlreadyProcessed,
            "Withdrawal already processed",
        )?;
        require!(
            r.mint == *mint_info.key,
            ManifestError::InvalidWithdrawalMint,
            "Receipt mint mismatch",
        )?;
        let (expected, _bump) = get_withdrawal_receipt_address(&r.market, &r.trader, &r.mint);
        require!(
            &expected == receipt_info.key,
            ManifestError::InvalidWithdrawalReceiptPubkey,
            "Receipt PDA mismatch",
        )?;

        let market_fixed = market.get_fixed()?;
        let is_base = if r.mint == *market_fixed.get_base_mint() {
            true
        } else if r.mint == *market_fixed.get_quote_mint() {
            false
        } else {
            return Err(ManifestError::InvalidWithdrawalMint.into());
        };
        (r.requested_amount, is_base)
    };

    // Look up trader's seat, clamp, debit.
    let processed_amount: u64 = {
        let market_data: &mut RefMut<&mut [u8]> = &mut market_info.try_borrow_mut_data()?;
        let dynamic_account: MarketRefMut = get_mut_dynamic_account(market_data);
        let (base_atoms, quote_atoms) = dynamic_account.get_trader_balance(trader.key);
        let available: u64 = if is_base {
            base_atoms.into()
        } else {
            quote_atoms.into()
        };
        let processed = requested_amount.min(available);

        let trader_index = dynamic_account.get_trader_index(trader.key);
        require!(
            trader_index != NIL,
            ManifestError::InvalidWithdrawAccounts,
            "Trader has no seat on this market",
        )?;
        drop(dynamic_account);
        let mut dynamic_account_mut: MarketRefMut = get_mut_dynamic_account(market_data);
        dynamic_account_mut.withdraw(trader_index, processed, is_base)?;
        processed
    };

    // Record processed_amount in the receipt.
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        let r = get_mut_helper::<WithdrawalReceiptFixed>(&mut data, 0_u32);
        r.processed_amount = processed_amount;
    }

    // Schedule the post-undelegate ExecuteWithdrawalBaseChain callback.
    let exec_ix_data: Vec<u8> = vec![25u8]; // ExecuteWithdrawalBaseChain
    let exec_action = CallHandler {
        args: ActionArgs::new(exec_ix_data),
        compute_units: 60_000,
        escrow_authority: trader_info.clone(),
        destination_program: crate::ID,
        accounts: vec![
            ShortAccountMeta {
                pubkey: trader_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: market_info.key.to_bytes().into(),
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: receipt_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: market_vault_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: trader_token_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: mint_info.key.to_bytes().into(),
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: token_program_info.key.to_bytes().into(),
                is_writable: false,
            },
        ],
    };

    MagicIntentBundleBuilder::new(
        trader_info.clone(),
        magic_context.clone(),
        magic_program.clone(),
    )
    .commit_and_undelegate(&[receipt_info.clone()])
    .add_post_undelegate_actions([exec_action])
    .build_and_invoke()?;

    Ok(())
}
