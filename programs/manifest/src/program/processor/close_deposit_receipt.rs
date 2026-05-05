//! CloseDepositReceipt — base layer, auto-fired post-undelegate action.
//!
//! Phase 10 deposit Path A, step [3]. No user signer; the MagicBlock
//! validator signs the outer tx as part of the post-undelegate action
//! bundle scheduled by ProcessDepositEr.
//!
//! Tokens are already in market_vault from step [1] and the seat is
//! already credited from step [2], so this ix just needs to close the
//! receipt and refund rent to the trader.
//!
//! Account list:
//!   [0] trader   (writable, NOT a signer — pubkey verified vs receipt.trader)
//!   [1] receipt  (writable — closed)

use std::cell::Ref;

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::{
    program::ManifestError,
    require,
    state::DepositReceiptFixed,
    validation::ManifestAccountInfo,
};

pub(crate) fn process_close_deposit_receipt(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;

    // Receipt must be back under Manifest ownership (post-undelegate).
    let receipt: ManifestAccountInfo<DepositReceiptFixed> =
        ManifestAccountInfo::<DepositReceiptFixed>::new(receipt_info)?;

    {
        let r: Ref<DepositReceiptFixed> = receipt.get_fixed()?;
        require!(
            r.trader == *trader_info.key,
            ManifestError::IncorrectAccount,
            "Receipt trader mismatch",
        )?;
        require!(
            r.processed == 1,
            ManifestError::DepositNotProcessed,
            "Cannot close an unprocessed receipt",
        )?;
    }

    // Drain receipt lamports to trader, zero data, assign to system program.
    let dest_starting = trader_info.lamports();
    **trader_info.lamports.borrow_mut() =
        dest_starting.checked_add(receipt_info.lamports()).unwrap();
    **receipt_info.lamports.borrow_mut() = 0;
    receipt_info.assign(&solana_program::system_program::id());
    #[allow(deprecated)]
    receipt_info.realloc(0, false)?;

    Ok(())
}
