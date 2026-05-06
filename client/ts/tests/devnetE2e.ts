/**
 * Devnet end-to-end test. Hits the live deployed program at
 * 3nqmFMjrw829a88AU3vSnr4BGraKp1pd8jtLnibeCNnw on Solana devnet plus the
 * MagicBlock devnet endpoint for the ER half of the session.
 *
 * Uses the developer's local keypair at ~/.config/solana/id.json as payer
 * and authority (must be funded with devnet SOL).
 *
 * Run with:
 *   yarn test:devnet
 *
 * Captures and prints every tx signature so they can be inspected on the
 * Solana Explorer (devnet) and MagicBlock Explorer.
 */

import {
  Connection,
  Keypair,
  PublicKey,
  sendAndConfirmTransaction,
  SystemProgram,
  Transaction,
} from '@solana/web3.js';
import {
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
} from '@solana/spl-token';
import { expect } from 'chai';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import BN from 'bn.js';

import {
  createBatchUpdateInstruction,
  createCommitAndUndelegateMarketInstruction,
  createCommitMarketInstruction,
  createCreateMarketInstruction,
  createDelegateMarketInstruction,
} from '../src/manifest/instructions';
import { createClaimSeatInstruction } from '../src/manifest/instructions/ClaimSeat';
import { OrderType } from '../src/manifest/types/OrderType';
import { getMarketAddress, getVaultAddress } from '../src/utils/market';
import {
  DELEGATION_PROGRAM_ID,
  getDelegationBuffer,
  getDelegationMetadata,
  getDelegationRecord,
  isDelegated,
  MAGIC_CONTEXT_ID,
  MAGIC_PROGRAM_ID,
  makeBaseConnection,
  makeErConnection,
} from '../src/utils/magicblock';
import { PROGRAM_ID } from '../src/manifest';

const DEVNET_RPC = 'https://api.devnet.solana.com';
// MagicBlock devnet ER endpoint that handles fee bridging from non-delegated
// payers. This is the endpoint magic-trade uses for their devnet tests.
const ER_RPC = process.env.ER_RPC_URL ?? 'https://devnet-as.magicblock.app';
const ER_WS = process.env.ER_WS_URL ?? 'wss://devnet-as.magicblock.app';

function loadLocalKeypair(): Keypair {
  const p = path.join(os.homedir(), '.config', 'solana', 'id.json');
  return Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(fs.readFileSync(p, 'utf8'))),
  );
}

function explorerUrl(sig: string): string {
  return `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
}

describe('Manifest × MagicBlock — devnet E2E', function () {
  this.timeout(600_000);

  const baseConn: Connection = makeBaseConnection(DEVNET_RPC);
  const erConn: Connection = makeErConnection(ER_RPC, ER_WS);
  const payer: Keypair = loadLocalKeypair();
  const authority: Keypair = payer; // use same key for both
  const sigs: { label: string; sig: string }[] = [];

  let baseMint: PublicKey;
  let quoteMint: PublicKey;
  let market: PublicKey;
  const marketId = Math.floor(Math.random() * 250); // unique per run

  function record(label: string, sig: string) {
    sigs.push({ label, sig });
    console.log(`  ✓ ${label}`);
    console.log(`      ${explorerUrl(sig)}`);
  }

  before(async function () {
    console.log(`  payer:     ${payer.publicKey.toBase58()}`);
    console.log(`  programId: ${PROGRAM_ID.toBase58()}`);
    console.log(`  marketId:  ${marketId}`);

    const bal = await baseConn.getBalance(payer.publicKey);
    console.log(`  balance:   ${(bal / 1e9).toFixed(4)} SOL`);
    if (bal < 0.5e9) {
      console.warn('  ⚠ low balance — top up via `solana airdrop 2 --url devnet`');
      this.skip();
    }
  });

  after(() => {
    console.log('\n  ─── Captured tx signatures ─────────────────────────');
    for (const { label, sig } of sigs) {
      console.log(`  ${label.padEnd(28)} ${sig}`);
    }
  });

  it('creates devnet test mints', async () => {
    baseMint = await createMint(
      baseConn,
      payer,
      payer.publicKey,
      payer.publicKey,
      9,
    );
    quoteMint = await createMint(
      baseConn,
      payer,
      payer.publicKey,
      payer.publicKey,
      6,
    );
    console.log(`  baseMint:  ${baseMint.toBase58()}`);
    console.log(`  quoteMint: ${quoteMint.toBase58()}`);
  });

  it('CreateMarket — base layer, sets market_id and authority', async () => {
    market = getMarketAddress(baseMint, quoteMint, marketId);
    console.log(`  market PDA: ${market.toBase58()}`);

    const ix = createCreateMarketInstruction(
      {
        payer: payer.publicKey,
        market,
        baseMint,
        quoteMint,
        baseVault: getVaultAddress(market, baseMint),
        quoteVault: getVaultAddress(market, quoteMint),
        tokenProgram22: TOKEN_2022_PROGRAM_ID,
        tokenProgram: TOKEN_PROGRAM_ID,
      },
      { marketId, authority: authority.publicKey },
    );

    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [payer], {
      commitment: 'confirmed',
    });
    record('CreateMarket', sig);

    const acct = await baseConn.getAccountInfo(market);
    expect(acct, 'market should exist after create').to.not.be.null;
    expect(acct!.owner.toBase58()).to.equal(PROGRAM_ID.toBase58());
    expect(acct!.data[13]).to.equal(marketId);
    expect(new PublicKey(acct!.data.subarray(192, 224)).toBase58()).to.equal(
      authority.publicKey.toBase58(),
    );
  });

  it('ClaimSeat — base layer', async () => {
    const ix = createClaimSeatInstruction({
      payer: payer.publicKey,
      market,
    });
    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [payer], {
      commitment: 'confirmed',
    });
    record('ClaimSeat', sig);
  });

  let traderQuoteAta: PublicKey;
  let traderBaseAta: PublicKey;

  it('Mint test tokens + Deposit on base layer (must happen before delegate)', async () => {
    // Pre-flight: trader needs an ATA + a mint for each side, and the
    // market vaults need to be funded so seat balances are credited.
    const baseAcct = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      baseMint,
      payer.publicKey,
    );
    const quoteAcct = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      quoteMint,
      payer.publicKey,
    );
    traderBaseAta = baseAcct.address;
    traderQuoteAta = quoteAcct.address;

    await mintTo(
      baseConn,
      payer,
      baseMint,
      traderBaseAta,
      payer,
      1_000_000_000_000n,
    );
    await mintTo(
      baseConn,
      payer,
      quoteMint,
      traderQuoteAta,
      payer,
      1_000_000_000_000n,
    );

    // Build ix data manually because the solita-generated DepositStruct has
    // a stale duplicate `traderIndexHint` field. Rust expects only:
    //   [discriminator u8][amount_atoms u64 LE][trader_index_hint COption<u32>]
    function depositIxData(amount: bigint): Buffer {
      const buf = Buffer.alloc(1 + 8 + 1);
      buf.writeUInt8(2, 0); // Deposit discriminator
      buf.writeBigUInt64LE(amount, 1);
      buf.writeUInt8(0, 9); // None for trader_index_hint
      return buf;
    }

    const buildDepositIx = (
      mint: PublicKey,
      vault: PublicKey,
      traderToken: PublicKey,
      amount: bigint,
    ) =>
      new (require('@solana/web3.js').TransactionInstruction)({
        programId: PROGRAM_ID,
        keys: [
          { pubkey: payer.publicKey, isSigner: true, isWritable: true },
          { pubkey: market, isSigner: false, isWritable: true },
          { pubkey: traderToken, isSigner: false, isWritable: true },
          { pubkey: vault, isSigner: false, isWritable: true },
          { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
          { pubkey: mint, isSigner: false, isWritable: false },
        ],
        data: depositIxData(amount),
      });

    const depositBaseIx = buildDepositIx(
      baseMint,
      getVaultAddress(market, baseMint),
      traderBaseAta,
      100_000_000_000n,
    );
    const depositQuoteIx = buildDepositIx(
      quoteMint,
      getVaultAddress(market, quoteMint),
      traderQuoteAta,
      100_000_000_000n,
    );

    const sigBase = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(depositBaseIx),
      [payer],
      { commitment: 'confirmed' },
    );
    record('Deposit (base side)', sigBase);

    const sigQuote = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(depositQuoteIx),
      [payer],
      { commitment: 'confirmed' },
    );
    record('Deposit (quote side)', sigQuote);
  });

  it('Pre-expand market to reserve free blocks (must happen before delegate)', async () => {
    // The ER cannot realloc the market (disable-realloc + Feepayer-not-
    // delegated checks). Every place_order consumes a free block from the
    // market arena. Pre-expand on base so the ER never needs to grow.
    const num_free_blocks = 50;
    const data = Buffer.alloc(1 + 4);
    data.writeUInt8(5, 0); // Expand discriminator
    data.writeUInt32LE(num_free_blocks, 1);
    const expandIx = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      data,
    });
    const sig = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(expandIx),
      [payer],
      { commitment: 'confirmed' },
    );
    record(`Expand market by ${num_free_blocks} blocks`, sig);
  });

  it('DelegateMarket — base layer (CPIs to delegation program)', async () => {
    expect(await isDelegated(baseConn, market)).to.equal(false);

    const ix = createDelegateMarketInstruction(
      { authority: authority.publicKey, market },
      { minFreeBlocks: 50, validator: null },
    );
    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [authority], {
      commitment: 'confirmed',
      skipPreflight: true,
    });
    record('DelegateMarket', sig);

    // Wait a moment for delegation to settle, then verify owner change.
    await new Promise((r) => setTimeout(r, 3000));
    const owned = await isDelegated(baseConn, market);
    console.log(`  market.owner now delegation-program? ${owned}`);
  });

  async function sendErAndConfirm(label: string, tx: Transaction) {
    const t0 = Date.now();
    const sig = await erConn.sendRawTransaction(tx.serialize(), {
      skipPreflight: true,
    });
    const sentAt = Date.now() - t0;
    // Poll for tx result on the ER.
    let parsed: any = null;
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 500));
      try {
        parsed = await erConn.getTransaction(sig, {
          commitment: 'confirmed',
          maxSupportedTransactionVersion: 0,
        });
        if (parsed) break;
      } catch {
        /* retry */
      }
    }
    record(`${label} (ER, ${sentAt}ms send)`, sig);
    if (parsed?.meta?.err) {
      const errStr = JSON.stringify(parsed.meta.err);
      const logs: string[] = parsed.meta.logMessages ?? [];
      console.log(`  ✗ ${label} on-chain error:`, errStr);
      console.log(`  logs:`, logs.slice(0, 30).join('\n         '));
      throw new Error(`${label} reverted: ${errStr}`);
    }
    return sig;
  }

  it('BatchUpdate place — ER, posts a bid + an ask', async () => {
    const ix = createBatchUpdateInstruction(
      { payer: payer.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [],
          orders: [
            {
              baseAtoms: new BN('1000000000'),
              priceMantissa: 100,
              priceExponent: -2,
              isBid: true,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
            {
              baseAtoms: new BN('1000000000'),
              priceMantissa: 105,
              priceExponent: -2,
              isBid: false,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
          ],
        },
      },
    );

    const tx = new Transaction().add(ix);
    tx.feePayer = payer.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(payer);
    await sendErAndConfirm('BatchUpdate place', tx);
  });

  it('BatchUpdate cancel — ER, cancels both orders', async () => {
    await new Promise((r) => setTimeout(r, 1500));

    const ix = createBatchUpdateInstruction(
      { payer: payer.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [
            { orderSequenceNumber: new BN(0), orderIndexHint: null },
            { orderSequenceNumber: new BN(1), orderIndexHint: null },
          ],
          orders: [],
        },
      },
    );

    const tx = new Transaction().add(ix);
    tx.feePayer = payer.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(payer);
    await sendErAndConfirm('BatchUpdate cancel', tx);
  });

  it('CommitAndUndelegateMarket — ER side', async () => {
    const ix = createCommitAndUndelegateMarketInstruction({
      payer: payer.publicKey,
      market,
    });
    const tx = new Transaction().add(ix);
    tx.feePayer = payer.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(payer);
    const sig = await erConn.sendRawTransaction(tx.serialize(), {
      skipPreflight: true,
    });
    record('CommitAndUndelegate', sig);
  });

  it('Auto-undelegation: market returns to Manifest ownership', async () => {
    // The MagicBlock delegation program calls back into our program with
    // the EXTERNAL_UNDELEGATE_DISCRIMINATOR after CommitAndUndelegate is
    // processed on the ER. No manual ix needed from the user side.
    let owner: PublicKey | null = null;
    for (let i = 0; i < 60; i++) {
      const acct = await baseConn.getAccountInfo(market);
      if (acct) {
        owner = acct.owner;
        if (owner.equals(PROGRAM_ID)) break;
      }
      await new Promise((r) => setTimeout(r, 2000));
    }
    console.log(`  market.owner after wait: ${owner?.toBase58()}`);
    expect(
      owner?.toBase58(),
      'market should be back under Manifest ownership',
    ).to.equal(PROGRAM_ID.toBase58());
  });

  // ── Phase 10: deposit Path A ─────────────────────────────────────────
  // After the previous suite undelegated the market, re-delegate it and
  // then run a single-tx deposit-while-delegated. The ix chain is:
  //   user -> RequestDeposit (base)
  //              ├─ SPL transfer wallet → market_vault
  //              ├─ create DepositReceipt PDA
  //              └─ delegate_account_with_actions(receipt, [ProcessDepositEr])
  //   validator -> ProcessDepositEr (ER, post-delegation action)
  //              ├─ credits seat
  //              └─ commit_and_undelegate(receipt) + post-undelegate action
  //   validator -> CloseDepositReceipt (base, post-undelegate action)
  //              └─ closes receipt, refunds rent

  function getDepositReceiptAddress(
    market_: PublicKey,
    trader: PublicKey,
    mint: PublicKey,
  ): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from('deposit_receipt'),
        market_.toBuffer(),
        trader.toBuffer(),
        mint.toBuffer(),
      ],
      PROGRAM_ID,
    )[0];
  }

  it('Re-DelegateMarket for deposit Path A', async () => {
    const delegated = await isDelegated(baseConn, market);
    expect(delegated, 'market should be undelegated before re-delegate').to
      .equal(false);

    const ix = createDelegateMarketInstruction(
      { authority: authority.publicKey, market },
      { minFreeBlocks: 50, validator: null },
    );
    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [authority], {
      commitment: 'confirmed',
      skipPreflight: true,
    });
    record('Re-DelegateMarket', sig);
    await new Promise((r) => setTimeout(r, 3000));
    expect(await isDelegated(baseConn, market), 'market should be delegated')
      .to.equal(true);
  });

  it('RequestDeposit — single user signature, full flow auto-fires', async () => {
    const depositAmount = 25_000_000n; // 25 USDC (quote, 6 decimals)

    // Snapshot vault balance and trader_token balance before.
    const quoteVault = getVaultAddress(market, quoteMint);
    const vaultAccBefore = await baseConn.getTokenAccountBalance(quoteVault);
    const traderAccBefore = await baseConn.getTokenAccountBalance(traderQuoteAta);
    const vaultBalBefore = BigInt(vaultAccBefore.value.amount);
    const traderBalBefore = BigInt(traderAccBefore.value.amount);

    const receiptPda = getDepositReceiptAddress(
      market,
      payer.publicKey,
      quoteMint,
    );

    const data = Buffer.alloc(1 + 8);
    data.writeUInt8(20, 0); // RequestDeposit discriminator
    data.writeBigUInt64LE(depositAmount, 1);

    const ix = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: false },
        { pubkey: quoteVault, isSigner: false, isWritable: true },
        { pubkey: receiptPda, isSigner: false, isWritable: true },
        { pubkey: traderQuoteAta, isSigner: false, isWritable: true },
        { pubkey: quoteMint, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
        {
          pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationRecord(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationMetadata(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
      ],
      data,
    });

    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [payer], {
      commitment: 'confirmed',
      skipPreflight: true,
    });
    record('RequestDeposit', sig);

    // Vault balance should have increased by exactly depositAmount.
    const vaultAccAfterReq = await baseConn.getTokenAccountBalance(quoteVault);
    expect(
      BigInt(vaultAccAfterReq.value.amount) - vaultBalBefore,
      'vault should have +depositAmount immediately',
    ).to.equal(depositAmount);
    expect(
      traderBalBefore - BigInt((await baseConn.getTokenAccountBalance(traderQuoteAta)).value.amount),
      'trader_token should have -depositAmount immediately',
    ).to.equal(depositAmount);

    // Wait for the post-delegation action chain to complete and the
    // receipt to close (i.e., account no longer exists).
    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    const deadline = 30_000;
    while (receiptInfo !== null && elapsed < deadline) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }

    expect(
      receiptInfo,
      'receipt should be closed by CloseDepositReceipt callback',
    ).to.equal(null);

    console.log(
      `  vault delta: +${depositAmount}; receipt closed in ~${elapsed / 1000}s`,
    );
  });

  // ── Phase 9: withdrawal Path A ───────────────────────────────────────
  // Same single-tx pattern as deposit, but in reverse.
  //   user -> RequestWithdrawal (base): create receipt + delegate-with-actions
  //   validator -> ProcessWithdrawalEr (ER): debit seat, schedule
  //                ExecuteWithdrawalBaseChain post-undelegate action
  //   validator -> ExecuteWithdrawalBaseChain (base): SPL transfer
  //                market_vault -> trader_token, close receipt

  function getWithdrawalReceiptAddress(
    market_: PublicKey,
    trader: PublicKey,
    mint: PublicKey,
  ): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from('withdraw_receipt'),
        market_.toBuffer(),
        trader.toBuffer(),
        mint.toBuffer(),
      ],
      PROGRAM_ID,
    )[0];
  }

  it('RequestWithdrawal — single user signature, full flow auto-fires', async () => {
    // Market should still be delegated from the previous test.
    expect(await isDelegated(baseConn, market), 'market must be delegated')
      .to.equal(true);

    const withdrawAmount = 10_000_000n; // 10 USDC

    const quoteVault = getVaultAddress(market, quoteMint);
    const vaultAccBefore = await baseConn.getTokenAccountBalance(quoteVault);
    const traderAccBefore = await baseConn.getTokenAccountBalance(traderQuoteAta);
    const vaultBalBefore = BigInt(vaultAccBefore.value.amount);
    const traderBalBefore = BigInt(traderAccBefore.value.amount);

    const receiptPda = getWithdrawalReceiptAddress(
      market,
      payer.publicKey,
      quoteMint,
    );

    const data = Buffer.alloc(1 + 8);
    data.writeUInt8(23, 0); // RequestWithdrawal discriminator
    data.writeBigUInt64LE(withdrawAmount, 1);

    const ix = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: false },
        { pubkey: receiptPda, isSigner: false, isWritable: true },
        { pubkey: quoteMint, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
        {
          pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationRecord(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationMetadata(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
        { pubkey: quoteVault, isSigner: false, isWritable: true },
        { pubkey: traderQuoteAta, isSigner: false, isWritable: true },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      ],
      data,
    });

    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(baseConn, tx, [payer], {
      commitment: 'confirmed',
      skipPreflight: true,
    });
    record('RequestWithdrawal', sig);

    // Wait for the post-delegation action chain to land back on base.
    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    const deadline = 30_000;
    while (receiptInfo !== null && elapsed < deadline) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }

    expect(
      receiptInfo,
      'receipt should be closed by ExecuteWithdrawalBaseChain callback',
    ).to.equal(null);

    const vaultAccAfter = await baseConn.getTokenAccountBalance(quoteVault);
    const traderAccAfter = await baseConn.getTokenAccountBalance(traderQuoteAta);
    const vaultDelta =
      BigInt(vaultAccAfter.value.amount) - vaultBalBefore;
    const traderDelta =
      BigInt(traderAccAfter.value.amount) - traderBalBefore;

    console.log(
      `  vault delta: ${vaultDelta}; trader delta: +${traderDelta}; receipt closed in ~${elapsed / 1000}s`,
    );

    // ER may have clamped the withdrawal to seat balance; we expect the
    // negative-of-vault to equal the trader delta and to be positive.
    expect(vaultDelta).to.equal(-traderDelta);
    expect(traderDelta > 0n, 'trader should have received tokens').to.equal(true);
  });

  // ── Phase A: 2-trader taker-fill scenario on the ER ──────────────────
  // Onboard trader2 *while the market stays delegated* — never undelegate
  // for setup, since in production a market should remain delegated 24/7.
  //
  // Flow:
  //   1. Fund trader2 with SOL + mint test tokens (base, doesn't touch market)
  //   2. Trader2 ClaimSeat on the ER (delegated market accepts ClaimSeat;
  //      expand_market_if_needed is a no-op when delegated)
  //   3. Trader2 RequestDeposit Path A on base (1 sig, deposit while delegated)
  //   4. Trader1 posts a resting bid on the ER
  //   5. Trader2 fires a crossing ask on the ER → fill happens inside BatchUpdate
  //   6. CommitAndUndelegate + Withdraws on base prove the fill persisted.

  const trader2 = Keypair.generate();
  let trader2Base: PublicKey;
  let trader2Quote: PublicKey;
  const FILL_BASE_ATOMS = 1_000_000_000n; // 1B base atoms
  const FILL_QUOTE_ATOMS = 1_000_000_000n; // price = 1.0 quote per base
  const TRADER2_DEPOSIT = 50_000_000_000n; // 50B atoms each side

  it('Setup trader2: fund SOL + mint test tokens (no market mutation)', async () => {
    expect(
      await isDelegated(baseConn, market),
      'market must stay delegated for trader2 onboarding',
    ).to.equal(true);

    const fundTx = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: payer.publicKey,
        toPubkey: trader2.publicKey,
        lamports: 500_000_000, // 0.5 SOL on base for fee bridging + ATA owners
      }),
    );
    await sendAndConfirmTransaction(baseConn, fundTx, [payer], {
      commitment: 'confirmed',
    });

    const baseAcct2 = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      baseMint,
      trader2.publicKey,
    );
    const quoteAcct2 = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      quoteMint,
      trader2.publicKey,
    );
    trader2Base = baseAcct2.address;
    trader2Quote = quoteAcct2.address;

    await mintTo(
      baseConn,
      payer,
      baseMint,
      trader2Base,
      payer,
      100_000_000_000n,
    );
    await mintTo(
      baseConn,
      payer,
      quoteMint,
      trader2Quote,
      payer,
      100_000_000_000n,
    );
  });

  it('Trader2 ClaimSeat on the ER (delegated market)', async () => {
    const ix = createClaimSeatInstruction({
      payer: trader2.publicKey,
      market,
    });
    const tx = new Transaction().add(ix);
    tx.feePayer = trader2.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(trader2);
    await sendErAndConfirm('Trader2 ClaimSeat (ER)', tx);
  });

  it('Trader2 RequestDeposit (base side) — Path A while delegated', async () => {
    const baseVault = getVaultAddress(market, baseMint);
    const receiptPda = getDepositReceiptAddress(
      market,
      trader2.publicKey,
      baseMint,
    );
    const data = Buffer.alloc(1 + 8);
    data.writeUInt8(20, 0);
    data.writeBigUInt64LE(TRADER2_DEPOSIT, 1);

    const ix = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: trader2.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: false },
        { pubkey: baseVault, isSigner: false, isWritable: true },
        { pubkey: receiptPda, isSigner: false, isWritable: true },
        { pubkey: trader2Base, isSigner: false, isWritable: true },
        { pubkey: baseMint, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
        {
          pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationRecord(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationMetadata(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
      ],
      data,
    });
    const sig = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(ix),
      [trader2],
      { commitment: 'confirmed', skipPreflight: true },
    );
    record('Trader2 RequestDeposit (base)', sig);

    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    while (receiptInfo !== null && elapsed < 30_000) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }
    expect(receiptInfo, 'base receipt should be closed by callback').to.equal(
      null,
    );
  });

  it('Trader2 RequestDeposit (quote side) — Path A while delegated', async () => {
    const quoteVault = getVaultAddress(market, quoteMint);
    const receiptPda = getDepositReceiptAddress(
      market,
      trader2.publicKey,
      quoteMint,
    );
    const data = Buffer.alloc(1 + 8);
    data.writeUInt8(20, 0);
    data.writeBigUInt64LE(TRADER2_DEPOSIT, 1);

    const ix = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: trader2.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: false },
        { pubkey: quoteVault, isSigner: false, isWritable: true },
        { pubkey: receiptPda, isSigner: false, isWritable: true },
        { pubkey: trader2Quote, isSigner: false, isWritable: true },
        { pubkey: quoteMint, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
        {
          pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationRecord(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationMetadata(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
      ],
      data,
    });
    const sig = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(ix),
      [trader2],
      { commitment: 'confirmed', skipPreflight: true },
    );
    record('Trader2 RequestDeposit (quote)', sig);

    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    while (receiptInfo !== null && elapsed < 30_000) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }
    expect(receiptInfo, 'quote receipt should be closed by callback').to.equal(
      null,
    );
  });

  it('Trader1 posts resting bid on ER', async () => {
    const ix = createBatchUpdateInstruction(
      { payer: payer.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [],
          orders: [
            {
              baseAtoms: new BN(FILL_BASE_ATOMS.toString()),
              priceMantissa: 100,
              priceExponent: -2, // price = 1.0 quote per base
              isBid: true,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
          ],
        },
      },
    );
    const tx = new Transaction().add(ix);
    tx.feePayer = payer.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(payer);
    await sendErAndConfirm('Trader1 resting bid', tx);
  });

  it('Trader2 places crossing ask on ER (taker fill)', async () => {
    const ix = createBatchUpdateInstruction(
      { payer: trader2.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [],
          orders: [
            {
              baseAtoms: new BN(FILL_BASE_ATOMS.toString()),
              priceMantissa: 100,
              priceExponent: -2, // price = 1.0 — crosses trader1's bid
              isBid: false,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
          ],
        },
      },
    );
    const tx = new Transaction().add(ix);
    tx.feePayer = trader2.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(trader2);
    await sendErAndConfirm('Trader2 crossing ask (taker)', tx);
  });

  // Single helper used by both fill-verification withdraws below.
  function buildRequestWithdrawalIx(
    signerKey: PublicKey,
    mint: PublicKey,
    vault: PublicKey,
    traderToken: PublicKey,
    amount: bigint,
  ) {
    const receiptPda = getWithdrawalReceiptAddress(market, signerKey, mint);
    const data = Buffer.alloc(1 + 8);
    data.writeUInt8(23, 0);
    data.writeBigUInt64LE(amount, 1);
    return {
      receiptPda,
      ix: new (require('@solana/web3.js').TransactionInstruction)({
        programId: PROGRAM_ID,
        keys: [
          { pubkey: signerKey, isSigner: true, isWritable: true },
          { pubkey: market, isSigner: false, isWritable: false },
          { pubkey: receiptPda, isSigner: false, isWritable: true },
          { pubkey: mint, isSigner: false, isWritable: false },
          { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
          { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
          {
            pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
            isSigner: false,
            isWritable: true,
          },
          {
            pubkey: getDelegationRecord(receiptPda),
            isSigner: false,
            isWritable: true,
          },
          {
            pubkey: getDelegationMetadata(receiptPda),
            isSigner: false,
            isWritable: true,
          },
          { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
          { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
          { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
          { pubkey: vault, isSigner: false, isWritable: true },
          { pubkey: traderToken, isSigner: false, isWritable: true },
          { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        ],
        data,
      }),
    };
  }

  async function awaitReceiptClosed(receiptPda: PublicKey) {
    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    while (receiptInfo !== null && elapsed < 30_000) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }
    expect(receiptInfo, 'withdrawal receipt should be closed').to.equal(null);
  }

  it('Trader1 RequestWithdrawal Path A — pulls fill base while delegated', async () => {
    expect(
      await isDelegated(baseConn, market),
      'market must still be delegated for verification',
    ).to.equal(true);

    const t1BaseBefore = BigInt(
      (await baseConn.getTokenAccountBalance(traderBaseAta)).value.amount,
    );
    const { receiptPda, ix } = buildRequestWithdrawalIx(
      payer.publicKey,
      baseMint,
      getVaultAddress(market, baseMint),
      traderBaseAta,
      FILL_BASE_ATOMS,
    );
    const sig = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(ix),
      [payer],
      { commitment: 'confirmed', skipPreflight: true },
    );
    record('Trader1 RequestWithdrawal (gained base)', sig);
    await awaitReceiptClosed(receiptPda);

    const t1BaseAfter = BigInt(
      (await baseConn.getTokenAccountBalance(traderBaseAta)).value.amount,
    );
    expect(
      t1BaseAfter - t1BaseBefore,
      'trader1 should have received fill base',
    ).to.equal(FILL_BASE_ATOMS);
  });

  it('Trader2 RequestWithdrawal Path A — pulls fill quote while delegated', async () => {
    expect(
      await isDelegated(baseConn, market),
      'market must still be delegated for verification',
    ).to.equal(true);

    const t2QuoteBefore = BigInt(
      (await baseConn.getTokenAccountBalance(trader2Quote)).value.amount,
    );
    const { receiptPda, ix } = buildRequestWithdrawalIx(
      trader2.publicKey,
      quoteMint,
      getVaultAddress(market, quoteMint),
      trader2Quote,
      FILL_QUOTE_ATOMS,
    );
    const sig = await sendAndConfirmTransaction(
      baseConn,
      new Transaction().add(ix),
      [trader2],
      { commitment: 'confirmed', skipPreflight: true },
    );
    record('Trader2 RequestWithdrawal (gained quote)', sig);
    await awaitReceiptClosed(receiptPda);

    const t2QuoteAfter = BigInt(
      (await baseConn.getTokenAccountBalance(trader2Quote)).value.amount,
    );
    expect(
      t2QuoteAfter - t2QuoteBefore,
      'trader2 should have received fill quote',
    ).to.equal(FILL_QUOTE_ATOMS);

    console.log(
      `  fill verified (market still delegated): trader1 +${FILL_BASE_ATOMS} base, trader2 +${FILL_QUOTE_ATOMS} quote`,
    );
  });

  // ── Phase B: 1-sig swap-from-wallet (Path A) ────────────────────────
  // Brand-new trader3 has NO seat, NO deposit. They sign one base-layer
  // tx (RequestSwap) and the validator chain handles seat-claim + match
  // + payout while the market stays delegated. Final state: trader3's
  // wallet has the output mint, no seat left behind.

  const trader3 = Keypair.generate();
  let trader3Base: PublicKey;
  let trader3Quote: PublicKey;
  const SWAP_INPUT_QUOTE = 1_000_000_000n; // 1B quote atoms in
  const SWAP_MIN_OUT_BASE = 900_000_000n; // expect ~1B base out at price 1.0

  function getSwapReceiptAddress(
    market_: PublicKey,
    trader: PublicKey,
    inputMint: PublicKey,
  ): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from('swap_receipt'),
        market_.toBuffer(),
        trader.toBuffer(),
        inputMint.toBuffer(),
      ],
      PROGRAM_ID,
    )[0];
  }

  it('Setup trader3: fund SOL + mint quote tokens (no seat, no deposit)', async () => {
    expect(
      await isDelegated(baseConn, market),
      'market must stay delegated for swap',
    ).to.equal(true);

    const fundTx = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: payer.publicKey,
        toPubkey: trader3.publicKey,
        lamports: 500_000_000,
      }),
    );
    await sendAndConfirmTransaction(baseConn, fundTx, [payer], {
      commitment: 'confirmed',
    });

    const baseAcct3 = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      baseMint,
      trader3.publicKey,
    );
    const quoteAcct3 = await getOrCreateAssociatedTokenAccount(
      baseConn,
      payer,
      quoteMint,
      trader3.publicKey,
    );
    trader3Base = baseAcct3.address;
    trader3Quote = quoteAcct3.address;

    // Only mint quote — trader3 will swap quote→base.
    await mintTo(
      baseConn,
      payer,
      quoteMint,
      trader3Quote,
      payer,
      10_000_000_000n,
    );
  });

  it('Trader1 reposts a resting ask on ER for trader3 to take', async () => {
    // After the fill test, trader1 has plenty of base on-seat. Post a
    // fresh ask for trader3's swap to consume.
    const ix = createBatchUpdateInstruction(
      { payer: payer.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [],
          orders: [
            {
              baseAtoms: new BN('1000000000'),
              priceMantissa: 100,
              priceExponent: -2, // price = 1.0 quote per base
              isBid: false,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
          ],
        },
      },
    );
    const tx = new Transaction().add(ix);
    tx.feePayer = payer.publicKey;
    const { blockhash } = await erConn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.sign(payer);
    await sendErAndConfirm('Trader1 resting ask (for swap)', tx);
  });

  it('Trader3 RequestSwap — 1 sig, full chain auto-fires', async () => {
    const baseVault = getVaultAddress(market, baseMint);
    const quoteVault = getVaultAddress(market, quoteMint);
    const receiptPda = getSwapReceiptAddress(
      market,
      trader3.publicKey,
      quoteMint,
    );

    const t3BaseBefore = BigInt(
      (await baseConn.getTokenAccountBalance(trader3Base)).value.amount,
    );
    const t3QuoteBefore = BigInt(
      (await baseConn.getTokenAccountBalance(trader3Quote)).value.amount,
    );

    const data = Buffer.alloc(1 + 8 + 8);
    data.writeUInt8(26, 0); // RequestSwap discriminator
    data.writeBigUInt64LE(SWAP_INPUT_QUOTE, 1);
    data.writeBigUInt64LE(SWAP_MIN_OUT_BASE, 9);

    const ix = new (require('@solana/web3.js').TransactionInstruction)({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: trader3.publicKey, isSigner: true, isWritable: true },
        { pubkey: market, isSigner: false, isWritable: false },
        { pubkey: quoteVault, isSigner: false, isWritable: true }, // input_vault
        { pubkey: baseVault, isSigner: false, isWritable: true }, // output_vault
        { pubkey: receiptPda, isSigner: false, isWritable: true },
        { pubkey: trader3Quote, isSigner: false, isWritable: true }, // trader_token_in
        { pubkey: trader3Base, isSigner: false, isWritable: true }, // trader_token_out
        { pubkey: quoteMint, isSigner: false, isWritable: false }, // input_mint
        { pubkey: baseMint, isSigner: false, isWritable: false }, // output_mint
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
        {
          pubkey: getDelegationBuffer(receiptPda, PROGRAM_ID),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationRecord(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        {
          pubkey: getDelegationMetadata(receiptPda),
          isSigner: false,
          isWritable: true,
        },
        { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
      ],
      data,
    });

    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(
      baseConn,
      tx,
      [trader3],
      { commitment: 'confirmed', skipPreflight: true },
    );
    record('Trader3 RequestSwap', sig);

    // Wait for the post-delegation/post-undelegate chain to close the receipt.
    let receiptInfo = await baseConn.getAccountInfo(receiptPda);
    let elapsed = 0;
    while (receiptInfo !== null && elapsed < 30_000) {
      await new Promise((r) => setTimeout(r, 2000));
      elapsed += 2000;
      receiptInfo = await baseConn.getAccountInfo(receiptPda);
    }
    expect(receiptInfo, 'swap receipt should be closed').to.equal(null);

    const t3BaseAfter = BigInt(
      (await baseConn.getTokenAccountBalance(trader3Base)).value.amount,
    );
    const t3QuoteAfter = BigInt(
      (await baseConn.getTokenAccountBalance(trader3Quote)).value.amount,
    );
    const baseGained = t3BaseAfter - t3BaseBefore;
    const quoteSpent = t3QuoteBefore - t3QuoteAfter;
    console.log(
      `  swap result: trader3 gained ${baseGained} base, spent ${quoteSpent} quote in ~${elapsed / 1000}s`,
    );

    // At price 1.0 with a 1B base resting ask, swapping 1B quote should
    // fill exactly: 1B base out, 1B quote in.
    expect(baseGained > 0n, 'trader3 should have received base').to.equal(true);
    expect(baseGained >= SWAP_MIN_OUT_BASE, 'output should meet min_out').to
      .equal(true);
    // Net quote spent <= input (refund kicks in for any unconsumed input)
    expect(quoteSpent <= SWAP_INPUT_QUOTE, 'quote spent should not exceed input')
      .to.equal(true);
  });
});
