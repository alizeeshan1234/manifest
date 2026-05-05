//! ProcessDepositEr — runs on the MagicBlock ER as the post-delegation
//! action fired by RequestDeposit. Phase 10 deposit Path A, step [2].
//!
//! Validator-signed (NOT the trader). The trader's pubkey is verified
//! against `receipt.trader`.
//!
//! Behaviour:
//!   1. Validate receipt + delegated market.
//!   2. Credit the trader's seat by `receipt.amount`.
//!   3. Set `receipt.processed = 1`.
//!   4. MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!        .add_post_undelegate_actions([CloseDepositReceipt callback])
//!
//! Account list (matches what RequestDeposit baked into the
//! post-delegation action):
//!   [0] trader        (signer per the action's signer list)
//!   [1] market        (writable, delegated)
//!   [2] receipt       (writable, delegated)
//!   [3] magic_program
//!   [4] magic_context (writable)

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
    state::{get_deposit_receipt_address, DepositReceiptFixed, MarketFixed, MarketRefMut},
    validation::{ManifestAccountInfo, Signer},
};

pub(crate) fn process_process_deposit_er(
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

    // Trader is the signer of the post-delegation action (via SDK's
    // signer mechanism — the validator wraps it).
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

    // Both market and receipt are delegated — load via new_delegated.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;
    let receipt: ManifestAccountInfo<DepositReceiptFixed> =
        ManifestAccountInfo::<DepositReceiptFixed>::new_delegated(receipt_info)?;

    let (amount, is_base) = {
        let r: Ref<DepositReceiptFixed> = receipt.get_fixed()?;
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
            r.processed == 0,
            ManifestError::DepositAlreadyProcessed,
            "Deposit already processed",
        )?;
        let (expected, _bump) = get_deposit_receipt_address(&r.market, &r.trader, &r.mint);
        require!(
            &expected == receipt_info.key,
            ManifestError::InvalidDepositReceiptPubkey,
            "Receipt PDA mismatch",
        )?;

        let market_fixed = market.get_fixed()?;
        let is_base = if r.mint == *market_fixed.get_base_mint() {
            true
        } else if r.mint == *market_fixed.get_quote_mint() {
            false
        } else {
            return Err(ManifestError::InvalidDepositMint.into());
        };
        (r.amount, is_base)
    };

    // Credit the seat.
    {
        let market_data: &mut RefMut<&mut [u8]> = &mut market_info.try_borrow_mut_data()?;
        let dynamic_account: MarketRefMut = get_mut_dynamic_account(market_data);
        let trader_index = dynamic_account.get_trader_index(trader.key);
        require!(
            trader_index != NIL,
            ManifestError::InvalidDepositAccounts,
            "Trader has no seat — ClaimSeat first",
        )?;
        drop(dynamic_account);
        let mut dynamic_account_mut: MarketRefMut = get_mut_dynamic_account(market_data);
        dynamic_account_mut.deposit(trader_index, amount, is_base)?;
    }

    // Mark processed.
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        let r = get_mut_helper::<DepositReceiptFixed>(&mut data, 0_u32);
        r.processed = 1;
    }

    // Schedule the post-undelegate CloseDepositReceipt callback.
    // CloseDepositReceipt accounts: [trader (writable), receipt (writable)].
    let close_ix_data: Vec<u8> = vec![22u8]; // CloseDepositReceipt discriminator
    let close_action = CallHandler {
        args: ActionArgs::new(close_ix_data),
        compute_units: 30_000,
        escrow_authority: trader_info.clone(),
        destination_program: crate::ID,
        accounts: vec![
            ShortAccountMeta {
                pubkey: trader_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: receipt_info.key.to_bytes().into(),
                is_writable: true,
            },
        ],
    };

    MagicIntentBundleBuilder::new(
        trader_info.clone(),
        magic_context.clone(),
        magic_program.clone(),
    )
    .commit_and_undelegate(&[receipt_info.clone()])
    .add_post_undelegate_actions([close_action])
    .build_and_invoke()?;

    Ok(())
}
