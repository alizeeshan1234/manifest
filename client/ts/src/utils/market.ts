import { PROGRAM_ID } from '../manifest/index';

import { PublicKey } from '@solana/web3.js';

export function getVaultAddress(market: PublicKey, mint: PublicKey): PublicKey {
  const [vaultAddress, _unusedBump] = PublicKey.findProgramAddressSync(
    [Buffer.from('vault'), market.toBuffer(), mint.toBuffer()],
    PROGRAM_ID,
  );
  return vaultAddress;
}

/**
 * Derive the market PDA. Markets are now PDAs at
 * [b"market", base_mint, quote_mint, market_id_byte]. The `marketId` byte
 * disambiguates multiple markets for the same (base, quote) pair.
 */
export function getMarketAddress(
  baseMint: PublicKey,
  quoteMint: PublicKey,
  marketId: number = 0,
): PublicKey {
  const [marketAddress] = PublicKey.findProgramAddressSync(
    [
      Buffer.from('market'),
      baseMint.toBuffer(),
      quoteMint.toBuffer(),
      Buffer.from([marketId & 0xff]),
    ],
    PROGRAM_ID,
  );
  return marketAddress;
}
