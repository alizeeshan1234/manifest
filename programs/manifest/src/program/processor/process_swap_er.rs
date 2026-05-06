//! ProcessSwapEr — runs on the ER as the post-delegation action fired by
//! RequestSwap. Phase B swap Path A, step [2]. Validator-signed.
//!
//! Behaviour:
//!   1. Validate receipt + delegated market.
//!   2. Claim a seat for the trader if they don't already have one;
//!      remember whether we created it so we can release it after.
//!   3. Virtually credit input_amount onto the seat's input side
//!      (this matches the SPL transfer that happened on base in step [1]).
//!   4. Place an aggressive IOC limit (price = MIN/MAX) so the order
//!      sweeps the book on this side.
//!   5. Read the seat balance after matching:
//!         processed_in  = input_amount - residual on input side
//!         processed_out = gain on output side
//!   6. Require processed_out >= min_out (else revert).
//!   7. Withdraw both gain + residual from the seat so the seat is back
//!      to its pre-swap state. If we created the seat in step 2, release it.
//!   8. Write processed_in / processed_out into the receipt.
//!   9. MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!         .add_post_undelegate_actions([ExecuteSwapBaseChain])
//!
//! Account list (must match what RequestSwap baked into the post-
//! delegation action):
//!   [0]  trader            (action signer)
//!   [1]  market            (writable, delegated)
//!   [2]  receipt           (writable, delegated)
//!   [3]  magic_program
//!   [4]  magic_context     (writable)
//!   [5]  input_vault       (forwarded to step [3])
//!   [6]  output_vault      (forwarded to step [3])
//!   [7]  trader_token_in   (forwarded to step [3])
//!   [8]  trader_token_out  (forwarded to step [3])
//!   [9]  input_mint        (forwarded)
//!   [10] output_mint       (forwarded)
//!   [11] token_program     (forwarded)

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
    quantities::{BaseAtoms, QuoteAtoms, QuoteAtomsPerBaseAtom, WrapperU64},
    require,
    state::{
        get_swap_receipt_address, AddOrderToMarketArgs, AddOrderToMarketResult, MarketFixed,
        MarketRefMut, OrderType, SwapReceiptFixed, NO_EXPIRATION_LAST_VALID_SLOT,
    },
    validation::{ManifestAccountInfo, Signer},
};

pub(crate) fn process_process_swap_er(
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
    let input_vault_info = next_account_info(account_iter)?;
    let output_vault_info = next_account_info(account_iter)?;
    let trader_token_in_info = next_account_info(account_iter)?;
    let trader_token_out_info = next_account_info(account_iter)?;
    let input_mint_info = next_account_info(account_iter)?;
    let output_mint_info = next_account_info(account_iter)?;
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
    let receipt: ManifestAccountInfo<SwapReceiptFixed> =
        ManifestAccountInfo::<SwapReceiptFixed>::new_delegated(receipt_info)?;

    let (input_amount, min_out, is_base_in) = {
        let r: Ref<SwapReceiptFixed> = receipt.get_fixed()?;
        require!(
            r.trader == *trader.key,
            ManifestError::IncorrectAccount,
            "Receipt trader != action signer",
        )?;
        require!(
            r.market == *market_info.key,
            ManifestError::IncorrectAccount,
            "Receipt market mismatch",
        )?;
        require!(
            r.processed_in == 0 && r.processed_out == 0,
            ManifestError::SwapAlreadyProcessed,
            "Swap already processed",
        )?;
        require!(
            r.input_mint == *input_mint_info.key,
            ManifestError::InvalidSwapMint,
            "Receipt input_mint mismatch",
        )?;
        require!(
            r.output_mint == *output_mint_info.key,
            ManifestError::InvalidSwapMint,
            "Receipt output_mint mismatch",
        )?;
        let (expected, _bump) =
            get_swap_receipt_address(&r.market, &r.trader, &r.input_mint);
        require!(
            &expected == receipt_info.key,
            ManifestError::InvalidSwapReceiptPubkey,
            "SwapReceipt PDA mismatch",
        )?;
        (r.input_amount, r.min_out, r.is_base_in == 1)
    };

    // Verify direction matches market mints.
    {
        let market_fixed = market.get_fixed()?;
        let dir_ok = if is_base_in {
            input_mint_info.key == market_fixed.get_base_mint()
                && output_mint_info.key == market_fixed.get_quote_mint()
        } else {
            input_mint_info.key == market_fixed.get_quote_mint()
                && output_mint_info.key == market_fixed.get_base_mint()
        };
        require!(
            dir_ok,
            ManifestError::InvalidSwapMint,
            "Receipt direction does not match market mints",
        )?;
    }

    // Forwarded accounts only need pubkey forwarding to step [3] — no
    // validation here (vault PDAs were checked in step [1]).
    let _ = input_vault_info;
    let _ = output_vault_info;
    let _ = trader_token_in_info;
    let _ = trader_token_out_info;
    let _ = token_program_info;

    let (processed_in, processed_out) = {
        let market_data: &mut RefMut<&mut [u8]> = &mut market_info.try_borrow_mut_data()?;
        let mut dynamic_account: MarketRefMut = get_mut_dynamic_account(market_data);

        let existing_seat_index = dynamic_account.get_trader_index(trader.key);
        let was_new_seat = existing_seat_index == NIL;
        if was_new_seat {
            dynamic_account.claim_seat(trader.key)?;
        }
        let trader_index = dynamic_account.get_trader_index(trader.key);
        require!(
            trader_index != NIL,
            ManifestError::InvalidSwapAccounts,
            "Failed to obtain seat index after claim_seat",
        )?;

        // Snapshot pre-swap balances.
        let (initial_base, initial_quote) = dynamic_account.get_trader_balance(trader.key);

        // Virtual deposit so matching has the input on hand.
        dynamic_account.deposit(trader_index, input_amount, is_base_in)?;

        // Aggressive IOC limit on the side that consumes input_amount.
        // is_base_in=true  -> trader sells base for quote -> ask at MIN
        // is_base_in=false -> trader buys base with quote -> bid at MAX
        let price: QuoteAtomsPerBaseAtom = if is_base_in {
            QuoteAtomsPerBaseAtom::MIN
        } else {
            QuoteAtomsPerBaseAtom::MAX
        };

        // For exact-input-base, num_base_atoms = input_amount directly.
        // For exact-input-quote, ask the book how many base atoms it can
        // back with the given quote ceiling.
        let global_trade_accounts_opts = &[None, None];
        let num_base_atoms: BaseAtoms = if is_base_in {
            BaseAtoms::new(input_amount)
        } else {
            dynamic_account.impact_base_atoms(
                true, // is_bid (we're buying base)
                QuoteAtoms::new(input_amount),
                global_trade_accounts_opts,
            )?
        };

        let _result: AddOrderToMarketResult = dynamic_account.place_order(AddOrderToMarketArgs {
            market: *market_info.key,
            trader_index,
            num_base_atoms,
            price,
            is_bid: !is_base_in,
            last_valid_slot: NO_EXPIRATION_LAST_VALID_SLOT,
            order_type: OrderType::ImmediateOrCancel,
            global_trade_accounts_opts,
            current_slot: None,
        })?;

        // Post-trade balance gives us processed_in / processed_out.
        let (final_base, final_quote) = dynamic_account.get_trader_balance(trader.key);
        let (input_residual, output_gain): (u64, u64) = if is_base_in {
            // residual base = final_base - initial_base (we'd virtually
            //   deposited input_amount; whatever's left over wasn't sold)
            // gain quote   = final_quote - initial_quote
            (
                u64::from(final_base) - u64::from(initial_base),
                u64::from(final_quote) - u64::from(initial_quote),
            )
        } else {
            (
                u64::from(final_quote) - u64::from(initial_quote),
                u64::from(final_base) - u64::from(initial_base),
            )
        };

        let processed_in = input_amount.saturating_sub(input_residual);
        let processed_out = output_gain;

        require!(
            processed_out >= min_out,
            ManifestError::SwapSlippageExceeded,
            "Swap output {} below min_out {}",
            processed_out,
            min_out,
        )?;

        // Zero out the swap's contribution to the seat so the vaults can
        // settle without leaving balance dust on the seat. After these
        // withdraws the seat is back to (initial_base, initial_quote).
        if input_residual > 0 {
            dynamic_account.withdraw(trader_index, input_residual, is_base_in)?;
        }
        if output_gain > 0 {
            dynamic_account.withdraw(trader_index, output_gain, !is_base_in)?;
        }

        if was_new_seat {
            dynamic_account.release_seat(trader.key)?;
        }

        (processed_in, processed_out)
    };

    // Write results into the receipt.
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        let r = get_mut_helper::<SwapReceiptFixed>(&mut data, 0_u32);
        r.processed_in = processed_in;
        r.processed_out = processed_out;
    }

    // Schedule the post-undelegate ExecuteSwapBaseChain callback.
    let exec_ix_data: Vec<u8> = vec![28u8]; // ExecuteSwapBaseChain
    let exec_action = CallHandler {
        args: ActionArgs::new(exec_ix_data),
        compute_units: 80_000,
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
                pubkey: input_vault_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: output_vault_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: trader_token_in_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: trader_token_out_info.key.to_bytes().into(),
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: input_mint_info.key.to_bytes().into(),
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: output_mint_info.key.to_bytes().into(),
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
