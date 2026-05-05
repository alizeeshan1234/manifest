import { Connection, PublicKey } from '@solana/web3.js';

/** MagicBlock delegation program ID. */
export const DELEGATION_PROGRAM_ID = new PublicKey(
  'DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh',
);

/** MagicBlock program (used inside the ER for commit / undelegate scheduling). */
export const MAGIC_PROGRAM_ID = new PublicKey(
  'Magic11111111111111111111111111111111111111',
);

/** MagicBlock context account. */
export const MAGIC_CONTEXT_ID = new PublicKey(
  'MagicContext1111111111111111111111111111111',
);

/** Default endpoints. Override via env in production. */
export const DEFAULT_BASE_RPC =
  process.env.SOLANA_RPC_URL ?? 'https://api.devnet.solana.com';
export const DEFAULT_ER_RPC =
  process.env.EPHEMERAL_PROVIDER_ENDPOINT ?? 'https://devnet.magicblock.app';
export const DEFAULT_ER_WS =
  process.env.EPHEMERAL_WS_ENDPOINT ?? 'wss://devnet.magicblock.app';

/** Build a base-layer RPC connection. */
export function makeBaseConnection(rpcUrl: string = DEFAULT_BASE_RPC): Connection {
  return new Connection(rpcUrl, { commitment: 'confirmed' });
}

/** Build an ephemeral-rollup RPC connection. */
export function makeErConnection(
  rpcUrl: string = DEFAULT_ER_RPC,
  wsUrl: string = DEFAULT_ER_WS,
): Connection {
  return new Connection(rpcUrl, {
    commitment: 'confirmed',
    wsEndpoint: wsUrl,
  });
}

/** Returns true if the given account is currently delegated to the MagicBlock ER. */
export async function isDelegated(
  connection: Connection,
  accountKey: PublicKey,
): Promise<boolean> {
  const info = await connection.getAccountInfo(accountKey);
  if (info === null) return false;
  return info.owner.equals(DELEGATION_PROGRAM_ID);
}

/** Derive the MagicBlock buffer PDA used during delegation. */
export function getDelegationBuffer(
  pda: PublicKey,
  ownerProgram: PublicKey,
): PublicKey {
  const [buf] = PublicKey.findProgramAddressSync(
    [Buffer.from('buffer'), pda.toBuffer()],
    ownerProgram,
  );
  return buf;
}

/** Derive the MagicBlock delegation record PDA. */
export function getDelegationRecord(pda: PublicKey): PublicKey {
  const [rec] = PublicKey.findProgramAddressSync(
    [Buffer.from('delegation'), pda.toBuffer()],
    DELEGATION_PROGRAM_ID,
  );
  return rec;
}

/** Derive the MagicBlock delegation metadata PDA. */
export function getDelegationMetadata(pda: PublicKey): PublicKey {
  const [meta] = PublicKey.findProgramAddressSync(
    [Buffer.from('delegation-metadata'), pda.toBuffer()],
    DELEGATION_PROGRAM_ID,
  );
  return meta;
}
