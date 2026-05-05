//! RevokeSessionToken — close the SessionToken PDA and return rent to
//! the owner.

use std::cell::Ref;

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::{
    program::ManifestError,
    require,
    state::SessionTokenFixed,
    validation::{ManifestAccountInfo, Signer},
};

pub(crate) fn process_revoke_session_token(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_iter)?;
    let session_token_info = next_account_info(account_iter)?;

    let owner: Signer = Signer::new_payer(owner_info)?;
    let token: ManifestAccountInfo<SessionTokenFixed> =
        ManifestAccountInfo::<SessionTokenFixed>::new(session_token_info)?;

    {
        let fixed: Ref<SessionTokenFixed> = token.get_fixed()?;
        require!(
            fixed.owner == *owner.key,
            ManifestError::InvalidSessionSigner,
            "Only the token owner can revoke",
        )?;
    }

    // Close: drain lamports to owner, zero data, assign to system program.
    let dest_starting = owner_info.lamports();
    **owner_info.lamports.borrow_mut() =
        dest_starting.checked_add(session_token_info.lamports()).unwrap();
    **session_token_info.lamports.borrow_mut() = 0;
    session_token_info.assign(&solana_program::system_program::id());
    #[allow(deprecated)]
    session_token_info.realloc(0, false)?;
    Ok(())
}
