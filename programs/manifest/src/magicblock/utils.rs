//! Vendored from ephemeral-rollups-sdk v0.2.5 (MIT).

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey, rent::Rent,
    sysvar::Sysvar,
};

/// Create a new PDA. Handles both fresh-account and lamports-already-present
/// cases (the latter is rare but happens for prefunded accounts).
#[inline(always)]
pub fn create_pda<'a, 'info>(
    target_account: &'a AccountInfo<'info>,
    owner: &Pubkey,
    space: usize,
    pda_seeds: &[&[&[u8]]],
    system_program: &'a AccountInfo<'info>,
    payer: &'a AccountInfo<'info>,
) -> ProgramResult {
    let rent = Rent::get()?;
    if target_account.lamports().eq(&0) {
        solana_program::program::invoke_signed(
            &solana_program::system_instruction::create_account(
                payer.key,
                target_account.key,
                rent.minimum_balance(space),
                space as u64,
                owner,
            ),
            &[
                payer.clone(),
                target_account.clone(),
                system_program.clone(),
            ],
            pda_seeds,
        )?;
    } else {
        let rent_exempt_balance = rent
            .minimum_balance(space)
            .saturating_sub(target_account.lamports());
        if rent_exempt_balance.gt(&0) {
            solana_program::program::invoke(
                &solana_program::system_instruction::transfer(
                    payer.key,
                    target_account.key,
                    rent_exempt_balance,
                ),
                &[
                    payer.as_ref().clone(),
                    target_account.as_ref().clone(),
                    system_program.as_ref().clone(),
                ],
            )?;
        }

        solana_program::program::invoke_signed(
            &solana_program::system_instruction::allocate(target_account.key, space as u64),
            &[
                target_account.as_ref().clone(),
                system_program.as_ref().clone(),
            ],
            pda_seeds,
        )?;

        solana_program::program::invoke_signed(
            &solana_program::system_instruction::assign(target_account.key, owner),
            &[
                target_account.as_ref().clone(),
                system_program.as_ref().clone(),
            ],
            pda_seeds,
        )?;
    }

    Ok(())
}

/// Close PDA, transferring its lamports to `destination`.
#[inline(always)]
pub fn close_pda<'a, 'info>(
    target_account: &'a AccountInfo<'info>,
    destination: &'a AccountInfo<'info>,
) -> ProgramResult {
    let dest_starting_lamports = destination.lamports();
    **destination.lamports.borrow_mut() = dest_starting_lamports
        .checked_add(target_account.lamports())
        .unwrap();
    **target_account.lamports.borrow_mut() = 0;

    target_account.assign(&solana_program::system_program::ID);
    #[allow(deprecated)]
    target_account.realloc(0, false)
}

/// Close PDA via system_instruction::transfer (used for buffer cleanup).
#[inline(always)]
pub fn close_pda_with_system_transfer<'a, 'info>(
    target_account: &'a AccountInfo<'info>,
    seeds: &[&[&[u8]]],
    destination: &'a AccountInfo<'info>,
    system_program: &'a AccountInfo<'info>,
) -> ProgramResult {
    let transfer_instruction = solana_program::system_instruction::transfer(
        target_account.key,
        destination.key,
        target_account.lamports(),
    );
    #[allow(deprecated)]
    target_account.realloc(0, true)?;
    target_account.assign(&solana_program::system_program::ID);
    solana_program::program::invoke_signed(
        &transfer_instruction,
        &[
            target_account.clone(),
            destination.clone(),
            system_program.clone(),
        ],
        seeds,
    )?;
    Ok(())
}

/// Combine signer seeds with a bump byte.
#[inline(always)]
pub fn seeds_with_bump<'a>(seeds: &'a [&'a [u8]], bump: &'a [u8]) -> Vec<&'a [u8]> {
    let mut combined: Vec<&'a [u8]> = Vec::with_capacity(seeds.len() + 1);
    combined.extend_from_slice(seeds);
    combined.push(bump);
    combined
}
