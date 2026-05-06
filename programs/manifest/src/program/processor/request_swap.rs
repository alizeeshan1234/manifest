//! RequestSwap — base layer, single user signature.
//!
//! Phase B swap Path A, step [1]. Performs the input-side SPL transfer,
//! creates the SwapReceipt PDA, and delegates it to the ER WITH a
//! post-delegation action that auto-fires ProcessSwapEr on the ER. The
//! swap's matching + output transfer happen in steps [2] and [3]; the
//! trader does not sign again.
//!
//! Account list:
//!   [0]  trader               (signer, writable, payer)
//!   [1]  market               (read-only; expected delegated)
//!   [2]  input_vault          (writable; SPL transfer destination)
//!   [3]  output_vault         (passed through to step [3])
//!   [4]  receipt              (writable, empty — created + delegated)
//!   [5]  trader_token_in      (writable; SPL transfer source)
//!   [6]  trader_token_out     (passed through to step [3])
//!   [7]  input_mint
//!   [8]  output_mint
//!   [9]  system_program
//!   [10] token_program
//!   [11] owner_program        (= Manifest)
//!   [12] delegation_buffer    (writable)
//!   [13] delegation_record    (writable)
//!   [14] delegation_metadata  (writable)
//!   [15] delegation_program
//!   [16] magic_program
//!   [17] magic_context        (writable)

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
    program::invoke,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

use crate::{
    program::ManifestError,
    require,
    state::{get_swap_receipt_address, MarketFixed, SwapReceiptFixed},
    utils::create_account,
    validation::{
        get_vault_address, EmptyAccount, ManifestAccountInfo, MintAccountInfo, Program, Signer,
    },
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct RequestSwapParams {
    pub input_amount: u64,
    pub min_out: u64,
}

impl RequestSwapParams {
    pub fn new(input_amount: u64, min_out: u64) -> Self {
        Self {
            input_amount,
            min_out,
        }
    }
}

pub(crate) fn process_request_swap(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let RequestSwapParams {
        input_amount,
        min_out,
    } = RequestSwapParams::try_from_slice(data)?;

    require!(
        input_amount > 0,
        ManifestError::InvalidSwapAccounts,
        "RequestSwap input_amount must be > 0",
    )?;

    let account_iter = &mut accounts.iter();
    let trader_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let input_vault_info = next_account_info(account_iter)?;
    let output_vault_info = next_account_info(account_iter)?;
    let receipt_info = next_account_info(account_iter)?;
    let trader_token_in_info = next_account_info(account_iter)?;
    let trader_token_out_info = next_account_info(account_iter)?;
    let input_mint_info = next_account_info(account_iter)?;
    let output_mint_info = next_account_info(account_iter)?;
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
    let input_mint: MintAccountInfo = MintAccountInfo::new(input_mint_info)?;
    let _output_mint: MintAccountInfo = MintAccountInfo::new(output_mint_info)?;

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
        "Market is not delegated; use Swap on base instead",
    )?;
    require!(
        input_mint_info.key != output_mint_info.key,
        ManifestError::InvalidSwapAccounts,
        "input_mint must differ from output_mint",
    )?;

    // Validate mints + vault PDAs.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new_delegated(market_info)?;
    let (is_base_in, decimals, expected_input_vault, expected_output_vault) = {
        let fixed: Ref<MarketFixed> = market.get_fixed()?;
        if input_mint.info.key == fixed.get_base_mint()
            && output_mint_info.key == fixed.get_quote_mint()
        {
            (
                true,
                fixed.get_base_mint_decimals(),
                *fixed.get_base_vault(),
                *fixed.get_quote_vault(),
            )
        } else if input_mint.info.key == fixed.get_quote_mint()
            && output_mint_info.key == fixed.get_base_mint()
        {
            (
                false,
                fixed.get_quote_mint_decimals(),
                *fixed.get_quote_vault(),
                *fixed.get_base_vault(),
            )
        } else {
            return Err(ManifestError::InvalidSwapMint.into());
        }
    };
    require!(
        expected_input_vault == *input_vault_info.key,
        ManifestError::IncorrectAccount,
        "input_vault account mismatch",
    )?;
    require!(
        expected_output_vault == *output_vault_info.key,
        ManifestError::IncorrectAccount,
        "output_vault account mismatch",
    )?;
    let (input_vault_pda, _bump1) = get_vault_address(market_info.key, input_mint_info.key);
    let (output_vault_pda, _bump2) = get_vault_address(market_info.key, output_mint_info.key);
    require!(
        input_vault_pda == *input_vault_info.key,
        ManifestError::IncorrectAccount,
        "input_vault PDA mismatch",
    )?;
    require!(
        output_vault_pda == *output_vault_info.key,
        ManifestError::IncorrectAccount,
        "output_vault PDA mismatch",
    )?;

    // Receipt PDA check.
    let (expected_receipt, receipt_bump) =
        get_swap_receipt_address(market_info.key, trader.key, input_mint_info.key);
    require!(
        &expected_receipt == receipt_info.key,
        ManifestError::InvalidSwapReceiptPubkey,
        "SwapReceipt PDA mismatch",
    )?;

    // 1. SPL transfer wallet -> input_vault, signed by trader.
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
                trader_token_in_info.key,
                input_mint_info.key,
                input_vault_info.key,
                trader.key,
                &[],
                input_amount,
                decimals,
            )?,
            &[
                token_program_info.clone(),
                trader_token_in_info.clone(),
                input_mint_info.clone(),
                input_vault_info.clone(),
                trader_info.clone(),
            ],
        )?;
    } else {
        invoke(
            &spl_token::instruction::transfer(
                &token_program_id,
                trader_token_in_info.key,
                input_vault_info.key,
                trader.key,
                &[],
                input_amount,
            )?,
            &[
                token_program_info.clone(),
                trader_token_in_info.clone(),
                input_vault_info.clone(),
                trader_info.clone(),
            ],
        )?;
    }

    // 2. Create receipt PDA owned by Manifest.
    let rent: Rent = Rent::get()?;
    let receipt_create_seeds: Vec<Vec<u8>> = vec![
        b"swap_receipt".to_vec(),
        market_info.key.as_ref().to_vec(),
        trader.key.as_ref().to_vec(),
        input_mint_info.key.as_ref().to_vec(),
        vec![receipt_bump],
    ];
    create_account(
        trader.as_ref(),
        receipt_empty.as_ref(),
        system_program_info,
        &crate::id(),
        &rent,
        size_of::<SwapReceiptFixed>() as u64,
        receipt_create_seeds,
    )?;

    let now_slot: u64 = Clock::get()?.slot;
    let receipt = SwapReceiptFixed::new(
        *trader.key,
        *market_info.key,
        *input_mint_info.key,
        *output_mint_info.key,
        input_amount,
        min_out,
        is_base_in,
        now_slot,
        receipt_bump,
    );
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        *get_mut_helper::<SwapReceiptFixed>(&mut data, 0_u32) = receipt;
    }

    // 3. Build the post-delegation ProcessSwapEr ix. Account ordering
    //    must match the ProcessSwapEr handler. Pass through the base-side
    //    accounts so step [3] (ExecuteSwapBaseChain) can settle the SPL
    //    transfers without needing to look anything up.
    let process_er_ix = SolInstruction {
        program_id: crate::ID.to_bytes().into(),
        accounts: vec![
            SolAccountMeta::new(trader.key.to_bytes().into(), true),
            SolAccountMeta::new(market_info.key.to_bytes().into(), false),
            SolAccountMeta::new(receipt_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(magic_program_info.key.to_bytes().into(), false),
            SolAccountMeta::new(magic_context_info.key.to_bytes().into(), false),
            SolAccountMeta::new(input_vault_info.key.to_bytes().into(), false),
            SolAccountMeta::new(output_vault_info.key.to_bytes().into(), false),
            SolAccountMeta::new(trader_token_in_info.key.to_bytes().into(), false),
            SolAccountMeta::new(trader_token_out_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(input_mint_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(output_mint_info.key.to_bytes().into(), false),
            SolAccountMeta::new_readonly(token_program_info.key.to_bytes().into(), false),
        ],
        data: vec![27u8], // ProcessSwapEr discriminator
    };

    let actions = vec![process_er_ix].cleartext();

    let pda_seeds: &[&[u8]] = &[
        b"swap_receipt",
        market_info.key.as_ref(),
        trader.key.as_ref(),
        input_mint_info.key.as_ref(),
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
