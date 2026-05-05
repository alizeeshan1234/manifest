//! UndelegateMarket — finalization callback.
//!
//! There are two entry points:
//!
//!   1. `process_undelegate_market` — the legacy single-byte dispatch path
//!      (`ManifestInstruction::UndelegateMarket = 17`). Reconstructs PDA
//!      seeds by reading mint offsets from the buffered account data. Useful
//!      for diagnostics and direct CLI invocation.
//!   2. `process_undelegate_market_with_seeds` — the canonical path. Called
//!      automatically by the MagicBlock delegation program via the 8-byte
//!      `EXTERNAL_UNDELEGATE_DISCRIMINATOR` after `CommitAndUndelegate` runs
//!      on the ER. Seeds are passed in as ix data; the buffer is signer.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::magicblock::cpi::undelegate_account;

const BASE_MINT_OFFSET: usize = 16;
const QUOTE_MINT_OFFSET: usize = 48;
const MARKET_ID_OFFSET: usize = 13;

/// Canonical entry point. Called by the delegation program with seeds
/// already serialized into the ix data.
pub fn process_undelegate_market_with_seeds(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    account_seeds: Vec<Vec<u8>>,
) -> ProgramResult {
    // Anchor-style account order from generate_undelegate macro:
    //   [base_account, buffer, payer, system_program]
    let account_iter = &mut accounts.iter();
    let delegated_account = next_account_info(account_iter)?;
    let buffer_info = next_account_info(account_iter)?;
    let payer_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;

    undelegate_account(
        delegated_account,
        program_id,
        buffer_info,
        payer_info,
        system_program_info,
        account_seeds,
    )?;
    Ok(())
}

/// Legacy single-byte dispatch path. Reconstructs seeds from the buffered
/// data instead of receiving them in the ix data.
pub(crate) fn process_undelegate_market(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let market_info = next_account_info(account_iter)?;
    let buffer_info = next_account_info(account_iter)?;
    let payer_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;

    let pda_seeds: Vec<Vec<u8>> = {
        let data = buffer_info.try_borrow_data()?;
        let base_mint = Pubkey::new_from_array(
            data[BASE_MINT_OFFSET..BASE_MINT_OFFSET + 32]
                .try_into()
                .unwrap(),
        );
        let quote_mint = Pubkey::new_from_array(
            data[QUOTE_MINT_OFFSET..QUOTE_MINT_OFFSET + 32]
                .try_into()
                .unwrap(),
        );
        let market_id = data[MARKET_ID_OFFSET];
        vec![
            b"market".to_vec(),
            base_mint.as_ref().to_vec(),
            quote_mint.as_ref().to_vec(),
            vec![market_id],
        ]
    };

    undelegate_account(
        market_info,
        program_id,
        buffer_info,
        payer_info,
        system_program_info,
        pda_seeds,
    )?;
    Ok(())
}
