import * as beet from '@metaplex-foundation/beet';
import * as web3 from '@solana/web3.js';
import { PROGRAM_ID } from '../index';

const RevokeSessionTokenStruct = new beet.BeetArgsStruct<{
  instructionDiscriminator: number;
}>(
  [['instructionDiscriminator', beet.u8]],
  'RevokeSessionTokenInstructionArgs',
);

export const revokeSessionTokenInstructionDiscriminator = 19;

export type RevokeSessionTokenAccounts = {
  owner: web3.PublicKey;
  sessionToken: web3.PublicKey;
};

export function createRevokeSessionTokenInstruction(
  accounts: RevokeSessionTokenAccounts,
  programId: web3.PublicKey = PROGRAM_ID,
): web3.TransactionInstruction {
  const [data] = RevokeSessionTokenStruct.serialize({
    instructionDiscriminator: revokeSessionTokenInstructionDiscriminator,
  });

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.owner, isSigner: true, isWritable: true },
    { pubkey: accounts.sessionToken, isSigner: false, isWritable: true },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
