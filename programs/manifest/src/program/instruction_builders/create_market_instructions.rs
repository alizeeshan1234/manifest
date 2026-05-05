use crate::{
    program::{processor::create_market::CreateMarketParams, ManifestInstruction},
    validation::{get_market_address, get_vault_address},
};
use borsh::BorshSerialize;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

/// Build the CreateMarket instruction. The market account is a PDA derived
/// from `[b"market", base_mint, quote_mint, market_id]`. The program creates
/// the PDA itself, so unlike previous versions this is a single instruction
/// (no preceding system create_account).
///
/// Pass `Pubkey::default()` as `authority` if the market should not be
/// delegatable to a MagicBlock ER. Otherwise the authority is the only key
/// allowed to call DelegateMarket / UndelegateMarket.
pub fn create_market_instructions(
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    market_creator: &Pubkey,
    market_id: u8,
    authority: &Pubkey,
) -> Vec<Instruction> {
    vec![create_market_instruction(
        base_mint,
        quote_mint,
        market_creator,
        market_id,
        authority,
    )]
}

pub fn create_market_instruction(
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    market_creator: &Pubkey,
    market_id: u8,
    authority: &Pubkey,
) -> Instruction {
    let (market, _market_bump) = get_market_address(base_mint, quote_mint, market_id);
    let (base_vault, _) = get_vault_address(&market, base_mint);
    let (quote_vault, _) = get_vault_address(&market, quote_mint);

    let params = CreateMarketParams::new(market_id, *authority);
    let mut data: Vec<u8> = ManifestInstruction::CreateMarket.to_vec();
    params.serialize(&mut data).unwrap();

    Instruction {
        program_id: crate::id(),
        accounts: vec![
            AccountMeta::new(*market_creator, true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(*base_mint, false),
            AccountMeta::new_readonly(*quote_mint, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
        ],
        data,
    }
}

/// Convenience: also return the derived market PDA for the caller.
pub fn create_market_instruction_with_address(
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    market_creator: &Pubkey,
    market_id: u8,
    authority: &Pubkey,
) -> (Instruction, Pubkey) {
    let (market, _bump) = get_market_address(base_mint, quote_mint, market_id);
    (
        create_market_instruction(base_mint, quote_mint, market_creator, market_id, authority),
        market,
    )
}
