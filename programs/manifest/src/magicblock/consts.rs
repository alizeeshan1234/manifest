//! Vendored from ephemeral-rollups-sdk v0.2.5 (MIT).

use solana_program::{pubkey, pubkey::Pubkey};

/// The MagicBlock delegation program ID.
pub const DELEGATION_PROGRAM_ID: Pubkey = pubkey!("DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh");

/// The MagicBlock program ID (used on the ER for commit / undelegate scheduling).
pub const MAGIC_PROGRAM_ID: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");

/// The MagicBlock context account.
pub const MAGIC_CONTEXT_ID: Pubkey = pubkey!("MagicContext1111111111111111111111111111111");

/// Seed for the buffer PDA used during delegation.
pub const BUFFER: &[u8] = b"buffer";

/// Discriminator the delegation program reserves at the start of a delegated
/// account. Not used directly by Manifest.
pub const EXTERNAL_UNDELEGATE_DISCRIMINATOR: [u8; 8] = [196, 28, 41, 206, 48, 37, 51, 167];
