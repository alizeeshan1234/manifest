import * as beet from '@metaplex-foundation/beet';
import * as web3 from '@solana/web3.js';
import { MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID } from '../../utils/magicblock';

const CommitMarketStruct = new beet.BeetArgsStruct<{
  instructionDiscriminator: number;
}>(
  [['instructionDiscriminator', beet.u8]],
  'CommitMarketInstructionArgs',
);

export const commitMarketInstructionDiscriminator = 15;

export type CommitMarketAccounts = {
  payer: web3.PublicKey;
  market: web3.PublicKey;
};

/**
 * Send to the EPHEMERAL ROLLUP. Snapshots the delegated market state to
 * the base layer; the market stays delegated.
 */
export function createCommitMarketInstruction(
  accounts: CommitMarketAccounts,
  programId: web3.PublicKey = new web3.PublicKey(
    'MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms',
  ),
): web3.TransactionInstruction {
  const [data] = CommitMarketStruct.serialize({
    instructionDiscriminator: commitMarketInstructionDiscriminator,
  });

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.payer, isSigner: true, isWritable: true },
    { pubkey: accounts.market, isSigner: false, isWritable: true },
    { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
    { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
