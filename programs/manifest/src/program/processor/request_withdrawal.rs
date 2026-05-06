//! RequestWithdrawal — base layer, single user signature.
//!
//! Phase 9 withdrawal Path A, step [1]. Creates the WithdrawalReceipt
//! PDA and delegates it to the ER WITH a post-delegation action that
//! auto-fires ProcessWithdrawalEr. No tokens move yet — that happens
//! in step [3] after the ER side has confirmed the seat debit.
//!
//! Account list:
//!   [0]  trader               (signer, writable, payer)
//!   [1]  market               (read-only; expected delegated)
//!   [2]  receipt              (writable, empty — created + delegated)
//!   [3]  mint
//!   [4]  system_program
//!   [5]  owner_program        (= Manifest)
//!   [6]  delegation_buffer    (writable)
//!   [7]  delegation_record    (writable)
//!   [8]  delegation_metadata  (writable)
//!   [9]  delegation_program
//!   [10] magic_program        (passed through to the post-delegation ix)
//!   [11] magic_context        (writable; same)
//!   [12] market_vault         (passed through to the post-undelegate ix
//!                              for the SPL transfer destination side)
//!   [13] trader_token         (passed through to the post-undelegate ix)
//!   [14] token_program        (passed through to the post-undelegate ix)

use std::{cell::Ref, mem::size_of};

use borsh::{BorshDeserialize, BorshSerialize};
use dlp_api::compact::ClearText;
use ephemeral_rollups_sdk::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    cpi::{delegate_account_with_actions, DelegateAccounts, DelegateConfig},
};
use hypertree::get_mut_helper;
use solana_instruction::{AccountMeta as SolAccountMeta, Instruction as SolInstruction};
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
    state::{get_withdrawal_receipt_address, MarketFixed, WithdrawalReceiptFixed},
    utils::create_account,
    validation::{
        get_vault_address, EmptyAccount, ManifestAccountInfo, MintAccountInfo, Program, Signer,
    },
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct RequestWithdrawalParams {
    pub amount: u64,
}

impl RequestWithdrawalParams {
    pub fn new(amount: u64) -> Self {
        Self { amount }
    }
}

pub(crate) fn process_request_withdrawal(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let params: RequestWithdrawalParams = RequestWithdrawalParams::try_from_slice(data)?;
    let RequestWithdrawalParams { amount } = params;

    require!(
        amount > 0,
        ManifestError::InvalidWithdrawAccounts,
        "RequestWithdrawal amount must be > 0",
    )?;

    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let mint_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;
    let owner_program_info = next_account_info(account_iter)?;
    let delegation_buffer = next_account_info(account_iter)?;
    let delegation_record = next_account_info(account_iter)?;
    let delegation_metadata = next_account_info(account_iter)?;
    let delegation_program = next_account_info(account_iter)?;
    let magic_program_info = next_account_info(account_iter)?;
    let magic_context_info = next_account_info(account_iter)?;
    let market_vault_info = next_account_info(account_iter)?;
    let trader_token_info = next_account_info(account_iter)?;
    let token_program_info = next_account_info(account_iter)?;

    let trader: Signer = Signer::new_payer(trader_info)?;
    let _system_program: Program =
        Program::new(system_program_info, &solana_program::system_program::id())?;
    let receipt_empty: EmptyAccount = EmptyAccount::new(receipt_info)?;
    let _mint: MintAccountInfo = MintAccountInfo::new(mint_info)?;

    require!(
        owner_program_info.key == &crate::ID,
        ManifestError::IncorrectAccount,
        "owner_program must be Manifest",
    )?;
    require!(
        magic_program_info.key.to_bytes() == MAGIC_PROGRAM_ID.to_bytes(),
        ManifestError::InvalidMagicProgramId,
        "Wrong MagicBlock program",
    )?;
    require!(
        magic_context_info.key.to_bytes() == MAGIC_CONTEXT_ID.to_bytes(),
        ManifestError::InvalidMagicContextId,
        "Wrong MagicBlock context",
    )?;
    require!(
        market_info.owner.to_bytes()
            == ephemeral_rollups_sdk::consts::DELEGATION_PROGRAM_ID.to_bytes(),
        ManifestError::MarketNotDelegated,
        "Market is not delegated; use Withdraw instead",
    )?;

    // Read the (delegated) market header to verify the mint.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;
    let expected_vault: Pubkey = {
        let fixed: Ref<MarketFixed> = market.get_fixed()?;
        if mint_info.key == fixed.get_base_mint() {
            *fixed.get_base_vault()
        } else if mint_info.key == fixed.get_quote_mint() {
            *fixed.get_quote_vault()
        } else {
            return Err(ManifestError::InvalidWithdrawalMint.into());
        }
    };
    require!(
        expected_vault == *market_vault_info.key,
        ManifestError::IncorrectAccount,
        "market_vault account mismatch for this mint",
    )?;
    let (vault_pda, _vault_bump) = get_vault_address(market_info.key, mint_info.key);
    require!(
        vault_pda == *market_vault_info.key,
        ManifestError::IncorrectAccount,
        "market_vault PDA mismatch",
    )?;

    let (expected_receipt, receipt_bump) =
        get_withdrawal_receipt_address(market_info.key, trader.key, mint_info.key);
    require!(
        &expected_receipt == receipt_info.key,
        ManifestError::InvalidWithdrawalReceiptPubkey,
        "WithdrawalReceipt PDA mismatch",
    )?;

    // Create the receipt PDA, owned by Manifest.
    let rent: Rent = Rent::get()?;
    let receipt_create_seeds: Vec<Vec<u8>> = vec![
        b"withdraw_receipt".to_vec(),
        market_info.key.as_ref().to_vec(),
        trader.key.as_ref().to_vec(),
        mint_info.key.as_ref().to_vec(),
        vec![receipt_bump],
    ];
    create_account(
        trader.as_ref(),
        receipt_empty.as_ref(),
        system_program_info,
        &crate::id(),
        &rent,
        size_of::<WithdrawalReceiptFixed>() as u64,
        receipt_create_seeds,
    )?;

    let now_slot: u64 = Clock::get()?.slot;
    let receipt = WithdrawalReceiptFixed::new(
        *trader.key,
        *market_info.key,
        *mint_info.key,
        amount,
        now_slot,
        receipt_bump,
    );
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        *get_mut_helper::<WithdrawalReceiptFixed>(&mut data, 0_u32) = receipt;
    }

    // Build the post-delegation ProcessWithdrawalEr ix. Account ordering
    // mirrors ProcessWithdrawalEr's expected list. We pass the base-side
    // accounts (market_vault, trader_token, mint, token_program) through
    // so the ER handler can hand them off to the post-undelegate
    // ExecuteWithdrawalBaseChain action.
    let process_er_ix = SolInstruction {
        program_id: crate::ID.to_bytes().into(),
        accounts: vec![
            SolAccountMeta::new(trader.key.to_bytes().into(), true),
            SolAccountMeta::new(market_info.key.to_bytes().into(), false),
            SolAccountMeta::new(receipt_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(magic_program_info.key.to_bytes().into(), false),
            SolAccountMeta::new(magic_context_info.key.to_bytes().into(), false),
            SolAccountMeta::new(market_vault_info.key.to_bytes().into(), false),
            SolAccountMeta::new(trader_token_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(mint_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(token_program_info.key.to_bytes().into(), false),
        ],
        data: vec![24u8], // ProcessWithdrawalEr discriminator
    };

    let actions = vec![process_er_ix].cleartext();

    // Delegate the receipt with the post-delegation action.
    let pda_seeds: &[&[u8]] = &[
        b"withdraw_receipt",
        market_info.key.as_ref(),
        trader.key.as_ref(),
        mint_info.key.as_ref(),
    ];
    delegate_account_with_actions(
        DelegateAccounts {
            payer: trader_info,
            pda: receipt_info,
            owner_program: owner_program_info,
            buffer: delegation_buffer,
            delegation_record,
            delegation_metadata,
            delegation_program,
            system_program: system_program_info,
        },
        pda_seeds,
        DelegateConfig {
            commit_frequency_ms: u32::MAX,
            validator: None,
        },
        actions,
        &[trader_info],
    )?;

    Ok(())
}
