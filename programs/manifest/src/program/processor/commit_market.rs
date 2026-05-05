//! CommitMarket — ER-side instruction that snapshots the delegated
//! market's state back to the base layer. The market stays delegated.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::{
    magicblock::{
        consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
        ephem::commit_accounts,
    },
    program::ManifestError,
    require,
    state::MarketFixed,
    validation::{get_market_address, ManifestAccountInfo, Signer},
};

pub(crate) fn process_commit_market(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let payer_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let magic_program = next_account_info(account_iter)?;
    let magic_context = next_account_info(account_iter)?;

    let payer: Signer = Signer::new_payer(payer_info)?;

    require!(
        magic_program.key == &MAGIC_PROGRAM_ID,
        ManifestError::InvalidMagicProgramId,
        "Invalid MagicBlock program id",
    )?;
    require!(
        magic_context.key == &MAGIC_CONTEXT_ID,
        ManifestError::InvalidMagicContextId,
        "Invalid MagicBlock context id",
    )?;

    // Use new_delegated — when this ix runs on the ER, market.owner is the
    // delegation program, not Manifest.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;

    // Verify the market account is at the expected PDA.
    {
        let market_fixed = market.get_fixed()?;
        let (expected_market, _) = get_market_address(
            market_fixed.get_base_mint(),
            market_fixed.get_quote_mint(),
            market_fixed.get_market_id(),
        );
        require!(
            &expected_market == market_info.key,
            ManifestError::InvalidMarketPubkey,
            "Market is not at expected PDA",
        )?;
    }

    commit_accounts(payer.info, vec![market_info], magic_context, magic_program)?;
    Ok(())
}
