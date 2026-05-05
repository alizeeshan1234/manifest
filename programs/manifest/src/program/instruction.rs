use num_enum::TryFromPrimitive;
use shank::ShankInstruction;

/// Instructions available for the Manifest program
#[repr(u8)]
#[derive(TryFromPrimitive, Debug, Copy, Clone, ShankInstruction, PartialEq, Eq)]
#[rustfmt::skip]
pub enum ManifestInstruction {
    /// Create a market
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    #[account(3, name = "base_mint", desc = "Base mint")]
    #[account(4, name = "quote_mint", desc = "Quote mint")]
    #[account(5, writable, name = "base_vault", desc = "Base vault PDA, seeds are [b'vault', market, base_mint]")]
    #[account(6, writable, name = "quote_vault", desc = "Quote vault PDA, seeds are [b'vault', market, quote_mint]")]
    #[account(7, name = "token_program", desc = "Token program")]
    // Always include both token programs so we can initialize both types of token vaults if needed.
    #[account(8, name = "token_program_22", desc = "Token program 22")]
    CreateMarket = 0,

    /// Allocate a seat
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    ClaimSeat = 1,

    /// Deposit
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, writable, name = "trader_token", desc = "Trader token account")]
    #[account(3, writable, name = "vault", desc = "Vault PDA, seeds are [b'vault', market, mint]")]
    #[account(4, name = "token_program", desc = "Token program(22), should be the version that aligns with the token being used")]
    #[account(5, name = "mint", desc = "Required for token22 transfer_checked")]
    Deposit = 2,

    /// Withdraw
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, writable, name = "trader_token", desc = "Trader token account")]
    #[account(3, writable, name = "vault", desc = "Vault PDA, seeds are [b'vault', market, mint]")]
    #[account(4, name = "token_program", desc = "Token program(22), should be the version that aligns with the token being used")]
    #[account(5, name = "mint", desc = "Required for token22 transfer_checked")]
    Withdraw = 3,

    /// Places an order using funds in a wallet instead of on deposit
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    #[account(3, writable, name = "trader_base", desc = "Trader base token account")]
    #[account(4, writable, name = "trader_quote", desc = "Trader quote token account")]
    #[account(5, writable, name = "base_vault", desc = "Base vault PDA, seeds are [b'vault', market_address, base_mint]")]
    #[account(6, writable, name = "quote_vault", desc = "Quote vault PDA, seeds are [b'vault', market_address, quote_mint]")]
    #[account(7, name = "token_program_base", desc = "Token program(22) base")]
    #[account(8, optional, name = "base_mint", desc = "Base mint, only included if base is Token22, otherwise not required")]
    #[account(9, optional, name = "token_program_quote", desc = "Token program(22) quote. Optional. Only include if different from base")]
    #[account(10, optional, name = "quote_mint", desc = "Quote mint, only included if base is Token22, otherwise not required")]
    #[account(11, writable, optional, name = "global", desc = "Global account")]
    #[account(12, writable, optional, name = "global_vault", desc = "Global vault")]
    Swap = 4,

    /// Expand a market.
    /// 
    /// This is not used in normal operations because expansion happens within
    /// instructions that could require it.
    /// This is useful for when rent payer != transaction signer.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    Expand = 5,

    /// Batch update with multiple place orders and cancels.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    #[account(3, optional, name = "base_mint", desc = "Mint for the base global account")]
    #[account(4, optional, writable, name = "base_global", desc = "Base global account")]
    #[account(5, optional, name = "base_global_vault", desc = "Base global vault")]
    #[account(6, optional, name = "base_market_vault", desc = "Base market vault")]
    #[account(7, optional, name = "base_token_program", desc = "Token program(22)")]
    #[account(8, optional, name = "quote_mint", desc = "Mint for this global account")]
    #[account(9, optional, writable, name = "quote_global", desc = "Quote global account")]
    #[account(10, optional, name = "quote_global_vault", desc = "Quote global vault")]
    #[account(11, optional, name = "quote_market_vault", desc = "Quote market vault")]
    #[account(12, optional, name = "quote_token_program", desc = "Token program(22)")]
    BatchUpdate = 6,

    /// Create global account for a given token.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "global", desc = "Global account")]
    #[account(2, name = "system_program", desc = "System program")]
    #[account(3, name = "mint", desc = "Mint for this global account")]
    #[account(4, writable, name = "global_vault", desc = "Global vault")]
    #[account(5, name = "token_program", desc = "Token program(22)")]
    GlobalCreate = 7,

    /// Add a trader to the global account.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "global", desc = "Global account")]
    #[account(2, name = "system_program", desc = "System program")]
    GlobalAddTrader = 8,

    /// Deposit into global account for a given token.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "global", desc = "Global account")]
    #[account(2, name = "mint", desc = "Mint for this global account")]
    #[account(3, writable, name = "global_vault", desc = "Global vault")]
    #[account(4, writable, name = "trader_token", desc = "Trader token account")]
    #[account(5, name = "token_program", desc = "Token program(22)")]
    GlobalDeposit = 9,

    /// Withdraw from global account for a given token.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "global", desc = "Global account")]
    #[account(2, name = "mint", desc = "Mint for this global account")]
    #[account(3, writable, name = "global_vault", desc = "Global vault")]
    #[account(4, writable, name = "trader_token", desc = "Trader token account")]
    #[account(5, name = "token_program", desc = "Token program(22)")]
    GlobalWithdraw = 10,

    /// Evict another trader from the global account.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "global", desc = "Global account")]
    #[account(2, name = "mint", desc = "Mint for this global account")]
    #[account(3, writable, name = "global_vault", desc = "Global vault")]
    #[account(4, name = "trader_token", desc = "Trader token account")]
    #[account(5, name = "evictee_token", desc = "Evictee token account")]
    #[account(6, name = "token_program", desc = "Token program(22)")]
    GlobalEvict = 11,

    /// Removes an order from a market that cannot be filled. There are 3
    /// reasons. It is expired, the global trader got evicted, or the global
    /// trader no longer has deposited the funds to back the order. This
    /// function results in cleaner orderbooks which helps reduce variance and
    /// thus compute unit estimates for traders. It is incentivized by receiving
    /// gas prepayment deposits. This is not required for normal operation of
    /// market. It exists as a deterrent to unfillable and unmaintained global
    /// spam.
    #[account(0, writable, signer, name = "payer", desc = "Payer for this tx, receiver of rent deposit")]
    #[account(1, writable, name = "market", desc = "Account holding all market state")]
    #[account(2, name = "system_program", desc = "System program")]
    #[account(3, writable, name = "global", desc = "Global account")]
    GlobalClean = 12,

    
    /// Places an order using funds in a wallet instead of on deposit. Separates
    /// the owner of the token accounts and the payer. This allows routers to
    /// swap and have intermediate hops go through PDAs, rather than all token
    /// accounts owned by the user.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, signer, name = "owner", desc = "Owner")]
    #[account(2, writable, name = "market", desc = "Account holding all market state")]
    #[account(3, name = "system_program", desc = "System program")]
    #[account(4, writable, name = "trader_base", desc = "Trader base token account")]
    #[account(5, writable, name = "trader_quote", desc = "Trader quote token account")]
    #[account(6, writable, name = "base_vault", desc = "Base vault PDA, seeds are [b'vault', market_address, base_mint]")]
    #[account(7, writable, name = "quote_vault", desc = "Quote vault PDA, seeds are [b'vault', market_address, quote_mint]")]
    #[account(8, name = "token_program_base", desc = "Token program(22) base")]
    #[account(9, optional, name = "base_mint", desc = "Base mint, only included if base is Token22, otherwise not required")]
    #[account(10, optional, name = "token_program_quote", desc = "Token program(22) quote. Optional. Only include if different from base")]
    #[account(11, optional, name = "quote_mint", desc = "Quote mint, only included if base is Token22, otherwise not required")]
    #[account(12, writable, optional, name = "global", desc = "Global account")]
    #[account(13, writable, optional, name = "global_vault", desc = "Global vault")]
    SwapV2 = 13,

    /// Delegate the market to a MagicBlock ephemeral rollup. Base layer only.
    /// Caller must be the market's authority.
    #[account(0, writable, signer, name = "authority", desc = "Authority key for this market (set at CreateMarket)")]
    #[account(1, name = "system_program", desc = "System program")]
    #[account(2, writable, name = "market", desc = "Market PDA to delegate")]
    #[account(3, name = "owner_program", desc = "Manifest program (this program)")]
    #[account(4, writable, name = "delegation_buffer", desc = "Delegation buffer PDA")]
    #[account(5, writable, name = "delegation_record", desc = "Delegation record PDA")]
    #[account(6, writable, name = "delegation_metadata", desc = "Delegation metadata PDA")]
    #[account(7, name = "delegation_program", desc = "MagicBlock delegation program")]
    DelegateMarket = 14,

    /// Snapshot delegated market state back to base layer; market stays delegated.
    /// ER-side only.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Delegated market account")]
    #[account(2, name = "magic_program", desc = "MagicBlock program")]
    #[account(3, writable, name = "magic_context", desc = "MagicBlock context PDA")]
    CommitMarket = 15,

    /// Snapshot final state and queue undelegation. ER-side only.
    /// Pair with UndelegateMarket on base layer to finalize.
    #[account(0, writable, signer, name = "payer", desc = "Payer")]
    #[account(1, writable, name = "market", desc = "Delegated market account")]
    #[account(2, name = "magic_program", desc = "MagicBlock program")]
    #[account(3, writable, name = "magic_context", desc = "MagicBlock context PDA")]
    CommitAndUndelegateMarket = 16,

    /// Finalize undelegation on base layer after CommitAndUndelegateMarket
    /// has run on the ER. Returns market ownership to Manifest.
    #[account(0, writable, name = "market", desc = "Market account being undelegated")]
    #[account(1, name = "delegation_buffer", desc = "Delegation buffer PDA")]
    #[account(2, writable, signer, name = "payer", desc = "Payer / rent receiver")]
    #[account(3, name = "system_program", desc = "System program")]
    UndelegateMarket = 17,

    /// Authorize an ephemeral session keypair to sign BatchUpdate ixs on
    /// behalf of the owner. PDA at [b"session", owner, session_signer].
    #[account(0, writable, signer, name = "owner", desc = "Real trader / payer")]
    #[account(1, writable, name = "session_token", desc = "SessionToken PDA being created")]
    #[account(2, name = "system_program", desc = "System program")]
    CreateSessionToken = 18,

    /// Close a SessionToken and refund the rent to the owner.
    #[account(0, writable, signer, name = "owner", desc = "Real trader (must match session_token.owner)")]
    #[account(1, writable, name = "session_token", desc = "SessionToken PDA being closed")]
    RevokeSessionToken = 19,

    /// Phase 10 deposit Path A — step [1]. Base layer, single user signature.
    /// Tokens land in market_vault directly; receipt is delegated WITH a
    /// post-delegation action that auto-fires ProcessDepositEr (ix 21) on
    /// the ER, which in turn auto-fires CloseDepositReceipt (ix 22) on
    /// base. End-to-end one-shot.
    #[account(0, writable, signer, name = "trader", desc = "Trader / payer")]
    #[account(1, name = "market", desc = "Market account (expected delegated)")]
    #[account(2, writable, name = "market_vault", desc = "Market vault for the deposit's mint")]
    #[account(3, writable, name = "receipt", desc = "DepositReceipt PDA — will be created + delegated")]
    #[account(4, writable, name = "trader_token", desc = "Trader's source token account")]
    #[account(5, name = "mint", desc = "Mint being deposited")]
    #[account(6, name = "system_program", desc = "System program")]
    #[account(7, name = "token_program", desc = "Token program (legacy or 2022)")]
    #[account(8, name = "owner_program", desc = "Manifest program (this program)")]
    #[account(9, writable, name = "delegation_buffer", desc = "Delegation buffer PDA")]
    #[account(10, writable, name = "delegation_record", desc = "Delegation record PDA")]
    #[account(11, writable, name = "delegation_metadata", desc = "Delegation metadata PDA")]
    #[account(12, name = "delegation_program", desc = "MagicBlock delegation program")]
    #[account(13, name = "magic_program", desc = "MagicBlock program (used by post-delegation action)")]
    #[account(14, writable, name = "magic_context", desc = "MagicBlock context (used by post-delegation action)")]
    RequestDeposit = 20,

    /// Phase 10 deposit Path A — step [2]. Auto-fired on ER as a
    /// post-delegation action. Validator-signed. Credits the seat,
    /// schedules the CloseDepositReceipt post-undelegate action.
    #[account(0, writable, signer, name = "trader", desc = "Trader (action signer)")]
    #[account(1, writable, name = "market", desc = "Delegated market account")]
    #[account(2, writable, name = "receipt", desc = "Delegated DepositReceipt PDA")]
    #[account(3, name = "magic_program", desc = "MagicBlock program")]
    #[account(4, writable, name = "magic_context", desc = "MagicBlock context PDA")]
    ProcessDepositEr = 21,

    /// Phase 10 deposit Path A — step [3]. Auto-fired on base as a
    /// post-undelegate action. Validator-signed (NOT trader). Closes the
    /// receipt and refunds rent.
    #[account(0, writable, name = "trader", desc = "Trader (rent recipient; not a signer)")]
    #[account(1, writable, name = "receipt", desc = "DepositReceipt — will be closed")]
    CloseDepositReceipt = 22,
}

impl ManifestInstruction {
    pub fn to_vec(&self) -> Vec<u8> {
        vec![*self as u8]
    }
}

#[test]
fn test_instruction_serialization() {
    let num_instructions: u8 = 22;
    for i in 0..=255 {
        let instruction: ManifestInstruction = match ManifestInstruction::try_from(i) {
            Ok(j) => {
                assert!(i <= num_instructions);
                j
            }
            Err(_) => {
                assert!(i > num_instructions);
                continue;
            }
        };
        assert_eq!(instruction as u8, i);
    }
}

#[test]
fn test_to_vec() {
    let create_market_ix = ManifestInstruction::CreateMarket;
    let vec = create_market_ix.to_vec();
    assert_eq!(*vec.first().unwrap(), 0);
}
