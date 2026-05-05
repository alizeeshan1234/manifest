//! UndelegateMarket — base-layer finalization step after the ER has
//! called `CommitAndUndelegateMarket`. Recreates the market PDA owned
//! by Manifest and copies the buffered state back into it.
//!
//! Seeds are reconstructed by reading the `base_mint`, `quote_mint`,
//! and `market_id` fields directly out of the buffered account data
//! at the byte offsets defined by `MarketFixed`.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::magicblock::cpi::undelegate_account;

// Byte offsets inside MarketFixed:
//   discriminant       u64    @ 0
//   version, decimals... 8 bytes @ 8
//   base_mint          [u8;32]@ 16
//   quote_mint         [u8;32]@ 48
const BASE_MINT_OFFSET: usize = 16;
const QUOTE_MINT_OFFSET: usize = 48;
// market_id sits at offset 13 (after the five u8 fields starting at offset 8).
const MARKET_ID_OFFSET: usize = 13;

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

    // Read seeds out of the buffered state. After CommitAndUndelegate, the
    // delegation buffer holds the latest market bytes; the on-chain market
    // account itself is owned by the delegation program.
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
