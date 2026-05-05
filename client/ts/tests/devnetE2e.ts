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
});
