use std::{cell::Ref, mem::size_of};

use crate::{
    logs::{emit_stack, CreateMarketLog},
    program::{expand_market_if_needed, invoke},
    require,
    state::MarketFixed,
    utils::create_account,
    validation::{
        get_market_address, get_vault_address, loaders::CreateMarketContext, ManifestAccountInfo,
    },
};
use borsh::{BorshDeserialize, BorshSerialize};
use hypertree::{get_mut_helper, trace};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program::invoke_signed,
    program_pack::Pack, pubkey::Pubkey, rent::Rent, sysvar::Sysvar,
};
use spl_token_2022::{
    extension::{
        mint_close_authority::MintCloseAuthority, permanent_delegate::PermanentDelegate,
        BaseStateWithExtensions, ExtensionType, PodStateWithExtensions, StateWithExtensions,
    },
    pod::PodMint,
    state::{Account, Mint},
};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct CreateMarketParams {
    /// Disambiguator allowing multiple markets per (base, quote) pair.
    pub market_id: u8,
    /// Authority allowed to delegate / undelegate this market to MagicBlock ER.
    /// Pass `Pubkey::default()` to make the market non-delegatable.
    pub authority: Pubkey,
}

impl CreateMarketParams {
    pub fn new(market_id: u8, authority: Pubkey) -> Self {
        CreateMarketParams {
            market_id,
            authority,
        }
    }
}

pub(crate) fn process_create_market(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    trace!("process_create_market accs={accounts:?}");

    let params: CreateMarketParams = CreateMarketParams::try_from_slice(data)?;
    let CreateMarketParams {
        market_id,
        authority,
    } = params;

    let create_market_context: CreateMarketContext = CreateMarketContext::load(accounts, market_id)?;

    let CreateMarketContext {
        market,
        payer,
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        system_program,
        token_program,
        token_program_22,
    } = create_market_context;

    require!(
        base_mint.info.key != quote_mint.info.key,
        crate::program::ManifestError::InvalidMarketParameters,
        "Base and quote must be different",
    )?;

    for mint in [base_mint.as_ref(), quote_mint.as_ref()] {
        if *mint.owner == spl_token_2022::id() {
            let mint_data: Ref<'_, &mut [u8]> = mint.data.borrow();
            let pool_mint: StateWithExtensions<'_, Mint> =
                StateWithExtensions::<Mint>::unpack(&mint_data)?;
            // Closable mints can be replaced with different ones, breaking some saved info on the market.
            if let Ok(extension) = pool_mint.get_extension::<MintCloseAuthority>() {
                let close_authority: Option<Pubkey> = extension.close_authority.into();
                if close_authority.is_some() {
                    solana_program::msg!(
                        "Warning, you are creating a market with a close authority."
                    );
                }
            }
            // Permanent delegates can steal your tokens. This will break all
            // accounting in the market, so there is no assertion of security
            // against loss of funds on these markets.
            if let Ok(extension) = pool_mint.get_extension::<PermanentDelegate>() {
                let permanent_delegate: Option<Pubkey> = extension.delegate.into();
                if permanent_delegate.is_some() {
                    solana_program::msg!(
                        "Warning, you are creating a market with a permanent delegate. There is no loss of funds protection for funds on this market"
                    );
                }
            }
        }
    }

    let rent: Rent = Rent::get()?;

    // Create the market PDA itself.
    let (_expected_market_key, market_bump) =
        get_market_address(base_mint.info.key, quote_mint.info.key, market_id);
    let market_seeds_owned: Vec<Vec<u8>> = vec![
        b"market".to_vec(),
        base_mint.info.key.as_ref().to_vec(),
        quote_mint.info.key.as_ref().to_vec(),
        vec![market_id],
        vec![market_bump],
    ];
    create_account(
        payer.as_ref(),
        market.as_ref(),
        system_program.as_ref(),
        &crate::id(),
        &rent,
        size_of::<MarketFixed>() as u64,
        market_seeds_owned,
    )?;

    // Borrowed market seeds for invoke_signed when initializing vaults.
    let market_seeds_for_signing: &[&[u8]] = &[
        b"market",
        base_mint.info.key.as_ref(),
        quote_mint.info.key.as_ref(),
        &[market_id],
        &[market_bump],
    ];

    // Create the base and quote vaults of this market.
    for (token_account, mint) in [
        (base_vault.as_ref(), base_mint.as_ref()),
        (quote_vault.as_ref(), quote_mint.as_ref()),
    ] {
        let is_mint_22: bool = *mint.owner == spl_token_2022::id();
        let token_program_for_mint: Pubkey = if is_mint_22 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };

        let (_vault_key, bump) = get_vault_address(market.info.key, mint.key);
        let seeds: Vec<Vec<u8>> = vec![
            b"vault".to_vec(),
            market.info.key.as_ref().to_vec(),
            mint.key.as_ref().to_vec(),
            vec![bump],
        ];

        if is_mint_22 {
            let mint_data: Ref<'_, &mut [u8]> = mint.data.borrow();
            let mint_with_extension: PodStateWithExtensions<'_, PodMint> =
                PodStateWithExtensions::<PodMint>::unpack(&mint_data).unwrap();
            let mint_extensions: Vec<ExtensionType> = mint_with_extension.get_extension_types()?;
            let required_extensions: Vec<ExtensionType> =
                ExtensionType::get_required_init_account_extensions(&mint_extensions);
            let space: usize =
                ExtensionType::try_calculate_account_len::<Account>(&required_extensions)?;
            create_account(
                payer.as_ref(),
                token_account,
                system_program.as_ref(),
                &token_program_for_mint,
                &rent,
                space as u64,
                seeds,
            )?;
            invoke_signed(
                &spl_token_2022::instruction::initialize_account3(
                    &token_program_for_mint,
                    token_account.key,
                    mint.key,
                    token_account.key,
                )?,
                &[
                    market.info.clone(),
                    token_account.clone(),
                    mint.clone(),
                    token_program_22.as_ref().clone(),
                ],
                &[market_seeds_for_signing],
            )?;
        } else {
            let space: usize = spl_token::state::Account::LEN;
            create_account(
                payer.as_ref(),
                token_account,
                system_program.as_ref(),
                &token_program_for_mint,
                &rent,
                space as u64,
                seeds,
            )?;
            invoke_signed(
                &spl_token::instruction::initialize_account3(
                    &token_program_for_mint,
                    token_account.key,
                    mint.key,
                    token_account.key,
                )?,
                &[
                    market.info.clone(),
                    token_account.clone(),
                    mint.clone(),
                    token_program.as_ref().clone(),
                ],
                &[market_seeds_for_signing],
            )?;
        }
    }

    // Setup the empty market — scoped so the mutable borrow drops before we
    // re-open the account as a ManifestAccountInfo.
    {
        let empty_market_fixed: MarketFixed = MarketFixed::new_empty(
            &base_mint,
            &quote_mint,
            market.info.key,
            market_id,
            authority,
        );
        assert_eq!(market.info.data_len(), size_of::<MarketFixed>());

        let mut market_data = market.info.try_borrow_mut_data()?;
        *get_mut_helper::<MarketFixed>(&mut market_data, 0_u32) = empty_market_fixed;
    }

    emit_stack(CreateMarketLog {
        market: *market.info.key,
        creator: *payer.key,
        base_mint: *base_mint.info.key,
        quote_mint: *quote_mint.info.key,
    })?;

    // Now that the market is initialized as a Manifest account, leave a free
    // block on it so takers can use and leave it.
    let market_account_info: ManifestAccountInfo<MarketFixed> =
        ManifestAccountInfo::<MarketFixed>::new(market.info)?;
    expand_market_if_needed(&payer, &market_account_info)?;

    // Suppress unused warning when invoke is not used downstream.
    let _ = invoke;

    Ok(())
}
