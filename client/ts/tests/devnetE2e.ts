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
  createUndelegateMarketInstruction,
} from '../src/manifest/instructions';
import { createClaimSeatInstruction } from '../src/manifest/instructions/ClaimSeat';
import { OrderType } from '../src/manifest/types/OrderType';
import { getMarketAddress, getVaultAddress } from '../src/utils/market';
import {
  isDelegated,
  makeBaseConnection,
  makeErConnection,
} from '../src/utils/magicblock';
import { PROGRAM_ID } from '../src/manifest';

const DEVNET_RPC = 'https://api.devnet.solana.com';
const ER_RPC = 'https://devnet.magicblock.app';
const ER_WS = 'wss://devnet.magicblock.app';

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

  it('DelegateMarket — base layer (CPIs to delegation program)', async () => {
    expect(await isDelegated(baseConn, market)).to.equal(false);

    const ix = createDelegateMarketInstruction(
      { authority: authority.publicKey, market },
      { minFreeBlocks: 0, validator: null },
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

  it('BatchUpdate place — ER, posts a bid + an ask', async () => {
    const ix = createBatchUpdateInstruction(
      { payer: payer.publicKey, market },
      {
        params: {
          traderIndexHint: null,
          cancels: [],
          orders: [
            {
              baseAtoms: new BN('1000000000'), // 1 base unit
              priceMantissa: 100,
              priceExponent: -2, // price 1.00 quote per base
              isBid: true,
              lastValidSlot: 0,
              orderType: OrderType.Limit,
            },
            {
              baseAtoms: new BN('1000000000'),
              priceMantissa: 105,
              priceExponent: -2, // 1.05
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
    const t0 = Date.now();
    const sig = await erConn.sendRawTransaction(tx.serialize(), {
      skipPreflight: true,
    });
    const sentAt = Date.now() - t0;
    record(`BatchUpdate place (ER, ${sentAt}ms send)`, sig);
  });

  it('BatchUpdate cancel — ER, cancels both orders by sequence number', async () => {
    // Latest market state on the ER. Read from ER endpoint, not base.
    await new Promise((r) => setTimeout(r, 1500)); // allow place to settle
    const acct = await erConn.getAccountInfo(market);
    expect(acct, 'market visible on ER').to.not.be.null;

    // Sequence numbers 0 and 1 (first two orders placed).
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
    const t0 = Date.now();
    const sig = await erConn.sendRawTransaction(tx.serialize(), {
      skipPreflight: true,
    });
    const sentAt = Date.now() - t0;
    record(`BatchUpdate cancel (ER, ${sentAt}ms send)`, sig);
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
});
