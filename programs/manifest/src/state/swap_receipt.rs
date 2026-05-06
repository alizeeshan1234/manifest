//! SwapReceipt — single-shot scratchpad for a delegated-market swap
//! (Phase B Path A — 1 user signature).
//!
//! Flow:
//!   [1] RequestSwap (base, user signs):
//!         - SPL transfer wallet → input_vault (full input_amount)
//!         - create this receipt with input_amount, min_out,
//!           processed_in=0, processed_out=0
//!         - delegate_account_with_actions(receipt, [ProcessSwapEr])
//!   [2] ProcessSwapEr (ER, validator-signed via post-delegation action):
//!         - claim seat for trader if missing
//!         - virtual deposit input_amount to seat input side
//!         - place IOC at MIN/MAX price → matches the book
//!         - read post-trade balances → processed_in / processed_out
//!         - require processed_out >= min_out (else revert)
//!         - withdraw the gain + residual from seat
//!         - release seat if it was newly created
//!         - write processed_in / processed_out into receipt
//!         - MagicIntentBundleBuilder::commit_and_undelegate(receipt)
//!             .add_post_undelegate_actions([ExecuteSwapBaseChain])
//!   [3] ExecuteSwapBaseChain (base, validator-signed):
//!         - SPL transfer output_vault → trader_token_out for processed_out
//!         - SPL transfer input_vault → trader_token_in for the residual
//!           (input_amount - processed_in), if any
//!         - close receipt, refund rent to trader
//!
//! PDA seeds: [b"swap_receipt", market, trader, input_mint]
//! Single in-flight per (market, trader, input_mint) — sufficient because
//! a swap is direction-specific.

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use shank::ShankType;
use solana_program::{entrypoint::ProgramResult, program_error::ProgramError, pubkey::Pubkey};
use static_assertions::const_assert_eq;

use crate::{require, validation::ManifestAccount};

pub const SWAP_RECEIPT_DISCRIMINANT: u64 = 0x5217_AB05_5A55_C0DE;

// 8 (discriminant) + 32*4 (trader, market, input_mint, output_mint)
// + 8*5 (input_amount, min_out, processed_in, processed_out, slot)
// + 1 (bump) + 1 (is_base_in) + 6 (padding) = 184
pub const SWAP_RECEIPT_SIZE: usize = 184;

#[repr(C)]
#[derive(Default, Copy, Clone, Zeroable, Pod, ShankType)]
pub struct SwapReceiptFixed {
    pub discriminant: u64,
    pub trader: Pubkey,
    pub market: Pubkey,
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    /// Amount the trader transferred to input_vault in step [1].
    pub input_amount: u64,
    /// Minimum acceptable output. ProcessSwapEr reverts if processed_out < min_out.
    pub min_out: u64,
    /// Amount of input actually consumed by matching (≤ input_amount).
    pub processed_in: u64,
    /// Amount of output produced by matching (must be ≥ min_out).
    pub processed_out: u64,
    pub created_at_slot: u64,
    pub bump: u8,
    /// 1 if input is the base mint, 0 if input is the quote mint.
    pub is_base_in: u8,
    _padding: [u8; 6],
}
const_assert_eq!(size_of::<SwapReceiptFixed>(), SWAP_RECEIPT_SIZE);
const_assert_eq!(size_of::<SwapReceiptFixed>() % 8, 0);

impl SwapReceiptFixed {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader: Pubkey,
        market: Pubkey,
        input_mint: Pubkey,
        output_mint: Pubkey,
        input_amount: u64,
        min_out: u64,
        is_base_in: bool,
        created_at_slot: u64,
        bump: u8,
    ) -> Self {
        Self {
            discriminant: SWAP_RECEIPT_DISCRIMINANT,
            trader,
            market,
            input_mint,
            output_mint,
            input_amount,
            min_out,
            processed_in: 0,
            processed_out: 0,
            created_at_slot,
            bump,
            is_base_in: if is_base_in { 1 } else { 0 },
            _padding: [0; 6],
        }
    }
}

impl ManifestAccount for SwapReceiptFixed {
    fn verify_discriminant(&self) -> ProgramResult {
        require!(
            self.discriminant == SWAP_RECEIPT_DISCRIMINANT,
            ProgramError::InvalidAccountData,
            "Invalid swap receipt discriminant",
        )?;
        Ok(())
    }
}

impl hypertree::Get for SwapReceiptFixed {}

#[macro_export]
macro_rules! swap_receipt_seeds_with_bump {
    ( $market:expr, $trader:expr, $input_mint:expr, $bump:expr ) => {
        &[&[
            b"swap_receipt",
            $market.as_ref(),
            $trader.as_ref(),
            $input_mint.as_ref(),
            &[$bump],
        ]]
    };
}

pub fn get_swap_receipt_address(
    market: &Pubkey,
    trader: &Pubkey,
    input_mint: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"swap_receipt",
            market.as_ref(),
            trader.as_ref(),
            input_mint.as_ref(),
        ],
        &crate::ID,
    )
}
