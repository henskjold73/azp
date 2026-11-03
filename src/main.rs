#[cfg(not(unix))]
compile_error!("azp supports macOS and Linux only");

mod profile;

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use profile::{parse_azure_profile, validate_name, ProfileInfo, AZURE_PROFILE_FILE};

const PROFILES_DIR: &str = ".azure-profiles";
const CONFIG_ENV: &str = "AZURE_CONFIG_DIR";

/// Manage multiple Azure CLI logins, one AZURE_CONFIG_DIR per profile.
#[derive(Parser)]
#[command(name = "azp", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a profile and run `az login` in it
    Add {
        name: String,
        /// Extra arguments for `az login`, e.g. `-- --tenant contoso.com`
        #[arg(last = true)]
        az_args: Vec<String>,
    },
    /// List profiles with their user, tenant and default subscription
    #[command(alias = "ls")]
    List,
    /// Print `export AZURE_CONFIG_DIR=...` for the profile (use with eval)
    Use { name: String },
    /// Start $SHELL with AZURE_CONFIG_DIR set to the profile
    Shell { name: String },
    /// Re-run `az login` for an existing profile
    Relogin {
        name: String,
        /// Extra arguments for `az login`, e.g. `-- --tenant contoso.com`
        #[arg(last = true)]
        az_args: Vec<String>,
    },
    /// Delete a profile directory after confirmation
    #[command(alias = "rm")]
    Remove { name: String },
}

fn main() {
    if let Err(err) = run(Cli::parse()) {
        eprintln!("azp: {err:#}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Cmd::Add { name, az_args } => add(&name, &az_args),
        Cmd::List => list(),
        Cmd::Use { name } => {
            let dir = existing_profile_dir(&name)?;
            println!("export {CONFIG_ENV}={}", shell_quote(&dir.to_string_lossy()));
            Ok(())
        }
        Cmd::Shell { name } => shell(&name),
        Cmd::Relogin { name, az_args } => az_login(&existing_profile_dir(&name)?, &az_args),
        Cmd::Remove { name } => remove(&name),
    }
}

fn profiles_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(PROFILES_DIR))
}

fn profile_dir(name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    Ok(profiles_root()?.join(name))
}

fn existing_profile_dir(name: &str) -> Result<PathBuf> {
    let dir = profile_dir(name)?;
    if !dir.is_dir() {
        bail!("profile {name:?} does not exist (create it with `azp add {name}`)");
    }
    Ok(dir)
}

fn add(name: &str, az_args: &[String]) -> Result<()> {
    let dir = profile_dir(name)?;
    if dir.exists() {
        bail!("profile {name:?} already exists (use `azp relogin {name}` to sign in again)");
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    // Enforce 0700 regardless of umask, on both the root and the profile dir.
    for d in [dir.parent().expect("profile dir has a parent"), dir.as_path()] {
        fs::set_permissions(d, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("setting permissions on {}", d.display()))?;
    }
    eprintln!("Created {}", dir.display());
    az_login(&dir, az_args)
        .with_context(|| format!("login failed; retry with `azp relogin {name}`"))
}

fn az_login(dir: &Path, az_args: &[String]) -> Result<()> {
    let status = Command::new("az")
        .arg("login")
        .args(az_args)
        .env(CONFIG_ENV, dir)
        .status()
        .context("failed to run `az` (is the Azure CLI installed and on PATH?)")?;
    if !status.success() {
        bail!("`az login` exited with {status}");
    }
    Ok(())
}

fn shell(name: &str) -> Result<()> {
    let dir = existing_profile_dir(name)?;
    let sh = env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    eprintln!("azp: starting {} with profile {name:?} (exit to return)", sh.to_string_lossy());
    let status = Command::new(&sh)
        .env(CONFIG_ENV, &dir)
        .status()
        .with_context(|| format!("failed to start {}", sh.to_string_lossy()))?;
    process::exit(status.code().unwrap_or(1));
}

fn remove(name: &str) -> Result<()> {
    let dir = existing_profile_dir(name)?;
    if is_active(&dir) {
        eprintln!("warning: {name:?} is the active profile in this shell");
    }
    eprint!("Delete profile {name:?} at {}? [y/N] ", dir.display());
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        eprintln!("Aborted.");
        return Ok(());
    }
    fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    eprintln!("Removed {name:?}");
    Ok(())
}

fn list() -> Result<()> {
    let root = profiles_root()?;
    let mut names: Vec<String> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| validate_name(n).is_ok())
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", root.display())),
    };
    if names.is_empty() {
        eprintln!("No profiles yet. Create one with `azp add <name>`.");
        return Ok(());
    }
    names.sort();

    let mut rows: Vec<[String; 5]> = vec![["".into(), "PROFILE".into(), "USER".into(), "TENANT".into(), "SUBSCRIPTION".into()]];
    for name in names {
        let dir = root.join(&name);
        let marker = if is_active(&dir) { "*" } else { "" }.to_string();
        let row = match read_profile_info(&dir) {
            Ok(Some(ProfileInfo { user, tenant, subscription })) => [marker, name, user, tenant, subscription],
            Ok(None) => [marker, name, "not logged in".into(), String::new(), String::new()],
            Err(e) => [marker, name, format!("error: {e:#}"), String::new(), String::new()],
        };
        rows.push(row);
    }
    print_table(&rows);
    Ok(())
}

/// Reads only `azureProfile.json`; never touches token caches.
fn read_profile_info(dir: &Path) -> Result<Option<ProfileInfo>> {
    match fs::read_to_string(dir.join(AZURE_PROFILE_FILE)) {
        Ok(contents) => parse_azure_profile(&contents),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("reading azureProfile.json"),
    }
}

fn is_active(dir: &Path) -> bool {
    let Some(current) = env::var_os(CONFIG_ENV) else { return false };
    let current = PathBuf::from(current);
    match (fs::canonicalize(&current), fs::canonicalize(dir)) {
        (Ok(a), Ok(b)) => a == b,
        _ => current == dir,
    }
}

fn print_table<const N: usize>(rows: &[[String; N]]) {
    let mut widths = [0usize; N];
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        println!("{}", line.join("  ").trim_end());
    }
}

/// Quote for POSIX shells, leaving simple paths untouched.
fn shell_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+:@%,".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_plain_and_special() {
        assert_eq!(shell_quote("/Users/me/.azure-profiles/work"), "/Users/me/.azure-profiles/work");
        assert_eq!(shell_quote("/Users/my name/x"), "'/Users/my name/x'");
        assert_eq!(shell_quote("/a'b"), r"'/a'\''b'");
    }
}
