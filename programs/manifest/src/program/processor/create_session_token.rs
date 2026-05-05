//! CreateSessionToken — base layer. The trader signs once with their
//! main wallet to authorize an ephemeral keypair to sign BatchUpdates
//! on the ER for some bounded duration.

use std::mem::size_of;

use borsh::{BorshDeserialize, BorshSerialize};
use hypertree::get_mut_helper;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

use crate::{
    program::ManifestError,
    require,
    state::{get_session_token_address, SessionTokenFixed},
    utils::create_account,
    validation::{EmptyAccount, Program, Signer},
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct CreateSessionTokenParams {
    /// Ephemeral keypair pubkey to authorize.
    pub session_signer: Pubkey,
    /// Unix timestamp after which the token becomes invalid. 0 means never.
    pub expires_at: i64,
}

impl CreateSessionTokenParams {
    pub fn new(session_signer: Pubkey, expires_at: i64) -> Self {
        Self {
            session_signer,
            expires_at,
        }
    }
}

pub(crate) fn process_create_session_token(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let params: CreateSessionTokenParams = CreateSessionTokenParams::try_from_slice(data)?;
    let CreateSessionTokenParams {
        session_signer,
        expires_at,
    } = params;

    let account_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_iter)?;
    let session_token_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;

    let owner: Signer = Signer::new_payer(owner_info)?;
    let _system_program: Program = Program::new(
        system_program_info,
        &solana_program::system_program::id(),
    )?;
    let session_token_empty: EmptyAccount = EmptyAccount::new(session_token_info)?;

    let (expected_pda, bump) = get_session_token_address(owner.key, &session_signer);
    require!(
        &expected_pda == session_token_info.key,
        ManifestError::InvalidSessionTokenPubkey,
        "session_token must be at PDA [b\"session\", owner, session_signer]",
    )?;

    let rent: Rent = Rent::get()?;
    let seeds: Vec<Vec<u8>> = vec![
        b"session".to_vec(),
        owner.key.as_ref().to_vec(),
        session_signer.as_ref().to_vec(),
        vec![bump],
    ];
    create_account(
        owner.as_ref(),
        session_token_empty.as_ref(),
        system_program_info,
        &crate::id(),
        &rent,
        size_of::<SessionTokenFixed>() as u64,
        seeds,
    )?;

    let now_slot: u64 = Clock::get()?.slot;
    let token = SessionTokenFixed::new(*owner.key, session_signer, expires_at, now_slot, bump);

    let mut data = session_token_info.try_borrow_mut_data()?;
    *get_mut_helper::<SessionTokenFixed>(&mut data, 0_u32) = token;

    Ok(())
}
