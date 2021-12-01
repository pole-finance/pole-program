/**
 * Utility for interacting with account data directly.
 *
 * Typically useful for mocking information being placed on-chain
 * by other programs, such as price oracles (e.g. Pyth).
 *
 * This depends on the associated `TestWriter` program, which can
 * process instructions to modify an account's data.
 */

 import * as anchor from "@project-serum/anchor";
 import {
     Keypair,
     SystemProgram,
     Transaction,
 } from "@solana/web3.js";
 
 
 const writer = anchor.workspace.TestWriter;
 
 export class DataManager {
     static readonly programId = writer.programId;
 
     provider: anchor.Provider;
 
     constructor(provider: anchor.Provider) {
         this.provider = provider;
     }
 
     /**
      * Create a new account for storing arbitrary data
      * @param space The data size to reserve for this account
      * @returns The keypair for the created accounts.
      */
     async createAccount(space: number): Promise<Keypair> {
        const newAccount = Keypair.generate();
        const createTx = new Transaction().add(
          SystemProgram.createAccount({
            fromPubkey: this.provider.wallet.publicKey,
            newAccountPubkey: newAccount.publicKey,
            programId: writer.programId,
            lamports: await this.provider.connection.getMinimumBalanceForRentExemption(
              space
            ),
            space,
            })
         );
        
        await this.provider.send(
          createTx,
          [newAccount]
        );

        return newAccount;
     }
 
     /**
      * Change the data stored in a configuration account
      * @param account The keypair for the account to modify
      * @param offset The starting offset of the section of the account data to modify.
      * @param input The data to store in the account
      */
     async store(account: Keypair, offset: number, input: Buffer) {
         const writeInstr = writer.instruction.write(
             new anchor.BN(offset),
             input,
             {
                 accounts: { target: account.publicKey },
             }
         );
         const writeTx = new Transaction({
             feePayer: this.provider.wallet.publicKey,
         }).add(writeInstr);
         
         await this.provider.send(
           writeTx,
           [account]
         );
     }
 }