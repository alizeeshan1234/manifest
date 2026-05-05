import * as beet from '@metaplex-foundation/beet';
import * as beetSolana from '@metaplex-foundation/beet-solana';
import * as web3 from '@solana/web3.js';
import {
  DELEGATION_PROGRAM_ID,
  getDelegationBuffer,
  getDelegationMetadata,
  getDelegationRecord,
} from '../../utils/magicblock';

const DelegateMarketStruct = new beet.FixableBeetArgsStruct<{
  instructionDiscriminator: number;
  minFreeBlocks: number;
  validator: beet.COption<web3.PublicKey>;
}>(
  [
    ['instructionDiscriminator', beet.u8],
    ['minFreeBlocks', beet.u32],
    ['validator', beet.coption(beetSolana.publicKey)],
  ],
  'DelegateMarketInstructionArgs',
);

export const delegateMarketInstructionDiscriminator = 14;

export type DelegateMarketAccounts = {
  authority: web3.PublicKey;
  market: web3.PublicKey;
  systemProgram?: web3.PublicKey;
};

export function createDelegateMarketInstruction(
  accounts: DelegateMarketAccounts,
  args: { minFreeBlocks: number; validator?: web3.PublicKey | null },
  programId: web3.PublicKey = new web3.PublicKey(
    'MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms',
  ),
): web3.TransactionInstruction {
  const [data] = DelegateMarketStruct.serialize({
    instructionDiscriminator: delegateMarketInstructionDiscriminator,
    minFreeBlocks: args.minFreeBlocks,
    validator: args.validator ?? null,
  });

  const buffer = getDelegationBuffer(accounts.market, programId);
  const record = getDelegationRecord(accounts.market);
  const metadata = getDelegationMetadata(accounts.market);

  const keys: web3.AccountMeta[] = [
    { pubkey: accounts.authority, isSigner: true, isWritable: true },
    {
      pubkey: accounts.systemProgram ?? web3.SystemProgram.programId,
      isSigner: false,
      isWritable: false,
    },
    { pubkey: accounts.market, isSigner: false, isWritable: true },
    { pubkey: programId, isSigner: false, isWritable: false }, // owner_program
    { pubkey: buffer, isSigner: false, isWritable: true },
    { pubkey: record, isSigner: false, isWritable: true },
    { pubkey: metadata, isSigner: false, isWritable: true },
    { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
  ];

  return new web3.TransactionInstruction({ programId, keys, data });
}
