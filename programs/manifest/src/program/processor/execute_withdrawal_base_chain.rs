//! ExecuteWithdrawalBaseChain — base layer, auto-fired post-undelegate
//! action. Phase 9 withdrawal Path A, step [3]. Validator-signed.
//!
//! Reads receipt.processed_amount, SPL-transfers market_vault -> trader
//! signed by the vault PDA, then closes the receipt.
//!
//! Account list:
//!   [0] trader         (writable, NOT a signer — pubkey verified vs receipt)
//!   [1] market         (read-only)
//!   [2] receipt        (writable — closed)
//!   [3] market_vault   (writable — SPL transfer source)
//!   [4] trader_token   (writable — SPL transfer destination)
//!   [5] mint
//!   [6] token_program

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
    state::{MarketFixed, WithdrawalReceiptFixed},
    validation::{
        get_vault_address, ManifestAccountInfo, MintAccountInfo, TokenAccountInfo, TokenProgram,
    },
};

pub(crate) fn process_execute_withdrawal_base_chain(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let market_vault_info = next_account_info(account_iter)?;
    let trader_token_info = next_account_info(account_iter)?;
    let mint_info = next_account_info(account_iter)?;
    let token_program_info = next_account_info(account_iter)?;

    // Market may be delegated or not — we only read its mints/vaults.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)
            .or_else(|_| ManifestAccountInfo::<MarketFixed>::new(market_info))?;

    // Receipt must be back under Manifest ownership.
    let receipt: ManifestAccountInfo<WithdrawalReceiptFixed> =
        ManifestAccountInfo::<WithdrawalReceiptFixed>::new(receipt_info)?;

    let (mint, processed_amount, market_key) = {
        let r: Ref<WithdrawalReceiptFixed> = receipt.get_fixed()?;
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
            r.mint == *mint_info.key,
            ManifestError::InvalidWithdrawalMint,
            "Receipt mint mismatch",
        )?;
        (r.mint, r.processed_amount, r.market)
    };

    if processed_amount > 0 {
        let market_fixed = market.get_fixed()?;
        let expected_vault: Pubkey = if mint == *market_fixed.get_base_mint() {
            *market_fixed.get_base_vault()
        } else if mint == *market_fixed.get_quote_mint() {
            *market_fixed.get_quote_vault()
        } else {
            return Err(ManifestError::InvalidWithdrawalMint.into());
        };
        let decimals: u8 = if mint == *market_fixed.get_base_mint() {
            market_fixed.get_base_mint_decimals()
        } else {
            market_fixed.get_quote_mint_decimals()
        };
        drop(market_fixed);

        require!(
            expected_vault == *market_vault_info.key,
            ManifestError::IncorrectAccount,
            "Vault mismatch for mint",
        )?;

        let _vault: TokenAccountInfo = TokenAccountInfo::new_with_owner_and_key(
            market_vault_info,
            &mint,
            &expected_vault,
            &expected_vault,
        )?;
        let _trader_token: TokenAccountInfo =
            TokenAccountInfo::new_with_owner(trader_token_info, &mint, trader_info.key)?;
        let _mint_acc: MintAccountInfo = MintAccountInfo::new(mint_info)?;
        let token_program: TokenProgram = TokenProgram::new(token_program_info)?;

        let (_vault_pda, vault_bump) = get_vault_address(&market_key, &mint);

        if *token_program.key == spl_token_2022::id() {
            invoke_signed(
                &spl_token_2022::instruction::transfer_checked(
                    token_program.key,
                    market_vault_info.key,
                    mint_info.key,
                    trader_token_info.key,
                    market_vault_info.key, // authority = vault itself
                    &[],
                    processed_amount,
                    decimals,
                )?,
                &[
                    token_program.as_ref().clone(),
                    market_vault_info.clone(),
                    mint_info.clone(),
                    trader_token_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &mint, vault_bump),
            )?;
        } else {
            invoke_signed(
                &spl_token::instruction::transfer(
                    token_program.key,
                    market_vault_info.key,
                    trader_token_info.key,
                    market_vault_info.key,
                    &[],
                    processed_amount,
                )?,
                &[
                    token_program.as_ref().clone(),
                    market_vault_info.clone(),
                    trader_token_info.clone(),
                ],
                market_vault_seeds_with_bump!(&market_key, &mint, vault_bump),
            )?;
        }
    }

    // Close the receipt: drain lamports to trader, zero data, assign to
    // system program.
    let dest_starting = trader_info.lamports();
    **trader_info.lamports.borrow_mut() =
        dest_starting.checked_add(receipt_info.lamports()).unwrap();
    **receipt_info.lamports.borrow_mut() = 0;
    receipt_info.assign(&solana_program::system_program::id());
    #[allow(deprecated)]
    receipt_info.realloc(0, false)?;

    Ok(())
}
