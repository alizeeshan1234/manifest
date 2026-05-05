//! SessionToken — base-layer-created authorization that lets an ephemeral
//! keypair sign hot-path instructions (BatchUpdate) on behalf of the real
//! trader. Solves the ER "fee payer must be delegated or it's modified
//! without delegation" warning by giving us a way to sign without prompting
//! the user's primary wallet on every transaction.
//!
//! PDA seeds: [b"session", owner, session_signer]

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use shank::ShankType;
use solana_program::{entrypoint::ProgramResult, program_error::ProgramError, pubkey::Pubkey};
use static_assertions::const_assert_eq;

use crate::{require, validation::ManifestAccount};

pub const SESSION_TOKEN_FIXED_DISCRIMINANT: u64 = 0xCAFE_BABE_DEAD_BEEF;
pub const SESSION_TOKEN_FIXED_SIZE: usize = 96;

#[repr(C)]
#[derive(Default, Copy, Clone, Zeroable, Pod, ShankType)]
pub struct SessionTokenFixed {
    pub discriminant: u64,
    /// The actual trader. Lookups against the market's claimed seats use this
    /// pubkey, not the signer of the BatchUpdate.
    pub owner: Pubkey,
    /// The ephemeral keypair authorized to sign BatchUpdates for the owner.
    pub session_signer: Pubkey,
    /// Unix timestamp after which the token is invalid. 0 = never expires
    /// (use sparingly).
    pub expires_at: i64,
    /// Slot at which the token was created (informational).
    pub created_at_slot: u64,
    pub bump: u8,
    _padding: [u8; 7],
}
const_assert_eq!(size_of::<SessionTokenFixed>(), SESSION_TOKEN_FIXED_SIZE);
const_assert_eq!(size_of::<SessionTokenFixed>() % 8, 0);

impl SessionTokenFixed {
    pub fn new(
        owner: Pubkey,
        session_signer: Pubkey,
        expires_at: i64,
        created_at_slot: u64,
        bump: u8,
    ) -> Self {
        Self {
            discriminant: SESSION_TOKEN_FIXED_DISCRIMINANT,
            owner,
            session_signer,
            expires_at,
            created_at_slot,
            bump,
            _padding: [0; 7],
        }
    }

    pub fn is_expired(&self, now_unix: i64) -> bool {
        self.expires_at != 0 && now_unix >= self.expires_at
    }
}

impl ManifestAccount for SessionTokenFixed {
    fn verify_discriminant(&self) -> ProgramResult {
        require!(
            self.discriminant == SESSION_TOKEN_FIXED_DISCRIMINANT,
            ProgramError::InvalidAccountData,
            "Invalid session token discriminant",
        )?;
        Ok(())
    }
}

impl hypertree::Get for SessionTokenFixed {}

#[macro_export]
macro_rules! session_token_seeds_with_bump {
    ( $owner:expr, $signer:expr, $bump:expr ) => {
        &[&[
            b"session",
            $owner.as_ref(),
            $signer.as_ref(),
            &[$bump],
        ]]
    };
}

pub fn get_session_token_address(owner: &Pubkey, session_signer: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"session", owner.as_ref(), session_signer.as_ref()],
        &crate::ID,
    )
}
