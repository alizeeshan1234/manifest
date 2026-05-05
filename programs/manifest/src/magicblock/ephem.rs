//! Vendored from ephemeral-rollups-sdk v0.2.5 (MIT).
//!
//! Helpers used inside the ephemeral rollup to schedule commits and
//! undelegation back to the base layer.

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke,
};

/// CPI to the magic program to snapshot delegated account state to base
/// layer (account stays delegated).
#[inline(always)]
pub fn commit_accounts<'a, 'info>(
    payer: &'a AccountInfo<'info>,
    account_infos: Vec<&'a AccountInfo<'info>>,
    magic_context: &'a AccountInfo<'info>,
    magic_program: &'a AccountInfo<'info>,
) -> ProgramResult {
    let ix = create_schedule_commit_ix(payer, &account_infos, magic_context, magic_program, false);
    let mut all_accounts = vec![payer.clone(), magic_context.clone()];
    all_accounts.extend(account_infos.into_iter().cloned());
    invoke(&ix, &all_accounts)
}

/// CPI to the magic program to snapshot AND queue undelegation. After this
/// runs on the ER, the base layer can finalize via `undelegate_account`.
#[inline(always)]
pub fn commit_and_undelegate_accounts<'a, 'info>(
    payer: &'a AccountInfo<'info>,
    account_infos: Vec<&'a AccountInfo<'info>>,
    magic_context: &'a AccountInfo<'info>,
    magic_program: &'a AccountInfo<'info>,
) -> ProgramResult {
    let ix = create_schedule_commit_ix(payer, &account_infos, magic_context, magic_program, true);
    let mut all_accounts = vec![payer.clone(), magic_context.clone()];
    all_accounts.extend(account_infos.into_iter().cloned());
    invoke(&ix, &all_accounts)
}

pub fn create_schedule_commit_ix<'a, 'info>(
    payer: &'a AccountInfo<'info>,
    account_infos: &[&'a AccountInfo<'info>],
    magic_context: &'a AccountInfo<'info>,
    magic_program: &'a AccountInfo<'info>,
    allow_undelegation: bool,
) -> Instruction {
    let instruction_data = if allow_undelegation {
        vec![2, 0, 0, 0]
    } else {
        vec![1, 0, 0, 0]
    };
    let mut account_metas = vec![
        AccountMeta {
            pubkey: *payer.key,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: *magic_context.key,
            is_signer: false,
            is_writable: true,
        },
    ];
    account_metas.extend(account_infos.iter().map(|x| AccountMeta {
        pubkey: *x.key,
        is_signer: x.is_signer,
        is_writable: x.is_writable,
    }));
    Instruction::new_with_bytes(*magic_program.key, &instruction_data, account_metas)
}
