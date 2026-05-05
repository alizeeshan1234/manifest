import * as beet from '@metaplex-foundation/beet';
import * as web3 from '@solana/web3.js';
import { getDelegationBuffer } from '../../utils/magicblock';

const UndelegateMarketStruct = new beet.BeetArgsStruct<{
  instructionDiscriminator: number;
}>(
  [['instructionDiscriminator', beet.u8]],
  'UndelegateMarketInstructionArgs',
);

export const undelegateMarketInstructionDiscriminator = 17;

export type UndelegateMarketAccounts = {
  market: web3.PublicKey;
  payer: web3.PublicKey;
  systemProgram?: web3.PublicKey;
};

/**
 * Send to the BASE LAYER. Finalizes the undelegation queued by
 * `CommitAndUndelegateMarket` on the ER. Returns market ownership to
 * Manifest.
 */
export function createUndelegateMarketInstruction(
  accounts: UndelegateMarketAccounts,
  programId: web3.PublicKey = new web3.PublicKey(
    '3nqmFMjrw829a88AU3vSnr4BGraKp1pd8jtLnibeCNnw',
  ),
): web3.TransactionInstruction {
  const [data] = UndelegateMarketStruct.serialize({
    instructionDiscriminator: undelegateMarketInstructionDiscriminator,
  });

  const buffer = getDelegationBuffer(accounts.market, programId);

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.market, isSigner: false, isWritable: true },
    { pubkey: buffer, isSigner: false, isWritable: false },
    { pubkey: accounts.payer, isSigner: true, isWritable: true },
    {
      pubkey: accounts.systemProgram ?? web3.SystemProgram.programId,
      isSigner: false,
      isWritable: false,
    },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
