//! RequestDeposit — base layer, single user signature.
//!
//! Phase 10 deposit Path A, step [1]. The trader signs once. We:
//!   1. SPL transfer wallet -> market_vault (the vault doubles as escrow)
//!   2. Create the DepositReceipt PDA recording (trader, mint, amount)
//!   3. Delegate the receipt to the ER WITH a post-delegation action
//!      that auto-fires ProcessDepositEr on the ER, signed by the trader
//!      via the SDK's escrow-authority mechanism.
//!
//! Steps [2] (ProcessDepositEr) and [3] (CloseDepositReceipt) run as
//! validator-signed callbacks — the trader does not need to sign again.
//!
//! Account list (passed in by the trader):
//!   [0]  trader               (signer, writable, payer)
//!   [1]  market               (read-only; expected delegated)
//!   [2]  market_vault         (writable; SPL transfer destination)
//!   [3]  receipt              (writable, empty — will be created + delegated)
//!   [4]  trader_token         (writable; SPL transfer source)
//!   [5]  mint
//!   [6]  system_program
//!   [7]  token_program
//!   [8]  owner_program        (= Manifest, for delegation CPI)
//!   [9]  delegation_buffer    (writable)
//!   [10] delegation_record    (writable)
//!   [11] delegation_metadata  (writable)
//!   [12] delegation_program
//!   [13] magic_program        (passed as a non-signer account in the
//!                              post-delegation ProcessDepositEr ix)
//!   [14] magic_context        (writable; same purpose)

use std::{cell::Ref, mem::size_of};

use borsh::{BorshDeserialize, BorshSerialize};
use ephemeral_rollups_sdk::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    cpi::{delegate_account_with_actions, DelegateAccounts, DelegateConfig},
};
use hypertree::get_mut_helper;
use dlp_api::compact::ClearText;
use solana_instruction::{AccountMeta as SolAccountMeta, Instruction as SolInstruction};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program::invoke,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

use crate::{
    program::ManifestError,
    require,
    state::{get_deposit_receipt_address, DepositReceiptFixed, MarketFixed},
    utils::create_account,
    validation::{
        get_vault_address, EmptyAccount, ManifestAccountInfo, MintAccountInfo, Program, Signer,
    },
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct RequestDepositParams {
    pub amount: u64,
}

impl RequestDepositParams {
    pub fn new(amount: u64) -> Self {
        Self { amount }
    }
}

pub(crate) fn process_request_deposit(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let params: RequestDepositParams = RequestDepositParams::try_from_slice(data)?;
    let RequestDepositParams { amount } = params;

    require!(
        amount > 0,
        ManifestError::InvalidDepositAccounts,
        "RequestDeposit amount must be > 0",
    )?;

    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let market_vault_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let trader_token_info = next_account_info(account_iter)?;
    let mint_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;
    let token_program_info = next_account_info(account_iter)?;
    let owner_program_info = next_account_info(account_iter)?;
    let delegation_buffer = next_account_info(account_iter)?;
    let delegation_record = next_account_info(account_iter)?;
    let delegation_metadata = next_account_info(account_iter)?;
    let delegation_program = next_account_info(account_iter)?;
    let magic_program_info = next_account_info(account_iter)?;
    let magic_context_info = next_account_info(account_iter)?;

    let trader: Signer = Signer::new_payer(trader_info)?;
    let _system_program: Program =
        Program::new(system_program_info, &solana_program::system_program::id())?;
    let receipt_empty: EmptyAccount = EmptyAccount::new(receipt_info)?;
    let mint: MintAccountInfo = MintAccountInfo::new(mint_info)?;

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
        market_info.owner.to_bytes() == ephemeral_rollups_sdk::consts::DELEGATION_PROGRAM_ID.to_bytes(),
        ManifestError::MarketNotDelegated,
        "Market is not delegated; use Deposit instead",
    )?;

    // Read the (delegated) market header to validate mint + decimals.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;
    let (is_base, decimals, expected_vault) = {
        let fixed: Ref<MarketFixed> = market.get_fixed()?;
        if mint.info.key == fixed.get_base_mint() {
            (true, fixed.get_base_mint_decimals(), *fixed.get_base_vault())
        } else if mint.info.key == fixed.get_quote_mint() {
            (false, fixed.get_quote_mint_decimals(), *fixed.get_quote_vault())
        } else {
            return Err(ManifestError::InvalidDepositMint.into());
        }
    };
    require!(
        expected_vault == *market_vault_info.key,
        ManifestError::IncorrectAccount,
        "market_vault account mismatch for this mint",
    )?;
    let _ = is_base;

    // Receipt PDA check.
    let (expected_receipt, receipt_bump) =
        get_deposit_receipt_address(market_info.key, trader.key, mint_info.key);
    require!(
        &expected_receipt == receipt_info.key,
        ManifestError::InvalidDepositReceiptPubkey,
        "DepositReceipt PDA mismatch",
    )?;

    // Sanity: market_vault must be at the expected vault PDA.
    let (vault_pda, _vault_bump) = get_vault_address(market_info.key, mint_info.key);
    require!(
        vault_pda == *market_vault_info.key,
        ManifestError::IncorrectAccount,
        "market_vault PDA mismatch",
    )?;

    // 1. SPL transfer wallet -> market_vault, signed by trader.
    let token_program_id: Pubkey = *token_program_info.key;
    require!(
        token_program_id == spl_token::id() || token_program_id == spl_token_2022::id(),
        ManifestError::IncorrectAccount,
        "token_program must be SPL Token or Token-2022",
    )?;

    if token_program_id == spl_token_2022::id() {
        invoke(
            &spl_token_2022::instruction::transfer_checked(
                &token_program_id,
                trader_token_info.key,
                mint_info.key,
                market_vault_info.key,
                trader.key,
                &[],
                amount,
                decimals,
            )?,
            &[
                token_program_info.clone(),
                trader_token_info.clone(),
                mint_info.clone(),
                market_vault_info.clone(),
                trader_info.clone(),
            ],
        )?;
    } else {
        invoke(
            &spl_token::instruction::transfer(
                &token_program_id,
                trader_token_info.key,
                market_vault_info.key,
                trader.key,
                &[],
                amount,
            )?,
            &[
                token_program_info.clone(),
                trader_token_info.clone(),
                market_vault_info.clone(),
                trader_info.clone(),
            ],
        )?;
    }

    // 2. Create the receipt PDA, owned by Manifest.
    let rent: Rent = Rent::get()?;
    let receipt_create_seeds: Vec<Vec<u8>> = vec![
        b"deposit_receipt".to_vec(),
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
        size_of::<DepositReceiptFixed>() as u64,
        receipt_create_seeds,
    )?;

    let now_slot: u64 = Clock::get()?.slot;
    let receipt = DepositReceiptFixed::new(
        *trader.key,
        *market_info.key,
        *mint_info.key,
        amount,
        now_slot,
        receipt_bump,
    );
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        *get_mut_helper::<DepositReceiptFixed>(&mut data, 0_u32) = receipt;
    }

    // 3. Build the post-delegation ProcessDepositEr ix (using the
    //    solana-instruction 3.x types the SDK's ClearText impl expects).
    let process_er_ix = SolInstruction {
        program_id: crate::ID.to_bytes().into(),
        accounts: vec![
            SolAccountMeta::new(trader.key.to_bytes().into(), true),
            SolAccountMeta::new(market_info.key.to_bytes().into(), false),
            SolAccountMeta::new(receipt_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(magic_program_info.key.to_bytes().into(), false),
            SolAccountMeta::new(magic_context_info.key.to_bytes().into(), false),
        ],
        data: vec![21u8], // ProcessDepositEr discriminator
    };

    let actions = vec![process_er_ix].cleartext();

    // 4. Delegate the receipt with the post-delegation action.
    let pda_seeds: &[&[u8]] = &[
        b"deposit_receipt",
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
