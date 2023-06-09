use candid::{Decode, Encode, Principal};
use eth_signer::{Signer, Wallet};
use ethers_core::k256::ecdsa::SigningKey;
use ethers_core::types::transaction::eip2718::TypedTransaction;
use ethers_core::types::TransactionRequest;
use evmc_did::error::EvmError;
use evmc_did::registration_info::RegistrationInfo;
use evmc_did::{BasicAccount, Transaction, H160};
use ic_agent::Agent;

use crate::agent::user_principal;
use crate::constant::{
    METHOD_ACCOUNT_BASIC, METHOD_ADDRESS_REGISTERED, METHOD_MINT_NATIVE_TOKENS,
    METHOD_REGISTER_IC_AGENT, METHOD_REGISTRATION_IC_AGENT_INFO, METHOD_VERIFY_REGISTRATION,
};
use crate::error::{Error, Result};

pub struct RegistrationService<'a> {
    agent: Agent,
    amount_to_mint: Option<u64>,
    chain_id: u64,
    evmc_canister_id: Principal,
    register_canister_id: Principal,
    registration_info: RegistrationInfo,
    wallet: Wallet<'a, SigningKey>,
}

impl<'a> RegistrationService<'a> {
    pub async fn new(
        agent: Agent,
        amount_to_mint: Option<u64>,
        chain_id: u64,
        evmc_canister_id: Principal,
        register_canister_id: Principal,
        wallet: Wallet<'a, SigningKey>,
    ) -> Result<RegistrationService<'a>> {
        info!("collecting registration info");
        let registration_info = Self::get_registration_info(&agent, &evmc_canister_id).await?;
        info!("registration service initialized");

        Ok(Self {
            agent,
            amount_to_mint,
            chain_id,
            evmc_canister_id,
            register_canister_id,
            registration_info,
            wallet,
        })
    }

    pub async fn register(&self) -> Result<()> {
        self.register_ic_agent().await?;
        self.verify_registration().await?;

        Ok(())
    }

    async fn register_ic_agent(&self) -> Result<()> {
        let principal = user_principal(&self.agent)?;
        info!("registering ic-agent {principal}");
        let is_registered = self.is_address_registered().await?;
        if is_registered {
            info!("agent is already registered");
            return Err(Error::AlreadyRegistered(principal));
        }

        let tx = self.registration_transaction().await?;
        let args = Encode!(&Transaction::from(tx), &self.register_canister_id)?;

        // mint tokens to be able to pay registration fee (only on testnets)
        if let Some(amount_to_mint) = self.amount_to_mint {
            info!("minting native tokens for address");
            self.mint_native_tokens_to_address(amount_to_mint).await?;
        }

        let res = self
            .agent
            .update(&self.evmc_canister_id, METHOD_REGISTER_IC_AGENT)
            .with_arg(args)
            .call_and_wait()
            .await?;

        info!("{METHOD_REGISTER_IC_AGENT} called, decoding result");
        Decode!(res.as_slice(), std::result::Result<(), EvmError>)??;
        info!("result is OK");

        Ok(())
    }

    async fn verify_registration(&self) -> Result<()> {
        info!("verifying registration...");
        let args = Encode!(
            &self.wallet.signer().to_bytes().to_vec(),
            &self.register_canister_id
        )?;

        let res = self
            .agent
            .update(&self.evmc_canister_id, METHOD_VERIFY_REGISTRATION)
            .with_arg(args)
            .call_and_wait()
            .await?;

        info!("{METHOD_VERIFY_REGISTRATION} called, decoding result");

        Decode!(res.as_slice(), std::result::Result<(), EvmError>)??;

        info!("result is OK");

        Ok(())
    }

    async fn is_address_registered(&self) -> Result<bool> {
        let address: H160 = self.wallet.address().into();
        info!("checking if {address} is already registered...");
        let args = Encode!(&address, &self.register_canister_id)?;
        let res = self
            .agent
            .query(&self.evmc_canister_id, METHOD_ADDRESS_REGISTERED)
            .with_arg(args)
            .call()
            .await?;
        let principal = user_principal(&self.agent)?;
        match Decode!(res.as_slice(), bool) {
            Ok(res) => {
                info!("{address} is not registered yet");
                Ok(res)
            }
            Err(_) => Err(Error::CouldNotCheckRegistrationStatus(
                address.to_hex_str(),
                principal,
            )),
        }
    }

    async fn registration_transaction(&self) -> Result<ethers_core::types::Transaction> {
        let to = ethers_core::types::H160::from(self.registration_info.minter_address.clone());
        let address = self.wallet.address();

        let args = Encode!(&H160::from(address))?;

        let res = self
            .agent
            .query(&self.evmc_canister_id, METHOD_ACCOUNT_BASIC)
            .with_arg(args)
            .call()
            .await?;

        let nonce = Decode!(res.as_slice(), BasicAccount)?.nonce;

        info!("creating registration transaction (from: {address}, to: {to}, value: {}, nonce: {nonce}, gas_price: 0, gas: 53000)", self.registration_info.registration_fee);

        let tx: TypedTransaction = TransactionRequest::new()
            .from(address)
            .to(to)
            .value(self.registration_info.registration_fee)
            .chain_id(self.chain_id)
            .nonce(nonce)
            .gas_price(0)
            .gas(53000)
            .into();
        let signature = self.wallet.sign_transaction(&tx).await.unwrap();
        let bytes = tx.rlp_signed(&signature);
        let mut tx: ethers_core::types::Transaction = rlp::decode(&bytes).unwrap();
        tx.from = address;

        Ok(tx)
    }

    async fn mint_native_tokens_to_address(&self, amount_to_mint: u64) -> Result<()> {
        let address = H160::from(self.wallet.address());
        info!("minting EVM tokens to {address}");
        let payload = Encode!(&address, &evmc_did::U256::from(amount_to_mint))?;

        let res = self
            .agent
            .update(&self.evmc_canister_id, METHOD_MINT_NATIVE_TOKENS)
            .with_arg(payload)
            .call_and_wait()
            .await?;

        Decode!(res.as_slice(), std::result::Result<evmc_did::U256, EvmError>)??;

        info!("tokens minted");

        Ok(())
    }

    async fn get_registration_info(
        agent: &Agent,
        evmc_canister_id: &Principal,
    ) -> Result<RegistrationInfo> {
        let args = Encode!()?;

        let res = agent
            .query(evmc_canister_id, METHOD_REGISTRATION_IC_AGENT_INFO)
            .with_arg(args)
            .call()
            .await?;
        match Decode!(res.as_slice(), RegistrationInfo) {
            Ok(res) => Ok(res),
            Err(e) => Err(Error::CouldNotGetRegistrationInfo(e.to_string())),
        }
    }
}
