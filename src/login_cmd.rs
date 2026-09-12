//! `ccstats login` — store local provider credentials without printing secrets.

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

use crate::cli::LoginTarget;
use crate::credentials::{
    API_KEY_ENV, SESSION_TOKEN_ENV, CursorAuth, clear_cursor_credentials,
    resolve_cursor_credentials, save_cursor_auth,
};

const CURSOR_API_KEY_URL: &str = "https://cursor.com/dashboard/api";
const CURSOR_SESSION_URL: &str = "https://cursor.com/dashboard/usage";

pub(crate) fn handle_login(target: &LoginTarget) {
    if let Err(error) = match target {
        LoginTarget::Cursor {
            api_key,
            session_token,
            api_key_file,
            session_token_file,
            check,
            clear,
            no_browser,
        } => handle_cursor_login(
            *api_key,
            *session_token,
            api_key_file.as_deref(),
            session_token_file.as_deref(),
            *check,
            *clear,
            *no_browser,
        ),
    } {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn handle_cursor_login(
    api_key: bool,
    session_token: bool,
    api_key_file: Option<&Path>,
    session_token_file: Option<&Path>,
    check: bool,
    clear: bool,
    no_browser: bool,
) -> Result<(), String> {
    let selected = [
        ("--api-key", api_key),
        ("--session-token", session_token),
        ("--api-key-file", api_key_file.is_some()),
        ("--session-token-file", session_token_file.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect::<Vec<_>>();

    if selected.len() > 1 {
        return Err(
            "provide only one of --api-key, --session-token, --api-key-file, or --session-token-file"
                .to_string(),
        );
    }
    if clear && !selected.is_empty() {
        return Err(
            "--clear cannot be combined with --api-key, --session-token, or --*-file".to_string(),
        );
    }

    if clear {
        clear_cursor_credentials().map_err(|error| error.to_string())?;
        println!("Removed stored Cursor credentials.");
        print_cursor_check()?;
        return Ok(());
    }

    if api_key {
        save_cursor_auth(CursorAuth::ApiKey(secret_from_env(API_KEY_ENV)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check()?;
        }
        return Ok(());
    }

    if session_token {
        save_cursor_auth(CursorAuth::SessionToken(secret_from_env(SESSION_TOKEN_ENV)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check()?;
        }
        return Ok(());
    }

    if let Some(path) = api_key_file {
        save_cursor_auth(CursorAuth::ApiKey(secret_from_file(path)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check()?;
        }
        return Ok(());
    }

    if let Some(path) = session_token_file {
        save_cursor_auth(CursorAuth::SessionToken(secret_from_file(path)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check()?;
        }
        return Ok(());
    }

    if check {
        print_cursor_check()?;
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(format!(
            "non-interactive login requires --api-key (from {API_KEY_ENV}), --session-token (from {SESSION_TOKEN_ENV}), --api-key-file, or --session-token-file"
        ));
    }

    interactive_cursor_login(no_browser)
}

fn print_cursor_check() -> Result<(), String> {
    match resolve_cursor_credentials().map_err(|error| error.to_string())? {
        Some(resolved) => {
            println!("cursor: configured ({})", resolved.origin.as_str());
        }
        None => println!("cursor: missing"),
    }
    Ok(())
}

fn interactive_cursor_login(no_browser: bool) -> Result<(), String> {
    println!(
        "ccstats does not log you into Cursor and does not send credentials to third parties."
    );
    println!("Credentials stay on this machine in credentials.toml.");
    println!();
    println!("1) Enterprise API key");
    println!("   {CURSOR_API_KEY_URL}");
    println!("2) Personal dashboard session token (WorkosCursorSessionToken)");
    println!("   {CURSOR_SESSION_URL}");
    println!();
    print!("Select 1 or 2: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("failed to write prompt: {error}"))?;

    let mut choice = String::new();
    io::stdin()
        .read_line(&mut choice)
        .map_err(|error| format!("failed to read selection: {error}"))?;
    let auth = match choice.trim() {
        "1" => {
            if !no_browser {
                open_url(CURSOR_API_KEY_URL);
            }
            print!("Paste the API key and press Enter: ");
            io::stdout()
                .flush()
                .map_err(|error| format!("failed to write prompt: {error}"))?;
            CursorAuth::ApiKey(read_secret_line()?)
        }
        "2" => {
            if !no_browser {
                open_url(CURSOR_SESSION_URL);
            }
            print!("Paste the session token and press Enter: ");
            io::stdout()
                .flush()
                .map_err(|error| format!("failed to write prompt: {error}"))?;
            CursorAuth::SessionToken(read_secret_line()?)
        }
        _ => return Err("select 1 for API key or 2 for session token".to_string()),
    };

    save_cursor_auth(auth).map_err(|error| error.to_string())?;
    println!("Saved Cursor credentials locally.");
    Ok(())
}

fn secret_from_env(var: &str) -> Result<String, String> {
    match env::var(var) {
        Ok(value) => parse_secret(&value),
        Err(env::VarError::NotPresent) => Err(format!("{var} is not set")),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{var} must be valid UTF-8")),
    }
}

fn secret_from_file(path: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_secret(&raw)
}

fn parse_secret(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Err("credential value must be non-empty".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn read_secret_line() -> Result<String, String> {
    let line = rpassword::read_password()
        .map_err(|error| format!("failed to read hidden credential: {error}"))?;
    parse_secret(&line)
}

fn open_url(url: &str) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-open").arg(url).status()
    } else if cfg!(windows) {
        Command::new("cmd").args(["/c", "start", "", url]).status()
    } else {
        return;
    };
    if !result.is_ok_and(|status| status.success()) {
        eprintln!("Could not open the browser; open {url} manually.");
    }
}
