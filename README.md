# Pole Aggregator

## Build Requirements
- Anchor v0.18.0
- Solana v1.7.8

## To start a front-end test environment

```
anchor localnet
```

In a separate terminal run the following command to set up all the on-chain program:
```
anchor migrate
```

Change directory and run the front end
```
cd app
yarn start
```


### Update Program Run Book
```bash
solana-keygen new -o <new-buffer-name>
cargo build-bpf
solana program write-buffer <compiled_so_file_path> --output json --buffer <new-buffer-name>
solana program set-buffer-authority <buffer-pubkey> --new-buffer-authority <program_upgrade_authority>
solana program deploy --buffer <buffer-pubkey> --program-id <program-id-json> --keypair usb://ledger
```


## Pool Addresses
USDC Pole Pool: `55sakCELRnCfAQNn968tcWHn17cGYWgqxW7pAnqtErMH`


## Multisig Address

- Authority: `DfU79xbq1PT56VAudnM1fErf466sFFJybJvBmrwjch2c`
- USDC token account: `FCGrEGR3Nq8UFBJrGxWM4TZdMmij1ZQ5mb3Ab9r2BsSP`
