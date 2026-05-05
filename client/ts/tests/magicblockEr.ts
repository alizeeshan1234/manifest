/**
 * Mocha tests for the MagicBlock ER instruction builders and the end-to-end
 * session flow. Three tiers:
 *
 *  1. UNIT — no network. Verifies builder output (account layout, ix data).
 *  2. LOCALNET — needs `solana-test-validator` on 127.0.0.1:8899 with the
 *     manifest program deployed. Tests CreateMarket with the new
 *     market_id+authority params, plus the MarketIsDelegated gate (run by
 *     forcibly setting market.owner via clone_account in test fixtures —
 *     not available here, so the gate test is skipped).
 *  3. ER E2E — gated on env MANIFEST_ER_E2E=1 and ER_RPC_URL set. Runs
 *     a full session flow against a live MagicBlock devnet validator.
 *
 * Run all tiers:    yarn test client/ts/tests/magicblockEr.ts
 * Skip localnet:    NO_LOCAL_VALIDATOR=1 yarn test ...
 * Run ER e2e too:   MANIFEST_ER_E2E=1 ER_RPC_URL=https://devnet.magicblock.app yarn test ...
 */

import { assert, expect } from 'chai';
import {
  Connection,
  Keypair,
  PublicKey,
  sendAndConfirmTransaction,
  SystemProgram,
  Transaction,
} from '@solana/web3.js';
import { createMint } from '@solana/spl-token';

import {
  createCommitAndUndelegateMarketInstruction,
  createCommitMarketInstruction,
  createCreateMarketInstruction,
  createDelegateMarketInstruction,
  createUndelegateMarketInstruction,
} from '../src/manifest/instructions';
import { getMarketAddress, getVaultAddress } from '../src/utils/market';
import {
  DELEGATION_PROGRAM_ID,
  MAGIC_CONTEXT_ID,
  MAGIC_PROGRAM_ID,
  getDelegationBuffer,
  getDelegationMetadata,
  getDelegationRecord,
  isDelegated,
  makeBaseConnection,
  makeErConnection,
} from '../src/utils/magicblock';
import { PROGRAM_ID } from '../src/manifest';
import { airdropSol } from '../src/utils/solana';

const LOCAL_RPC = 'http://127.0.0.1:8899';
const RUN_LOCALNET = !process.env.NO_LOCAL_VALIDATOR;
const RUN_ER_E2E = !!process.env.MANIFEST_ER_E2E;

// ─────────────────────────────────────────────────────────────────────────────
// Tier 1: UNIT — no network
// ─────────────────────────────────────────────────────────────────────────────

describe('MagicBlock ER — instruction builders (unit)', () => {
  const fakeBase = new PublicKey('So11111111111111111111111111111111111111112');
  const fakeQuote = new PublicKey('EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v');

  it('getMarketAddress is deterministic and PDA-derived', () => {
    const [a] = [getMarketAddress(fakeBase, fakeQuote, 0)];
    const [b] = [getMarketAddress(fakeBase, fakeQuote, 0)];
    expect(a.toBase58()).to.equal(b.toBase58());

    // Different market_id ⇒ different PDA
    const [c] = [getMarketAddress(fakeBase, fakeQuote, 1)];
    expect(a.toBase58()).to.not.equal(c.toBase58());
  });

  it('CreateMarket ix encodes market_id and authority into ix data', () => {
    const payer = Keypair.generate().publicKey;
    const market = getMarketAddress(fakeBase, fakeQuote, 7);
    const ix = createCreateMarketInstruction(
      {
        payer,
        market,
        baseMint: fakeBase,
        quoteMint: fakeQuote,
        baseVault: getVaultAddress(market, fakeBase),
        quoteVault: getVaultAddress(market, fakeQuote),
        tokenProgram22: new PublicKey(
          'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb',
        ),
      },
      { marketId: 7, authority: payer },
    );

    // ix data layout: [discriminator u8][market_id u8][authority Pubkey 32]
    expect(ix.data.length).to.equal(1 + 1 + 32);
    expect(ix.data[0]).to.equal(0); // CreateMarket discriminator
    expect(ix.data[1]).to.equal(7); // market_id
    const authBytes = ix.data.subarray(2, 34);
    expect(Buffer.from(authBytes).equals(payer.toBuffer())).to.equal(true);
  });

  it('DelegateMarket ix has the correct 8 accounts in the right order', () => {
    const authority = Keypair.generate().publicKey;
    const market = getMarketAddress(fakeBase, fakeQuote, 0);

    const ix = createDelegateMarketInstruction(
      { authority, market },
      { minFreeBlocks: 200, validator: null },
    );

    expect(ix.keys.length).to.equal(8);
    expect(ix.keys[0].pubkey.toBase58()).to.equal(authority.toBase58());
    expect(ix.keys[0].isSigner).to.equal(true);
    expect(ix.keys[0].isWritable).to.equal(true);

    expect(ix.keys[1].pubkey.toBase58()).to.equal(
      SystemProgram.programId.toBase58(),
    );
    expect(ix.keys[2].pubkey.toBase58()).to.equal(market.toBase58());
    expect(ix.keys[3].pubkey.toBase58()).to.equal(PROGRAM_ID.toBase58()); // owner_program

    expect(ix.keys[4].pubkey.toBase58()).to.equal(
      getDelegationBuffer(market, PROGRAM_ID).toBase58(),
    );
    expect(ix.keys[5].pubkey.toBase58()).to.equal(
      getDelegationRecord(market).toBase58(),
    );
    expect(ix.keys[6].pubkey.toBase58()).to.equal(
      getDelegationMetadata(market).toBase58(),
    );
    expect(ix.keys[7].pubkey.toBase58()).to.equal(
      DELEGATION_PROGRAM_ID.toBase58(),
    );
  });

  it('CommitMarket ix uses MAGIC_PROGRAM_ID and MAGIC_CONTEXT_ID', () => {
    const payer = Keypair.generate().publicKey;
    const market = getMarketAddress(fakeBase, fakeQuote, 0);
    const ix = createCommitMarketInstruction({ payer, market });

    expect(ix.keys.length).to.equal(4);
    expect(ix.keys[0].isSigner).to.equal(true);
    expect(ix.keys[2].pubkey.toBase58()).to.equal(MAGIC_PROGRAM_ID.toBase58());
    expect(ix.keys[3].pubkey.toBase58()).to.equal(MAGIC_CONTEXT_ID.toBase58());
    expect(ix.keys[3].isWritable).to.equal(true);
  });

  it('CommitAndUndelegateMarket ix mirrors CommitMarket layout', () => {
    const payer = Keypair.generate().publicKey;
    const market = getMarketAddress(fakeBase, fakeQuote, 0);
    const ix = createCommitAndUndelegateMarketInstruction({ payer, market });

    expect(ix.keys.length).to.equal(4);
    expect(ix.data[0]).to.equal(16); // CommitAndUndelegateMarket discriminator
  });

  it('UndelegateMarket ix has 4 accounts and writes to market only', () => {
    const payer = Keypair.generate().publicKey;
    const market = getMarketAddress(fakeBase, fakeQuote, 0);
    const ix = createUndelegateMarketInstruction({ payer, market });

    expect(ix.keys.length).to.equal(4);
    expect(ix.keys[0].pubkey.toBase58()).to.equal(market.toBase58());
    expect(ix.keys[0].isWritable).to.equal(true);
    expect(ix.keys[1].pubkey.toBase58()).to.equal(
      getDelegationBuffer(market, PROGRAM_ID).toBase58(),
    );
    expect(ix.keys[2].isSigner).to.equal(true);
    expect(ix.keys[3].pubkey.toBase58()).to.equal(
      SystemProgram.programId.toBase58(),
    );
    expect(ix.data[0]).to.equal(17); // UndelegateMarket discriminator
  });

  it('makeBaseConnection / makeErConnection produce distinct Connections', () => {
    const base = makeBaseConnection('http://127.0.0.1:8899');
    const er = makeErConnection('http://127.0.0.1:7799', 'ws://127.0.0.1:7800');
    expect(base.rpcEndpoint).to.not.equal(er.rpcEndpoint);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2: LOCALNET — full base-layer flow
// ─────────────────────────────────────────────────────────────────────────────

describe('MagicBlock ER — localnet (base layer)', function () {
  if (!RUN_LOCALNET) {
    it.skip('skipped (NO_LOCAL_VALIDATOR=1)', () => {});
    return;
  }

  this.timeout(60_000);

  let connection: Connection;
  let payer: Keypair;
  let baseMint: PublicKey;
  let quoteMint: PublicKey;
  let market: PublicKey;
  let authority: Keypair;

  before(async function () {
    connection = new Connection(LOCAL_RPC, 'confirmed');
    payer = Keypair.generate();
    authority = Keypair.generate();
    try {
      await airdropSol(connection, payer.publicKey);
      await airdropSol(connection, authority.publicKey);
    } catch (e: any) {
      console.warn('Localnet not available; skipping.', e?.message);
      this.skip();
    }
  });

  it('creates a market at the derived PDA with custom market_id and authority', async () => {
    baseMint = await createMint(
      connection,
      payer,
      payer.publicKey,
      payer.publicKey,
      9,
    );
    quoteMint = await createMint(
      connection,
      payer,
      payer.publicKey,
      payer.publicKey,
      6,
    );
    market = getMarketAddress(baseMint, quoteMint, 3);

    const ix = createCreateMarketInstruction(
      {
        payer: payer.publicKey,
        market,
        baseMint,
        quoteMint,
        baseVault: getVaultAddress(market, baseMint),
        quoteVault: getVaultAddress(market, quoteMint),
        tokenProgram22: new PublicKey(
          'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb',
        ),
      },
      { marketId: 3, authority: authority.publicKey },
    );

    const tx = new Transaction().add(ix);
    await sendAndConfirmTransaction(connection, tx, [payer]);

    const acct = await connection.getAccountInfo(market);
    expect(acct, 'market account should exist').to.not.be.null;
    expect(acct!.owner.toBase58()).to.equal(PROGRAM_ID.toBase58());

    // Read market_id and authority back from the raw bytes per the
    // MarketFixed layout: discriminant(8) + version(1) + 4×u8 + market_id(1)
    // ⇒ market_id at offset 13. Authority at offset 192.
    const data = acct!.data;
    expect(data[13]).to.equal(3);
    const auth = new PublicKey(data.subarray(192, 224));
    expect(auth.toBase58()).to.equal(authority.publicKey.toBase58());
  });

  it('rejects a duplicate CreateMarket on the same PDA', async () => {
    const ix = createCreateMarketInstruction(
      {
        payer: payer.publicKey,
        market,
        baseMint,
        quoteMint,
        baseVault: getVaultAddress(market, baseMint),
        quoteVault: getVaultAddress(market, quoteMint),
        tokenProgram22: new PublicKey(
          'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb',
        ),
      },
      { marketId: 3, authority: authority.publicKey },
    );
    const tx = new Transaction().add(ix);

    let threw = false;
    try {
      await sendAndConfirmTransaction(connection, tx, [payer]);
    } catch {
      threw = true;
    }
    expect(threw, 'second CreateMarket on same PDA should fail').to.equal(true);
  });

  it('isDelegated() returns false for a freshly-created market', async () => {
    expect(await isDelegated(connection, market)).to.equal(false);
  });

  it('DelegateMarket from non-authority is rejected', async () => {
    const stranger = Keypair.generate();
    await airdropSol(connection, stranger.publicKey);
    const ix = createDelegateMarketInstruction(
      { authority: stranger.publicKey, market },
      { minFreeBlocks: 0, validator: null },
    );
    const tx = new Transaction().add(ix);

    let threw = false;
    try {
      await sendAndConfirmTransaction(connection, tx, [stranger]);
    } catch {
      threw = true;
    }
    expect(threw, 'DelegateMarket without authority sig should fail').to.equal(
      true,
    );
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3: ER E2E — live MagicBlock devnet
// ─────────────────────────────────────────────────────────────────────────────

describe('MagicBlock ER — end-to-end session flow', function () {
  if (!RUN_ER_E2E) {
    it.skip('skipped (set MANIFEST_ER_E2E=1 and ER_RPC_URL to enable)', () => {});
    return;
  }

  this.timeout(300_000);
  let baseConn: Connection;
  let erConn: Connection;
  let payer: Keypair;
  let market: PublicKey;

  before(async () => {
    baseConn = makeBaseConnection(process.env.SOLANA_RPC_URL);
    erConn = makeErConnection(
      process.env.ER_RPC_URL!,
      process.env.ER_WS_URL ?? process.env.ER_RPC_URL!.replace('https', 'wss'),
    );
    payer = Keypair.generate();
    await airdropSol(baseConn, payer.publicKey);
  });

  it('full cycle: create → delegate → batchUpdate (ER) → commitAndUndelegate → undelegate', async () => {
    // Implementation note: this is a scaffold that the operator runs against a
    // real environment. It exercises the full surface area but depends on
    // having mints + a delegation_record/metadata that already exist via the
    // delegation program. Pre-fund payer with devnet SOL before running.
    assert.fail(
      'TODO: wire up devnet mints + funded authority before enabling this test',
    );
    // Suppress unused warnings until then.
    void erConn;
    void market;
  });
});
