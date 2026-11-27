use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::Command,
};
use token_manager::{
    credentials, prices,
    provider::{self, providers},
    state::{self, Account, Credential},
    status,
};

#[derive(Parser)]
#[command(
    version,
    about = "AI account balances and quotas. No inference routing."
)]
struct Cli {
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Permit HTTP only to literal loopback addresses for local fixtures"
    )]
    allow_local_http: bool,
    #[command(subcommand)]
    command: Action,
}
#[derive(Args)]
struct LoginOptions {
    #[arg(long, conflicts_with_all = ["env", "key_file"])]
    key_stdin: bool,
    #[arg(long, conflicts_with = "key_file", value_name = "VARIABLE")]
    env: Option<String>,
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,
    #[arg(long, help = "Open the provider's credential page")]
    open: bool,
}
#[derive(Subcommand)]
enum Action {
    /// Look up a model or compare a family using public catalog prices.
    Prices {
        query: String,
        #[arg(long, default_value = "openrouter")]
        provider: String,
        #[arg(long, value_enum, default_value = "output")]
        sort: prices::Sort,
        #[arg(long)]
        json: bool,
    },
    Providers {
        #[arg(long)]
        json: bool,
    },
    Add {
        provider: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        login: LoginOptions,
    },
    Login {
        account: String,
        #[command(flatten)]
        login: LoginOptions,
    },
    Status {
        account: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
        #[arg(long)]
        cached: bool,
    },
    Open {
        account: String,
        #[arg(default_value = "billing", value_parser = ["billing", "keys"])]
        page: String,
    },
}
fn open(url: &str) -> Result<()> {
    let result = Command::new("xdg-open")
        .arg(url)
        .status()
        .context("Could not open browser; use the displayed link")?;
    ensure!(result.success(), "browser opener failed");
    Ok(())
}
fn credential(
    options: LoginOptions,
    account: &Account,
    root: &std::path::Path,
) -> Result<Credential> {
    if let Some(name) = options.env {
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
            "invalid credential variable name"
        );
        let credential = Credential::Env(name);
        let mut candidate = account.clone();
        candidate.credential = credential.clone();
        credentials::resolve(&candidate)?;
        return Ok(credential);
    }
    if let Some(path) = options.key_file {
        ensure!(!path.is_symlink(), "credential file must not be a symlink");
        let credential =
            Credential::File(path.canonicalize().context("Credential file unavailable")?);
        let mut candidate = account.clone();
        candidate.credential = credential.clone();
        credentials::resolve(&candidate)?;
        return Ok(credential);
    }
    let secret = if options.key_stdin {
        let mut buffer = String::new();
        io::stdin().take(8193).read_to_string(&mut buffer)?;
        credentials::validate(buffer)?
    } else {
        ensure!(
            io::stdin().is_terminal(),
            "Use --env VARIABLE, --key-file PATH or --key-stdin in noninteractive sessions"
        );
        eprint!("API/account key (hidden): ");
        io::stderr().flush()?;
        credentials::validate(rpassword::read_password()?)?
    };
    let id = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}:{}:{}",
                root.display(),
                account.name,
                account.provider_fingerprint
            )
            .as_bytes()
        )
    );
    credentials::store(&id, &secret)?;
    Ok(Credential::Keyring(id))
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = cli
        .config_dir
        .or_else(|| std::env::var_os("TOKEN_MANAGER_CONFIG_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
                })
                .join("token-manager")
        });
    let root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    let definitions = providers(&root.join("providers"), cli.allow_local_http)?;
    let config_path = root.join("accounts.json");
    match cli.command {
        Action::Prices {
            query,
            provider,
            sort,
            json,
        } => {
            let definition = definitions
                .get(&provider)
                .context("Unknown pricing provider")?;
            let report = prices::fetch(definition, &query, sort, cli.allow_local_http)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", prices::table(&report));
            }
        }
        Action::Providers { json } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&definitions.values().collect::<Vec<_>>())?
                );
            } else {
                for provider in definitions.values() {
                    println!(
                        "{:<16} {}{}",
                        provider.id,
                        provider.name,
                        if provider.experimental {
                            " (experimental)"
                        } else {
                            ""
                        }
                    );
                }
                println!("\nStart with: token-manager add vercel");
            }
        }
        Action::Add {
            provider,
            name,
            login,
        } => {
            state::private_directory(&root)?;
            let _lock = state::EditLock::acquire(&root)?;
            let mut config = state::load(&config_path)?;
            let definition = definitions
                .get(&provider)
                .context("Unknown provider; run providers")?;
            let name = name.unwrap_or(provider.clone());
            ensure!(
                provider::identifier(&name),
                "account names use at most 64 letters, digits, - or _"
            );
            ensure!(config.accounts.len() < 64, "at most 64 accounts supported");
            ensure!(
                !config.accounts.iter().any(|a| a.name == name),
                "account exists; use login to reconnect or choose --name"
            );
            eprintln!(
                "{}\n{}\nKey page: {}",
                definition.name, definition.credential_help, definition.key_url
            );
            if login.open {
                open(&definition.key_url)?;
            }
            let mut account = Account {
                name,
                provider,
                provider_fingerprint: definition.fingerprint(),
                credential: Credential::Keyring(String::new()),
            };
            account.credential = if definition.unsupported_reason.is_some() {
                Credential::None
            } else {
                credential(login, &account, &root)?
            };
            let name = account.name.clone();
            config.accounts.push(account);
            state::atomic_write(&config_path, &serde_json::to_vec_pretty(&config)?)?;
            println!("Added {name}. Run `token-manager status {name}` to fetch account data.");
        }
        Action::Login { account, login } => {
            state::private_directory(&root)?;
            let _lock = state::EditLock::acquire(&root)?;
            let mut config = state::load(&config_path)?;
            let target = config
                .accounts
                .iter_mut()
                .find(|a| a.name == account)
                .context("Account not found; run add")?;
            let definition = definitions
                .get(&target.provider)
                .context("Provider unavailable")?;
            eprintln!(
                "{}\n{}\nAccount endpoint: {}\nKey page: {}",
                definition.name,
                definition.credential_help,
                definition.base_url,
                definition.key_url
            );
            if login.open {
                open(&definition.key_url)?;
            }
            ensure!(
                definition.unsupported_reason.is_none(),
                "This provider has no verified account API; use open to view billing"
            );
            let mut candidate = target.clone();
            candidate.provider_fingerprint = definition.fingerprint();
            candidate.credential = credential(login, &candidate, &root)?;
            *target = candidate;
            state::atomic_write(&config_path, &serde_json::to_vec_pretty(&config)?)?;
            let cache = root.join("cache").join(format!("{account}.json"));
            if cache.exists() && !cache.is_symlink() {
                fs::remove_file(cache)?;
            }
            println!("Reconnected {account}. Run status to validate account access.");
        }
        Action::Status {
            account,
            json,
            refresh,
            cached,
        } => {
            let config = state::load(&config_path)?;
            let accounts: Vec<_> = config
                .accounts
                .into_iter()
                .filter(|a| account.as_ref().is_none_or(|name| name == &a.name))
                .collect();
            if account.is_some() && accounts.is_empty() {
                bail!("account not found");
            }
            let rows = status::collect(
                &accounts,
                &definitions,
                &root.join("cache"),
                cached,
                refresh,
                cli.allow_local_http,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print!("{}", status::table(&rows));
            }
            if rows
                .iter()
                .any(|r| !matches!(r.state.as_str(), "ready" | "cached"))
            {
                std::process::exit(2);
            }
        }
        Action::Open { account, page } => {
            let config = state::load(&config_path)?;
            let account = config
                .accounts
                .iter()
                .find(|a| a.name == account)
                .context("Account not found")?;
            let definition = definitions
                .get(&account.provider)
                .context("Provider unavailable")?;
            let url = if page == "billing" {
                &definition.billing_url
            } else {
                &definition.key_url
            };
            println!("{url}");
            open(url)?;
        }
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
