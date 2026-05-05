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
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
} from '@solana/spl-token';
import { expect } from 'chai';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import BN from 'bn.js';

import {
  createCommitAndUndelegateMarketInstruction,
  createCommitMarketInstruction,
  createCreateMarketInstruction,
  createDelegateMarketInstruction,
  createUndelegateMarketInstruction,
} from '../src/manifest/instructions';
import { createClaimSeatInstruction } from '../src/manifest/instructions/ClaimSeat';
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
