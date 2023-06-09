use std::path::PathBuf;

use anyhow::Result;
use candid::Principal;
use clap::{Args, Parser, Subcommand};
use eth_signer::{Signer, Wallet};
use ethers_core::k256::ecdsa::SigningKey;
use evmc_did::H160;

use super::registration::RegistrationService;
use crate::agent::init_agent;
use crate::error::Error;

const DEFAULT_CHAIN_ID: u64 = 355113;
/// network name for production
const NETWORK_IC: &str = "ic";
/// network name for local replica
const NETWORK_LOCAL: &str = "local";

/// CLI tool for generating wallet & registering minter principal to the evmc
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct RegisterMinterCli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate an ETH Wallet
    GenerateWallet,

    /// Register a minter principal to the evmc
    Register(RegisterArgs),
}

#[derive(Args)]
pub struct RegisterArgs {
    /// amount of native tokens to mint on testnets for this wallet
    #[arg(short = 'a', long = "amount-to-mint")]
    pub amount_to_mint: Option<u64>,

    /// chain id
    #[arg(short = 'C', long = "chain-id", default_value_t = DEFAULT_CHAIN_ID)]
    pub chain_id: u64,

    /// Path to your identity pem file
    #[arg(short = 'i', long = "identity")]
    pub identity: PathBuf,

    /// Evmc canister principal
    #[arg(short = 'e', long = "evmc")]
    pub evmc: Principal,

    /// IC Network (ic, local or custom url)
    #[arg(short, long, default_value_t = String::from(NETWORK_LOCAL))]
    pub network: String,

    /// Principal of the canister to register
    #[arg(short = 'c', long = "canister-id")]
    pub register_canister_id: Principal,

    /// wallet signing key
    #[arg(short = 'k', long = "key")]
    pub signing_key: String,
}

impl RegisterArgs {
    pub async fn exec(&self) -> Result<()> {
        let wallet = get_wallet(self.signing_key.as_str())?;
        let address = wallet.address();

        info!("initializing agent...");
        let network = network_url(&self.network);
        let agent = init_agent(&self.identity, network).await?;

        match RegistrationService::new(
            agent,
            self.amount_to_mint,
            self.chain_id,
            self.evmc,
            self.register_canister_id,
            wallet,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .register()
        .await
        {
            Ok(()) => {
                println!(
                    "Registration succeeded:\n  Wallet Address = {}\n  Principal = {}",
                    H160::from(address).to_hex_str(),
                    self.register_canister_id
                );

                Ok(())
            }
            Err(Error::AlreadyRegistered(principal)) => {
                println!(
                    "Already registered:\n\tWallet Address = {}\n\tPrincipal = {}",
                    H160::from(address).to_hex_str(),
                    principal
                );
                Ok(())
            }
            Err(err) => anyhow::bail!("{err}"),
        }
    }
}

/// Parse an existing wallet
fn get_wallet<'a>(signing_key: &str) -> Result<Wallet<'a, SigningKey>> {
    let key_bytes = hex::decode(signing_key)?;
    let wallet = Wallet::from_bytes(&key_bytes)?;
    Ok(wallet)
}

/// generate a brand new wallet
pub fn generate_wallet<'a>() -> Result<Wallet<'a, SigningKey>> {
    let mut rng = rand::thread_rng();
    let wallet = Wallet::new(&mut rng);
    let signer = wallet.signer();
    let signer_hex = hex::encode(signer.to_bytes());
    let public_key = wallet.signer().verifying_key();
    let public_key_hex = hex::encode(public_key.to_sec1_bytes());
    let address: H160 = wallet.address().into();
    println!(
        "Wallet:\n  Private Key = {}\n  Public Key = {}\n  Address = {}",
        signer_hex,
        public_key_hex,
        address.to_hex_str(),
    );
    Ok(wallet)
}

/// make network url from network name
fn network_url(network: &str) -> &str {
    match network {
        NETWORK_LOCAL => "http://localhost:8000",
        NETWORK_IC => "https://ic0.app",
        url => url,
    }
}
