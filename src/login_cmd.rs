//! `ccstats login` — store local provider credentials without printing secrets.

use std::io::{self, IsTerminal, Write};
use std::process::Command;

use crate::cli::LoginTarget;
use crate::credentials::{
    CursorAuth, clear_cursor_credentials, resolve_cursor_credentials, save_cursor_auth,
};

const CURSOR_API_KEY_URL: &str = "https://cursor.com/dashboard/api";
const CURSOR_SESSION_URL: &str = "https://cursor.com/dashboard/usage";

pub(crate) fn handle_login(target: &LoginTarget) {
    if let Err(error) = match target {
        LoginTarget::Cursor {
            api_key,
            session_token,
            check,
            clear,
            no_browser,
        } => handle_cursor_login(
            api_key.as_deref(),
            session_token.as_deref(),
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
    api_key: Option<&str>,
    session_token: Option<&str>,
    check: bool,
    clear: bool,
    no_browser: bool,
) -> Result<(), String> {
    if api_key.is_some() && session_token.is_some() {
        return Err("provide only one of --api-key or --session-token".to_string());
    }
    if clear && (api_key.is_some() || session_token.is_some()) {
        return Err("--clear cannot be combined with --api-key or --session-token".to_string());
    }

    if clear {
        clear_cursor_credentials().map_err(|error| error.to_string())?;
        println!("Removed stored Cursor credentials.");
        if check {
            print_cursor_check();
        }
        return Ok(());
    }

    if let Some(api_key) = api_key {
        save_cursor_auth(CursorAuth::ApiKey(parse_secret(api_key)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check();
        }
        return Ok(());
    }

    if let Some(session_token) = session_token {
        save_cursor_auth(CursorAuth::SessionToken(parse_secret(session_token)?))
            .map_err(|error| error.to_string())?;
        println!("Saved Cursor credentials locally.");
        if check {
            print_cursor_check();
        }
        return Ok(());
    }

    if check {
        print_cursor_check();
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err("non-interactive login requires --api-key or --session-token".to_string());
    }

    interactive_cursor_login(no_browser)
}

fn print_cursor_check() {
    match resolve_cursor_credentials() {
        Some(resolved) => {
            println!("cursor: configured ({})", resolved.origin.as_str());
        }
        None => println!("cursor: missing"),
    }
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

fn parse_secret(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Err("credential value must be non-empty".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn read_secret_line() -> Result<String, String> {
    let guard = TtyEchoGuard::hide();
    let mut line = String::new();
    let result = io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("failed to read credential: {error}"));
    let hidden = guard.echo_was_hidden();
    drop(guard);
    if hidden {
        println!();
    }
    result?;
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
    let _ = result;
}

struct TtyEchoGuard {
    restore: bool,
}

impl TtyEchoGuard {
    fn hide() -> Self {
        if !io::stdin().is_terminal() {
            return Self { restore: false };
        }
        Self {
            restore: set_tty_echo(false),
        }
    }

    fn echo_was_hidden(&self) -> bool {
        self.restore
    }
}

impl Drop for TtyEchoGuard {
    fn drop(&mut self) {
        if self.restore {
            let _ = set_tty_echo(true);
        }
    }
}

fn set_tty_echo(enable: bool) -> bool {
    #[cfg(unix)]
    {
        let arg = if enable { "echo" } else { "-echo" };
        Command::new("stty")
            .arg(arg)
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(unix))]
    {
        let _ = enable;
        false
    }
}
