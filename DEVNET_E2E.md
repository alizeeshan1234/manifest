# Manifest × MagicBlock — Devnet E2E Transactions

End-to-end devnet run of the Manifest CLOB integrated with MagicBlock Ephemeral Rollups.

- **Program ID:** `3nqmFMjrw829a88AU3vSnr4BGraKp1pd8jtLnibeCNnw`
- **Cluster:** Solana devnet
- **Test:** `client/ts/tests/devnetE2e.ts` (13/13 passing, ~29s)
- **Run date:** 2026-05-06

## Maker-side flow

1. `CreateMarket` → `ClaimSeat` → `Deposit` (base layer)
2. `Expand` → `DelegateMarket` (move market to ER)
3. `BatchUpdate` place / cancel on the ER
4. `CommitAndUndelegateMarket` → `Re-DelegateMarket`
5. `RequestDeposit` (1-sig deposit-while-delegated, Path A)
6. `RequestWithdrawal` (1-sig withdrawal-while-delegated, Path A)

## Transaction signatures

| Step | Tx |
|---|---|
| CreateMarket | [5PxuExqDBMdJszehwdk2P2UjZrnXdLL6RQFFvMswLq8ZDaTuqcTzEfmsK1bT9n9Cu9EZVpaioxk4FUfBcb6aujkC](https://explorer.solana.com/tx/5PxuExqDBMdJszehwdk2P2UjZrnXdLL6RQFFvMswLq8ZDaTuqcTzEfmsK1bT9n9Cu9EZVpaioxk4FUfBcb6aujkC?cluster=devnet) |
| ClaimSeat | [3iM2fRUuY2dByGUDKJge2Je3re8MoSWNyTB7vLB51GZqyWpgbw5UQzh56BsJwpPHf1VB2BczC5LfE8qT1VHMBCf9](https://explorer.solana.com/tx/3iM2fRUuY2dByGUDKJge2Je3re8MoSWNyTB7vLB51GZqyWpgbw5UQzh56BsJwpPHf1VB2BczC5LfE8qT1VHMBCf9?cluster=devnet) |
| Deposit (base) | [4i3B1L8YvQB33j3kaBoMnxM6nCG98c285uFqQfVDyatEfhoyAqqtAwZHBqAvqqYx8gujJ8on35H9v8a9irwJ5K2K](https://explorer.solana.com/tx/4i3B1L8YvQB33j3kaBoMnxM6nCG98c285uFqQfVDyatEfhoyAqqtAwZHBqAvqqYx8gujJ8on35H9v8a9irwJ5K2K?cluster=devnet) |
| Deposit (quote) | [2S2ovqPDNaiTqCyzNdktnhspeXk1AYAbPsr8vbVmCd5YooC3ouowpnnQzk2v1Yy8L4PdEqwHdejWguMm2oGRuuax](https://explorer.solana.com/tx/2S2ovqPDNaiTqCyzNdktnhspeXk1AYAbPsr8vbVmCd5YooC3ouowpnnQzk2v1Yy8L4PdEqwHdejWguMm2oGRuuax?cluster=devnet) |
| Expand market | [3fSeuV487jJ7fZka5HPJDsPP9eJVzfDJfg6tK38YbHrd65FpwfmyaF43LKjayC24jvbWD8Zrvw5iGUhNfRskGiup](https://explorer.solana.com/tx/3fSeuV487jJ7fZka5HPJDsPP9eJVzfDJfg6tK38YbHrd65FpwfmyaF43LKjayC24jvbWD8Zrvw5iGUhNfRskGiup?cluster=devnet) |
| DelegateMarket | [2u2ttrL5rvnRUyGdgmTCu27em5ds71SLZPjbT1jaDfpMbhon11RtkdGDgkFtXf6adxsX87qvqVFxFAcZGcggn5VD](https://explorer.solana.com/tx/2u2ttrL5rvnRUyGdgmTCu27em5ds71SLZPjbT1jaDfpMbhon11RtkdGDgkFtXf6adxsX87qvqVFxFAcZGcggn5VD?cluster=devnet) |
| BatchUpdate place (ER) | [3KkNwpiUAUvGAL2wDUccsaJGRRC4EvoEVyAWeMDB4VDNR4ktNvY5bPFUfp31B7biMSSvqfcEbqCZ3nY7dZp42D89](https://explorer.solana.com/tx/3KkNwpiUAUvGAL2wDUccsaJGRRC4EvoEVyAWeMDB4VDNR4ktNvY5bPFUfp31B7biMSSvqfcEbqCZ3nY7dZp42D89?cluster=devnet) |
| BatchUpdate cancel (ER) | [3W34Uox3Z9rvdr3x9fi9PT2Z9bPchQ81ArriKxn6KxYHvmZ4p8B7jCpSqFTYrUsEB5MTWffYFTB9MwqocGDuPvcr](https://explorer.solana.com/tx/3W34Uox3Z9rvdr3x9fi9PT2Z9bPchQ81ArriKxn6KxYHvmZ4p8B7jCpSqFTYrUsEB5MTWffYFTB9MwqocGDuPvcr?cluster=devnet) |
| CommitAndUndelegate | [45zSXuH4rneXUQHxkQE8DEmxBfuHbwhHRJRoveX93ktDufmR9AxTvweKBncwZi1yC668M68LghSs7x1ukdZ9GKYX](https://explorer.solana.com/tx/45zSXuH4rneXUQHxkQE8DEmxBfuHbwhHRJRoveX93ktDufmR9AxTvweKBncwZi1yC668M68LghSs7x1ukdZ9GKYX?cluster=devnet) |
| Re-DelegateMarket | [541UTZN3bBbpo5xGGoT5ThVLepvyh7dBQm4SbsCZ7kvkKLRVCotqhvsirgT8qExetNn613waKLAFEjscZntVprqL](https://explorer.solana.com/tx/541UTZN3bBbpo5xGGoT5ThVLepvyh7dBQm4SbsCZ7kvkKLRVCotqhvsirgT8qExetNn613waKLAFEjscZntVprqL?cluster=devnet) |
| RequestDeposit | [3wyN4uEeMsf8ZNmvTTii729rhM23Ewy6mqNY7UsnKQaNTzkYmbzwBbgehFxSgVqoMkpgDb2rDpeSGmdk8vmmci4d](https://explorer.solana.com/tx/3wyN4uEeMsf8ZNmvTTii729rhM23Ewy6mqNY7UsnKQaNTzkYmbzwBbgehFxSgVqoMkpgDb2rDpeSGmdk8vmmci4d?cluster=devnet) |
| RequestWithdrawal | [5R9JAqR2RXEpapUiebAA3RtsL3ph81BSFY1UBQ9JhhDMfbQZchyjB98AstLbgn7dKWPWLMo8uFo4ZxfPnk58Lc5F](https://explorer.solana.com/tx/5R9JAqR2RXEpapUiebAA3RtsL3ph81BSFY1UBQ9JhhDMfbQZchyjB98AstLbgn7dKWPWLMo8uFo4ZxfPnk58Lc5F?cluster=devnet) |
| Program upgrade | [4gewaGzcF5Sr3NEDNWL7iC8qBdzsAECGmE4VfnAwq4ZuVvGCgJeuQkE3mqHzxaMdMbqvKRE8vnNTrefJgeozbus4](https://explorer.solana.com/tx/4gewaGzcF5Sr3NEDNWL7iC8qBdzsAECGmE4VfnAwq4ZuVvGCgJeuQkE3mqHzxaMdMbqvKRE8vnNTrefJgeozbus4?cluster=devnet) |

## Reproduce

```
yarn test:devnet
```
