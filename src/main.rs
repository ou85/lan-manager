use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use homelab_manager::{
    auth,
    model::{Credentials, SCHEMA_VERSION, Snapshot},
    server::{self, AppState},
    storage::Store,
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "homelab",
    version,
    about = "Home Lab Manager: one binary, your data."
)]
struct Cli {
    /// Data directory (default: data/ next to this executable)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create the database and set the administrator password interactively
    Init {
        #[arg(long, default_value = "admin")]
        username: String,
    },
    /// Run the web server
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        /// Set the Secure session cookie flag (only behind HTTPS)
        #[arg(long)]
        secure_cookie: bool,
    },
    /// Change the password; stop the server first
    Passwd,
    /// Write a consistent standalone redb backup; stop the server first
    Backup {
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore a backup; stop the server first (restores credentials too)
    Restore {
        #[arg(long = "from")]
        source: PathBuf,
        #[arg(long)]
        force: bool,
    },
}
fn protect(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}
fn password() -> Result<String> {
    let p = rpassword::prompt_password("New password (at least 6 characters): ")?;
    let again = rpassword::prompt_password("Repeat password: ")?;
    if p != again {
        bail!("Passwords do not match");
    }
    auth::hash_password(&p)
}
fn backup(store: &Store, path: &Path) -> Result<()> {
    let snapshot = store.read()?;
    let result = Store::create(path, &snapshot)?;
    protect(path, 0o600)?;
    drop(result);
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let dir = match cli.data_dir {
        Some(d) => d,
        None => std::env::current_exe()?
            .parent()
            .context("No executable directory")?
            .join("data"),
    };
    let file = dir.join("homelab.redb");
    match cli.command {
        Command::Init { username } => {
            if file.exists() {
                bail!("Already initialized. Use 'homelab passwd' to change the password.");
            }
            if username.trim().is_empty() || username.len() > 100 {
                bail!("Username must contain 1–100 bytes");
            }
            let hash = password()?;
            std::fs::create_dir_all(&dir)
                .context("Cannot create data directory. Specify a writable --data-dir.")?;
            protect(&dir, 0o700)?;
            let store = Store::create(
                &file,
                &Snapshot {
                    schema_version: SCHEMA_VERSION,
                    credentials: Credentials {
                        username,
                        password_hash: hash,
                    },
                    devices: vec![],
                    subnets: vec![],
                },
            )?;
            protect(&file, 0o600)?;
            drop(store);
            println!("Initialized {}. Start with: homelab serve", file.display());
        }
        Command::Passwd => {
            let store = Store::open(&file)?;
            let hash = password()?;
            store.update(|s| {
                s.credentials.password_hash = hash;
                Ok(())
            })?;
            println!("Password changed. Restart the server. Previous sessions are invalid.");
        }
        Command::Backup { output } => {
            let store = Store::open(&file)?;
            backup(&store, &output)?;
            println!(
                "Backup saved to {}. It includes the password hash; keep it private.",
                output.display()
            );
        }
        Command::Restore { source, force } => {
            let source_store = Store::open(&source)?;
            let snapshot = source_store.read()?;
            if file.exists() {
                if !force {
                    bail!(
                        "Destination exists. Create a backup first, then use --force to replace its contents."
                    );
                }
                let store = Store::open(&file)?;
                store.replace(&snapshot)?;
            } else {
                std::fs::create_dir_all(&dir)?;
                protect(&dir, 0o700)?;
                let store = Store::create(&file, &snapshot)?;
                drop(store);
            }
            protect(&file, 0o600)?;
            println!("Restore complete. The backup's administrator password is now active.");
        }
        Command::Serve {
            listen,
            secure_cookie,
        } => {
            let store = Store::open(&file)?;
            let listener = tokio::net::TcpListener::bind(listen).await?;
            tracing::info!("Home Lab Manager listening on http://{listen}");
            axum::serve(listener, server::app(AppState::new(store, secure_cookie)))
                .with_graceful_shutdown(shutdown())
                .await?;
        }
    }
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
