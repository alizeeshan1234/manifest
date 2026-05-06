//! WithdrawalReceipt — single-shot scratchpad for a delegated-market
//! withdrawal. Mirrors DepositReceipt but stores both the requested
//! amount and the actually-processed amount (ER may clamp to available
//! seat balance).
//!
//! Flow (Phase 9, withdrawal Path A — 1 user signature):
//!   [1] RequestWithdrawal (base, user signs):
//!         - create this receipt with requested_amount, processed_amount=0
//!         - delegate_account_with_actions(receipt, [ProcessWithdrawalEr])
//!   [2] ProcessWithdrawalEr (ER, validator-signed via post-delegation action):
//!         - debit seat by min(requested, available)
//!         - set processed_amount = actual debit
//!         - MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!             .add_post_undelegate_actions([ExecuteWithdrawalBaseChain])
//!   [3] ExecuteWithdrawalBaseChain (base, validator-signed via post-undelegate action):
//!         - SPL transfer market_vault -> trader_token, signed by vault PDA
//!         - close receipt, refund rent to trader
//!
//! PDA seeds: [b"withdraw_receipt", market, trader, mint]
//! Single in-flight per (market, trader, mint).

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use shank::ShankType;
use solana_program::{entrypoint::ProgramResult, program_error::ProgramError, pubkey::Pubkey};
use static_assertions::const_assert_eq;

use crate::{require, validation::ManifestAccount};

pub const WITHDRAWAL_RECEIPT_DISCRIMINANT: u64 = 0xBAAD_F00D_BEEF_CAFE;
// 8 (discriminant) + 32*3 (trader, market, mint) + 8*3 (req, proc, slot)
// + 1 (bump) + 7 (padding) = 136
pub const WITHDRAWAL_RECEIPT_SIZE: usize = 136;

#[repr(C)]
#[derive(Default, Copy, Clone, Zeroable, Pod, ShankType)]
pub struct WithdrawalReceiptFixed {
    pub discriminant: u64,
    pub trader: Pubkey,
    pub market: Pubkey,
    pub mint: Pubkey,
    /// Amount the trader asked for.
    pub requested_amount: u64,
    /// Amount actually approved by ProcessWithdrawalEr after clamping to
    /// available seat balance. Initially 0; set on the ER. Step [3] uses
    /// this value as the SPL transfer amount.
    pub processed_amount: u64,
    pub created_at_slot: u64,
    pub bump: u8,
    _padding: [u8; 7],
}
const_assert_eq!(size_of::<WithdrawalReceiptFixed>(), WITHDRAWAL_RECEIPT_SIZE);
const_assert_eq!(size_of::<WithdrawalReceiptFixed>() % 8, 0);

impl WithdrawalReceiptFixed {
    pub fn new(
        trader: Pubkey,
        market: Pubkey,
        mint: Pubkey,
        requested_amount: u64,
        created_at_slot: u64,
        bump: u8,
    ) -> Self {
        Self {
            discriminant: WITHDRAWAL_RECEIPT_DISCRIMINANT,
            trader,
            market,
            mint,
            requested_amount,
            processed_amount: 0,
            created_at_slot,
            bump,
            _padding: [0; 7],
        }
    }
}

impl ManifestAccount for WithdrawalReceiptFixed {
    fn verify_discriminant(&self) -> ProgramResult {
        require!(
            self.discriminant == WITHDRAWAL_RECEIPT_DISCRIMINANT,
            ProgramError::InvalidAccountData,
            "Invalid withdrawal receipt discriminant",
        )?;
        Ok(())
    }
}

impl hypertree::Get for WithdrawalReceiptFixed {}

#[macro_export]
macro_rules! withdrawal_receipt_seeds_with_bump {
    ( $market:expr, $trader:expr, $mint:expr, $bump:expr ) => {
        &[&[
            b"withdraw_receipt",
            $market.as_ref(),
            $trader.as_ref(),
            $mint.as_ref(),
            &[$bump],
        ]]
    };
}

pub fn get_withdrawal_receipt_address(
    market: &Pubkey,
    trader: &Pubkey,
    mint: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"withdraw_receipt",
            market.as_ref(),
            trader.as_ref(),
            mint.as_ref(),
        ],
        &crate::ID,
    )
}
