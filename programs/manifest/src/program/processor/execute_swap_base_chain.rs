//! ExecuteSwapBaseChain — base layer, auto-fired post-undelegate action
//! Phase B swap Path A, step [3]. Validator-signed.
//!
//! Reads the receipt's processed_in / processed_out / input_amount and:
//!   1. SPL transfer output_vault → trader_token_out for processed_out.
//!   2. If input_amount > processed_in (partial fill), refund the
//!      remainder: SPL transfer input_vault → trader_token_in.
//!   3. Close the receipt, refund rent to trader.
//!
//! Account list (must match what ProcessSwapEr embedded in its post-
//! undelegate action):
//!   [0] trader            (writable, NOT a signer — verified vs receipt)
//!   [1] market            (read-only)
//!   [2] receipt           (writable — closed at end)
//!   [3] input_vault       (writable — refund source if partial fill)
//!   [4] output_vault      (writable — payout source)
//!   [5] trader_token_in   (writable — refund destination)
//!   [6] trader_token_out  (writable — payout destination)
//!   [7] input_mint
//!   [8] output_mint
//!   [9] token_program

use std::cell::Ref;

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program::invoke_signed,
    pubkey::Pubkey,
};

use crate::{
    market_vault_seeds_with_bump,
    program::ManifestError,
    require,
    state::{MarketFixed, SwapReceiptFixed},
    validation::{
        get_vault_address, ManifestAccountInfo, MintAccountInfo, TokenAccountInfo, TokenProgram,
    },
};

pub(crate) fn process_execute_swap_base_chain(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let input_vault_info = next_account_info(account_iter)?;
    let output_vault_info = next_account_info(account_iter)?;
    let trader_token_in_info = next_account_info(account_iter)?;
    let trader_token_out_info = next_account_info(account_iter)?;
    let input_mint_info = next_account_info(account_iter)?;
    let output_mint_info = next_account_info(account_iter)?;
    let token_program_info = next_account_info(account_iter)?;

    // Market may be delegated or not — we only read its mints/vaults.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)
            .or_else(|_| ManifestAccountInfo::<MarketFixed>::new(market_info))?;

    let receipt: ManifestAccountInfo<SwapReceiptFixed> =
        ManifestAccountInfo::<SwapReceiptFixed>::new(receipt_info)?;

    let (input_amount, processed_in, processed_out, input_mint_key, output_mint_key, market_key) = {
        let r: Ref<SwapReceiptFixed> = receipt.get_fixed()?;
        require!(
            r.trader == *trader_info.key,
            ManifestError::IncorrectAccount,
            "Receipt trader mismatch",
        )?;
        require!(
            r.market == *market_info.key,
            ManifestError::IncorrectAccount,
            "Receipt market mismatch",
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
        (
            r.input_amount,
            r.processed_in,
            r.processed_out,
            r.input_mint,
            r.output_mint,
            r.market,
        )
    };

    let market_fixed = market.get_fixed()?;
    require!(
        *market_fixed.get_base_mint() == input_mint_key
            || *market_fixed.get_quote_mint() == input_mint_key,
        ManifestError::InvalidSwapMint,
        "input_mint not on this market",
    )?;
    require!(
        *market_fixed.get_base_mint() == output_mint_key
            || *market_fixed.get_quote_mint() == output_mint_key,
        ManifestError::InvalidSwapMint,
        "output_mint not on this market",
    )?;
    drop(market_fixed);

    // Resolve vault PDAs + bumps + decimals for both sides.
    let (expected_input_vault, input_bump) =
        get_vault_address(&market_key, &input_mint_key);
    let (expected_output_vault, output_bump) =
        get_vault_address(&market_key, &output_mint_key);
    require!(
        expected_input_vault == *input_vault_info.key,
        ManifestError::IncorrectAccount,
        "input_vault mismatch",
    )?;
    require!(
        expected_output_vault == *output_vault_info.key,
        ManifestError::IncorrectAccount,
        "output_vault mismatch",
    )?;

    let market_fixed = market.get_fixed()?;
    let input_decimals: u8 = if input_mint_key == *market_fixed.get_base_mint() {
        market_fixed.get_base_mint_decimals()
    } else {
        market_fixed.get_quote_mint_decimals()
    };
    let output_decimals: u8 = if output_mint_key == *market_fixed.get_base_mint() {
        market_fixed.get_base_mint_decimals()
    } else {
        market_fixed.get_quote_mint_decimals()
    };
    drop(market_fixed);

    let _input_vault_acc: TokenAccountInfo = TokenAccountInfo::new_with_owner_and_key(
        input_vault_info,
        &input_mint_key,
        &expected_input_vault,
        &expected_input_vault,
    )?;
    let _output_vault_acc: TokenAccountInfo = TokenAccountInfo::new_with_owner_and_key(
        output_vault_info,
        &output_mint_key,
        &expected_output_vault,
        &expected_output_vault,
    )?;
    let _trader_token_in_acc: TokenAccountInfo =
        TokenAccountInfo::new_with_owner(trader_token_in_info, &input_mint_key, trader_info.key)?;
    let _trader_token_out_acc: TokenAccountInfo =
        TokenAccountInfo::new_with_owner(trader_token_out_info, &output_mint_key, trader_info.key)?;
    let _input_mint_acc: MintAccountInfo = MintAccountInfo::new(input_mint_info)?;
    let _output_mint_acc: MintAccountInfo = MintAccountInfo::new(output_mint_info)?;
    let token_program: TokenProgram = TokenProgram::new(token_program_info)?;

    // 1. Payout: output_vault -> trader_token_out for processed_out.
    if processed_out > 0 {
        if *token_program.key == spl_token_2022::id() {
            invoke_signed(
                &spl_token_2022::instruction::transfer_checked(
                    token_program.key,
                    output_vault_info.key,
                    output_mint_info.key,
                    trader_token_out_info.key,
                    output_vault_info.key,
                    &[],
                    processed_out,
                    output_decimals,
                )?,
                &[
                    token_program.as_ref().clone(),
                    output_vault_info.clone(),
                    output_mint_info.clone(),
                    trader_token_out_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &output_mint_key, output_bump),
            )?;
        } else {
            invoke_signed(
                &spl_token::instruction::transfer(
                    token_program.key,
                    output_vault_info.key,
                    trader_token_out_info.key,
                    output_vault_info.key,
                    &[],
                    processed_out,
                )?,
                &[
                    token_program.as_ref().clone(),
                    output_vault_info.clone(),
                    trader_token_out_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &output_mint_key, output_bump),
            )?;
        }
    }

    // 2. Refund residual input on partial fill.
    let refund: u64 = input_amount.saturating_sub(processed_in);
    if refund > 0 {
        if *token_program.key == spl_token_2022::id() {
            invoke_signed(
                &spl_token_2022::instruction::transfer_checked(
                    token_program.key,
                    input_vault_info.key,
                    input_mint_info.key,
                    trader_token_in_info.key,
                    input_vault_info.key,
                    &[],
                    refund,
                    input_decimals,
                )?,
                &[
                    token_program.as_ref().clone(),
                    input_vault_info.clone(),
                    input_mint_info.clone(),
                    trader_token_in_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &input_mint_key, input_bump),
            )?;
        } else {
            invoke_signed(
                &spl_token::instruction::transfer(
                    token_program.key,
                    input_vault_info.key,
                    trader_token_in_info.key,
                    input_vault_info.key,
                    &[],
                    refund,
                )?,
                &[
                    token_program.as_ref().clone(),
                    input_vault_info.clone(),
                    trader_token_in_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &input_mint_key, input_bump),
            )?;
        }
    }

    // 3. Close the receipt, refund rent to the trader.
    let dest_starting = trader_info.lamports();
    **trader_info.lamports.borrow_mut() =
        dest_starting.checked_add(receipt_info.lamports()).unwrap();
    **receipt_info.lamports.borrow_mut() = 0;
    receipt_info.assign(&solana_program::system_program::id());
    #[allow(deprecated)]
    receipt_info.realloc(0, false)?;

    Ok(())
}
