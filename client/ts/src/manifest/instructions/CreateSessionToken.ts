import * as beet from '@metaplex-foundation/beet';
import * as beetSolana from '@metaplex-foundation/beet-solana';
import * as web3 from '@solana/web3.js';
import BN from 'bn.js';
import { PROGRAM_ID } from '../index';

const CreateSessionTokenStruct = new beet.BeetArgsStruct<{
  instructionDiscriminator: number;
  sessionSigner: web3.PublicKey;
  expiresAt: beet.bignum;
}>(
  [
    ['instructionDiscriminator', beet.u8],
    ['sessionSigner', beetSolana.publicKey],
    ['expiresAt', beet.i64],
  ],
  'CreateSessionTokenInstructionArgs',
);

export const createSessionTokenInstructionDiscriminator = 18;

export type CreateSessionTokenAccounts = {
  owner: web3.PublicKey;
  sessionToken: web3.PublicKey;
  systemProgram?: web3.PublicKey;
};

export function getSessionTokenAddress(
  owner: web3.PublicKey,
  sessionSigner: web3.PublicKey,
  programId: web3.PublicKey = PROGRAM_ID,
): web3.PublicKey {
  const [pda] = web3.PublicKey.findProgramAddressSync(
    [Buffer.from('session'), owner.toBuffer(), sessionSigner.toBuffer()],
    programId,
  );
  return pda;
}

export function createCreateSessionTokenInstruction(
  accounts: CreateSessionTokenAccounts,
  args: { sessionSigner: web3.PublicKey; expiresAt: BN },
  programId: web3.PublicKey = PROGRAM_ID,
): web3.TransactionInstruction {
  const [data] = CreateSessionTokenStruct.serialize({
    instructionDiscriminator: createSessionTokenInstructionDiscriminator,
    sessionSigner: args.sessionSigner,
    expiresAt: args.expiresAt,
  });

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.owner, isSigner: true, isWritable: true },
    { pubkey: accounts.sessionToken, isSigner: false, isWritable: true },
    {
      pubkey: accounts.systemProgram ?? web3.SystemProgram.programId,
      isSigner: false,
      isWritable: false,
    },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
