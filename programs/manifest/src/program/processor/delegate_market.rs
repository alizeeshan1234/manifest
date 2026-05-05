//! DelegateMarket — base-layer instruction that hands market account
//! ownership to the MagicBlock delegation program so the market can be
//! mutated on the ER.
//!
//! Flow:
//!   1. Verify caller is the market's authority.
//!   2. Verify the market has at least `min_free_blocks` free blocks
//!      reserved (since the ER cannot realloc under disable-realloc).
//!   3. CPI into the MagicBlock delegation program (vendored helpers in
//!      `crate::magicblock`).

use std::cell::Ref;

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use crate::{
    magicblock::cpi::{delegate_account, DelegateAccounts, DelegateConfig},
    program::{get_dynamic_account, ManifestError},
    require,
    state::MarketFixed,
    validation::{ManifestAccountInfo, Signer},
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct DelegateMarketParams {
    /// Minimum number of free blocks that must be reserved on the market
    /// before delegation. The ER cannot realloc under disable-realloc, so
    /// every order, seat, and fill that runs there must come out of this
    /// reservation.
    pub min_free_blocks: u32,
}

impl DelegateMarketParams {
    pub fn new(min_free_blocks: u32) -> Self {
        Self { min_free_blocks }
    }
}

pub(crate) fn process_delegate_market(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let params: DelegateMarketParams = DelegateMarketParams::try_from_slice(data)?;
    let DelegateMarketParams { min_free_blocks } = params;

    let account_iter = &mut accounts.iter();
    let authority_info = next_account_info(account_iter)?;
    let system_program_info = next_account_info(account_iter)?;
    let market_info = next_account_info(account_iter)?;
    let owner_program_info = next_account_info(account_iter)?;
    let delegation_buffer = next_account_info(account_iter)?;
    let delegation_record = next_account_info(account_iter)?;
    let delegation_metadata = next_account_info(account_iter)?;
    let delegation_program = next_account_info(account_iter)?;

    let authority: Signer = Signer::new_payer(authority_info)?;

    // Verify the owner program slot is Manifest itself.
    require!(
        owner_program_info.key == &crate::ID,
        ManifestError::IncorrectAccount,
        "owner_program account must be Manifest",
    )?;

    // Load market under the regular (non-delegated) loader — this ix is
    // base-layer-only and the market must currently be Manifest-owned.
    let market: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new(market_info)?;

    let (base_mint, quote_mint, market_id) = {
        let market_fixed: Ref<MarketFixed> = market.get_fixed()?;

        // Authority gate.
        let market_authority: &Pubkey = market_fixed.get_authority();
        require!(
            market_authority != &Pubkey::default(),
            ManifestError::MarketNotDelegatable,
            "Market authority is unset; market is non-delegatable",
        )?;
        require!(
            market_authority == authority.key,
            ManifestError::UnauthorizedDelegation,
            "Caller {} is not the market authority {}",
            authority.key,
            market_authority,
        )?;

        (
            *market_fixed.get_base_mint(),
            *market_fixed.get_quote_mint(),
            market_fixed.get_market_id(),
        )
    };

    // Free-block reservation precondition. ER can't realloc.
    {
        let market_data = market_info.try_borrow_data()?;
        let dyn_market = get_dynamic_account::<MarketFixed>(&market_data);
        if let Some(short_by) = dyn_market.free_blocks_short_of_n(min_free_blocks) {
            require!(
                short_by == 0,
                ManifestError::InsufficientFreeBlocks,
                "Market has {} fewer free blocks than required {}",
                short_by,
                min_free_blocks,
            )?;
        }
    }

    // CPI into the delegation program. Sign with market PDA seeds.
    let market_id_byte: [u8; 1] = [market_id];
    let pda_seeds: &[&[u8]] = &[
        b"market",
        base_mint.as_ref(),
        quote_mint.as_ref(),
        &market_id_byte,
    ];

    delegate_account(
        DelegateAccounts {
            payer: authority_info,
            pda: market_info,
            owner_program: owner_program_info,
            buffer: delegation_buffer,
            delegation_record,
            delegation_metadata,
            delegation_program,
            system_program: system_program_info,
        },
        pda_seeds,
        DelegateConfig {
            // Per locked decision #4: manual-only commits.
            commit_frequency_ms: u32::MAX,
            validator: None,
        },
    )?;

    Ok(())
}
