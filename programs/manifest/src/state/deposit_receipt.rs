//! DepositReceipt — single-shot scratchpad for a delegated-market deposit.
//!
//! The market_vault itself acts as the escrow (tokens land there directly
//! at step [1]). The receipt only carries the metadata the ER needs to
//! credit the seat and the marker showing whether that has happened.
//!
//! Flow (Phase 10, deposit Path A — 1 user signature):
//!   [1] RequestDeposit (base, user signs):
//!         - SPL transfer wallet -> market_vault
//!         - create this receipt PDA
//!         - delegate_account_with_actions(receipt, [ProcessDepositEr])
//!   [2] ProcessDepositEr (ER, validator-signed via post-delegation action):
//!         - credit seat by `amount`
//!         - set processed = 1
//!         - MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!             .add_post_undelegate_actions([CloseDepositReceipt])
//!   [3] CloseDepositReceipt (base, validator-signed via post-undelegate action):
//!         - drain receipt lamports to trader, assign to system program
//!
//! PDA seeds: [b"deposit_receipt", market, trader, mint]
//! Single in-flight per (market, trader, mint).

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use shank::ShankType;
use solana_program::{entrypoint::ProgramResult, program_error::ProgramError, pubkey::Pubkey};
use static_assertions::const_assert_eq;

use crate::{require, validation::ManifestAccount};

pub const DEPOSIT_RECEIPT_DISCRIMINANT: u64 = 0xD0D0_BEEF_F00D_CAFE;
// 8 (discriminant) + 32*3 (trader, market, mint) + 8*2 (amount, slot)
// + 1 (processed) + 1 (bump) + 6 (padding) = 128
pub const DEPOSIT_RECEIPT_SIZE: usize = 128;

#[repr(C)]
#[derive(Default, Copy, Clone, Zeroable, Pod, ShankType)]
pub struct DepositReceiptFixed {
    pub discriminant: u64,
    pub trader: Pubkey,
    pub market: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub created_at_slot: u64,
    /// 0 = pending; 1 = ER credited the seat. CloseDepositReceipt only
    /// runs if this is 1, so a never-completed deposit can't be silently
    /// dropped — the trader can either retry [2] or recover via cancel.
    pub processed: u8,
    pub bump: u8,
    _padding: [u8; 6],
}
const_assert_eq!(size_of::<DepositReceiptFixed>(), DEPOSIT_RECEIPT_SIZE);
const_assert_eq!(size_of::<DepositReceiptFixed>() % 8, 0);

impl DepositReceiptFixed {
    pub fn new(
        trader: Pubkey,
        market: Pubkey,
        mint: Pubkey,
        amount: u64,
        created_at_slot: u64,
        bump: u8,
    ) -> Self {
        Self {
            discriminant: DEPOSIT_RECEIPT_DISCRIMINANT,
            trader,
            market,
            mint,
            amount,
            created_at_slot,
            processed: 0,
            bump,
            _padding: [0; 6],
        }
    }
}

impl ManifestAccount for DepositReceiptFixed {
    fn verify_discriminant(&self) -> ProgramResult {
        require!(
            self.discriminant == DEPOSIT_RECEIPT_DISCRIMINANT,
            ProgramError::InvalidAccountData,
            "Invalid deposit receipt discriminant",
        )?;
        Ok(())
    }
}

impl hypertree::Get for DepositReceiptFixed {}

#[macro_export]
macro_rules! deposit_receipt_seeds_with_bump {
    ( $market:expr, $trader:expr, $mint:expr, $bump:expr ) => {
        &[&[
            b"deposit_receipt",
            $market.as_ref(),
            $trader.as_ref(),
            $mint.as_ref(),
            &[$bump],
        ]]
    };
}

pub fn get_deposit_receipt_address(
    market: &Pubkey,
    trader: &Pubkey,
    mint: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"deposit_receipt",
            market.as_ref(),
            trader.as_ref(),
            mint.as_ref(),
        ],
        &crate::ID,
    )
}
