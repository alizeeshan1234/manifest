import * as beet from '@metaplex-foundation/beet';
import * as web3 from '@solana/web3.js';
import { MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID } from '../../utils/magicblock';

const CommitAndUndelegateStruct = new beet.BeetArgsStruct<{
  instructionDiscriminator: number;
}>(
  [['instructionDiscriminator', beet.u8]],
  'CommitAndUndelegateMarketInstructionArgs',
);

export const commitAndUndelegateMarketInstructionDiscriminator = 16;

export type CommitAndUndelegateMarketAccounts = {
  payer: web3.PublicKey;
  market: web3.PublicKey;
};

/**
 * Send to the EPHEMERAL ROLLUP. Snapshots final state and queues
 * undelegation. Pair with `UndelegateMarket` on the base layer to finalize.
 */
export function createCommitAndUndelegateMarketInstruction(
  accounts: CommitAndUndelegateMarketAccounts,
  programId: web3.PublicKey = new web3.PublicKey(
    '3nqmFMjrw829a88AU3vSnr4BGraKp1pd8jtLnibeCNnw',
  ),
): web3.TransactionInstruction {
  const [data] = CommitAndUndelegateStruct.serialize({
    instructionDiscriminator: commitAndUndelegateMarketInstructionDiscriminator,
  });

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.payer, isSigner: true, isWritable: true },
    { pubkey: accounts.market, isSigner: false, isWritable: true },
    { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
    { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
