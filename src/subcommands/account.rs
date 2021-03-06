use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ckb_sdk::{
    wallet::{DerivationPath, Key, KeyStore, MasterPrivKey},
    Address, AddressPayload, NetworkType,
};
use ckb_types::{packed::Script, prelude::*, H160, H256};
use clap::{App, Arg, ArgMatches, SubCommand};

use super::CliSubCommand;
use crate::utils::{
    arg_parser::{
        ArgParser, DurationParser, ExtendedPrivkeyPathParser, FixedHashParser, FromStrParser,
        PrivkeyPathParser, PrivkeyWrapper,
    },
    other::read_password,
    printer::{OutputFormat, Printable},
};

pub struct AccountSubCommand<'a> {
    key_store: &'a mut KeyStore,
}

impl<'a> AccountSubCommand<'a> {
    pub fn new(key_store: &'a mut KeyStore) -> AccountSubCommand<'a> {
        AccountSubCommand { key_store }
    }

    pub fn subcommand(name: &'static str) -> App<'static, 'static> {
        let arg_lock_arg = Arg::with_name("lock-arg")
            .long("lock-arg")
            .takes_value(true)
            .validator(|input| FixedHashParser::<H160>::default().validate(input))
            .required(true)
            .help("The lock_arg (identifier) of the account");
        let arg_privkey_path = Arg::with_name("privkey-path")
            .long("privkey-path")
            .takes_value(true);
        let arg_extended_privkey_path = Arg::with_name("extended-privkey-path")
            .long("extended-privkey-path")
            .takes_value(true)
            .help("Extended private key path (include master private key and chain code)");
        SubCommand::with_name(name)
            .about("Manage accounts")
            .subcommands(vec![
                SubCommand::with_name("list").about("List all accounts"),
                SubCommand::with_name("new").about("Create a new account and print related information."),
                SubCommand::with_name("import")
                    .about("Import an unencrypted private key from <privkey-path> and create a new account.")
                    .arg(
                        arg_privkey_path
                            .clone()
                            .required_unless("extended-privkey-path")
                            .validator(|input| PrivkeyPathParser.validate(input))
                            .help("The privkey is assumed to contain an unencrypted private key in hexadecimal format. (only read first line)")
                    )
                    .arg(arg_extended_privkey_path
                         .clone()
                         .required_unless("privkey-path")
                         .validator(|input| ExtendedPrivkeyPathParser.validate(input))
                    ),
                SubCommand::with_name("unlock")
                    .about("Unlock an account")
                    .arg(arg_lock_arg.clone())
                    .arg(
                        Arg::with_name("keep")
                            .long("keep")
                            .takes_value(true)
                            .validator(|input| DurationParser.validate(input))
                            .required(true)
                            .help("How long before the key expired, format: 30s, 15m, 1h (repeat unlock will increase the time)")
                    ),
                SubCommand::with_name("update")
                    .about("Update password of an account")
                    .arg(arg_lock_arg.clone()),
                SubCommand::with_name("export")
                    .about("Export master private key and chain code as hex plain text (USE WITH YOUR OWN RISK)")
                    .arg(arg_lock_arg.clone())
                    .arg(
                        arg_extended_privkey_path
                            .clone()
                            .required(true)
                            .help("Output extended private key path (PrivKey + ChainCode)")
                    ),
                SubCommand::with_name("bip44-addresses")
                    .about("Extended receiving/change Addresses (see: BIP-44)")
                    .arg(
                        Arg::with_name("from-receiving-index")
                            .long("from-receiving-index")
                            .takes_value(true)
                            .default_value("0")
                            .validator(|input| FromStrParser::<u32>::default().validate(input))
                            .help("Start from receiving path index")
                    )
                    .arg(
                        Arg::with_name("receiving-length")
                            .long("receiving-length")
                            .takes_value(true)
                            .default_value("20")
                            .validator(|input| FromStrParser::<u32>::default().validate(input))
                            .help("Receiving addresses length")
                    )
                    .arg(
                        Arg::with_name("from-change-index")
                            .long("from-change-index")
                            .takes_value(true)
                            .default_value("0")
                            .validator(|input| FromStrParser::<u32>::default().validate(input))
                            .help("Start from change path index")
                    )
                    .arg(
                        Arg::with_name("change-length")
                            .long("change-length")
                            .takes_value(true)
                            .default_value("10")
                            .validator(|input| FromStrParser::<u32>::default().validate(input))
                            .help("Change addresses length")
                    )
                    .arg(arg_lock_arg.clone()),
                SubCommand::with_name("extended-address")
                    .about("Extended address (see: BIP-44)")
                    .arg(arg_lock_arg.clone())
                    .arg(
                        Arg::with_name("path")
                            .long("path")
                            .takes_value(true)
                            .validator(|input| FromStrParser::<DerivationPath>::new().validate(input))
                            .help("The address path")
                    ),
            ])
    }
}

impl<'a> CliSubCommand for AccountSubCommand<'a> {
    fn process(
        &mut self,
        matches: &ArgMatches,
        format: OutputFormat,
        color: bool,
        _debug: bool,
    ) -> Result<String, String> {
        match matches.subcommand() {
            ("list", _) => {
                let mut accounts = self
                    .key_store
                    .get_accounts()
                    .iter()
                    .map(|(address, filepath)| (address.clone(), filepath.clone()))
                    .collect::<Vec<(H160, PathBuf)>>();
                accounts.sort_by(|a, b| a.1.cmp(&b.1));
                let resp = accounts
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (lock_arg, _filepath))| {
                        let address_payload = AddressPayload::from_pubkey_hash(lock_arg.clone());
                        let lock_hash: H256 = Script::from(&address_payload)
                            .calc_script_hash()
                            .unpack();
                        serde_json::json!({
                            "#": idx,
                            "lock_arg": format!("{:#x}", lock_arg),
                            "lock_hash": format!("{:#x}", lock_hash),
                            "address": {
                                "mainnet": Address::new(NetworkType::Mainnet, address_payload.clone()).to_string(),
                                "testnet": Address::new(NetworkType::Testnet, address_payload.clone()).to_string(),
                            },
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!(resp).render(format, color))
            }
            ("new", _) => {
                println!("Your new account is locked with a password. Please give a password. Do not forget this password.");

                let pass = read_password(true, None)?;
                let lock_arg = self
                    .key_store
                    .new_account(pass.as_bytes())
                    .map_err(|err| err.to_string())?;
                let address_payload = AddressPayload::from_pubkey_hash(lock_arg.clone());
                let lock_hash: H256 = Script::from(&address_payload).calc_script_hash().unpack();
                let resp = serde_json::json!({
                    "lock_arg": format!("{:#x}", lock_arg),
                    "lock_hash": format!("{:#x}", lock_hash),
                    "address": {
                        "mainnet": Address::new(NetworkType::Mainnet, address_payload.clone()).to_string(),
                        "testnet": Address::new(NetworkType::Testnet, address_payload.clone()).to_string(),
                    },
                });
                Ok(resp.render(format, color))
            }
            ("import", Some(m)) => {
                let secp_key: Option<PrivkeyWrapper> =
                    PrivkeyPathParser.from_matches_opt(m, "privkey-path", false)?;
                let password = read_password(true, None)?;
                let lock_arg = if let Some(secp_key) = secp_key {
                    self.key_store
                        .import_secp_key(&secp_key, password.as_bytes())
                        .map_err(|err| err.to_string())?
                } else {
                    let master_privkey: MasterPrivKey =
                        ExtendedPrivkeyPathParser.from_matches(m, "extended-privkey-path")?;
                    let key = Key::new(master_privkey);
                    self.key_store
                        .import_key(&key, password.as_bytes())
                        .map_err(|err| err.to_string())?
                };
                let address_payload = AddressPayload::from_pubkey_hash(lock_arg.clone());
                let resp = serde_json::json!({
                    "lock_arg": format!("{:x}", lock_arg),
                    "address": {
                        "mainnet": Address::new(NetworkType::Mainnet, address_payload.clone()).to_string(),
                        "testnet": Address::new(NetworkType::Testnet, address_payload.clone()).to_string(),
                    },
                });
                Ok(resp.render(format, color))
            }
            ("unlock", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let keep: Duration = DurationParser.from_matches(m, "keep")?;
                let password = read_password(false, None)?;
                let lock_after = self
                    .key_store
                    .timed_unlock(&lock_arg, password.as_bytes(), keep)
                    .map(|timeout| timeout.to_string())
                    .map_err(|err| err.to_string())?;
                let resp = serde_json::json!({
                    "status": lock_after,
                });
                Ok(resp.render(format, color))
            }
            ("update", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let old_password = read_password(false, Some("Old password"))?;
                let new_passsword = read_password(true, Some("New password"))?;
                self.key_store
                    .update(&lock_arg, old_password.as_bytes(), new_passsword.as_bytes())
                    .map_err(|err| err.to_string())?;
                Ok("success".to_owned())
            }
            ("export", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let key_path = m.value_of("extended-privkey-path").unwrap();
                let password = read_password(false, None)?;

                if Path::new(key_path).exists() {
                    return Err(format!("File exists: {}", key_path));
                }
                let master_privkey = self
                    .key_store
                    .export_key(&lock_arg, password.as_bytes())
                    .map_err(|err| err.to_string())?;
                let bytes = master_privkey.to_bytes();
                let privkey = H256::from_slice(&bytes[0..32]).unwrap();
                let chain_code = H256::from_slice(&bytes[32..64]).unwrap();
                let mut file = fs::File::create(key_path).map_err(|err| err.to_string())?;
                file.write(format!("{:x}\n", privkey).as_bytes())
                    .map_err(|err| err.to_string())?;
                file.write(format!("{:x}", chain_code).as_bytes())
                    .map_err(|err| err.to_string())?;
                Ok(format!(
                    "Success exported account as extended privkey to: \"{}\", please use this file carefully",
                    key_path
                ))
            }
            ("bip44-addresses", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let from_receiving_index: u32 =
                    FromStrParser::<u32>::default().from_matches(m, "from-receiving-index")?;
                let receiving_length: u32 =
                    FromStrParser::<u32>::default().from_matches(m, "receiving-length")?;
                let from_change_index: u32 =
                    FromStrParser::<u32>::default().from_matches(m, "from-change-index")?;
                let change_length: u32 =
                    FromStrParser::<u32>::default().from_matches(m, "change-length")?;

                let password = read_password(false, None)?;
                let key_set = self
                    .key_store
                    .derived_key_set_by_index_with_password(
                        &lock_arg,
                        password.as_bytes(),
                        from_receiving_index,
                        receiving_length,
                        from_change_index,
                        change_length,
                    )
                    .map_err(|err| err.to_string())?;
                let get_addresses = |set: &[(DerivationPath, H160)]| {
                    set.iter()
                        .map(|(path, hash160)| {
                            let payload = AddressPayload::from_pubkey_hash(hash160.clone());
                            serde_json::json!({
                                "path": path.to_string(),
                                "address": Address::new(NetworkType::Mainnet, payload).to_string(),
                            })
                        })
                        .collect::<Vec<_>>()
                };
                let resp = serde_json::json!({
                    "receiving": get_addresses(&key_set.external),
                    "change": get_addresses(&key_set.change),
                });
                Ok(resp.render(format, color))
            }
            ("extended-address", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let path: Option<DerivationPath> =
                    FromStrParser::<DerivationPath>::new().from_matches_opt(m, "path", false)?;

                let password = read_password(false, None)?;
                let extended_pubkey = self
                    .key_store
                    .extended_pubkey_with_password(&lock_arg, path.as_ref(), password.as_bytes())
                    .map_err(|err| err.to_string())?;
                let address_payload = AddressPayload::from_pubkey(&extended_pubkey.public_key);
                let resp = serde_json::json!({
                    "lock_arg": format!("{:#x}", H160::from_slice(address_payload.args().as_ref()).unwrap()),
                    "address": {
                        "mainnet": Address::new(NetworkType::Mainnet, address_payload.clone()).to_string(),
                        "testnet": Address::new(NetworkType::Testnet, address_payload.clone()).to_string(),
                    },
                });
                Ok(resp.render(format, color))
            }
            _ => Err(matches.usage().to_owned()),
        }
    }
}
