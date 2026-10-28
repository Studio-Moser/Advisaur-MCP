//! CLI flags and environment, preserving the pre-0.4 CLI contract so existing
//! launch configs (`--db`, `--transport http`, `PORT`, `HOST`, and the variables in
//! `DB_PATH_ENV`, `TRANSPORT_ENV` and `API_KEY_ENV`, under either name) keep working.
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServeConfig {
    pub db_path: String,
    pub verbose: bool,
    pub transport: Transport,
    pub http_port: u16,
    pub http_host: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cli {
    Serve(ServeConfig),
    Version,
    Help,
    Setup {
        client: Option<String>,
        forage: bool,
        onboard: bool,
    },
    Uninstall {
        client: Option<String>,
    },
    Protocol,
    Forage,
    Onboard,
    Migrate,
    RepairProjects {
        apply: bool,
    },
}

/// The server's environment variables (ADR-0012 D4), each with the name it had before the
/// rename, which is still read for one release, with a warning.
pub const DB_PATH_ENV: (&str, &str) = (
    concat!(assistant_memory::product_env!(), "_DB_PATH"),
    "SHELBY_DB_PATH",
);
pub const TRANSPORT_ENV: (&str, &str) = (
    concat!(assistant_memory::product_env!(), "_TRANSPORT"),
    "SHELBY_TRANSPORT",
);
pub const API_KEY_ENV: (&str, &str) = (
    concat!(assistant_memory::product_env!(), "_API_KEY"),
    "SHELBY_API_KEY",
);

/// A variable under its current name, or its old one, with a warning. Empty counts as unset, so
/// a blank placeholder under the new name never hides a value under the old one.
fn env_or_legacy(
    env: &dyn Fn(&str) -> Option<String>,
    (name, legacy): (&str, &str),
) -> Option<String> {
    let set = |key: &str| env(key).filter(|value| !value.is_empty());
    set(name).or_else(|| {
        let value = set(legacy)?;
        eprintln!("[WARN] {legacy} is deprecated; use {name}");
        Some(value)
    })
}

/// Whether the database path comes from a flag or a variable rather than the default.
pub fn db_path_given(argv: &[String]) -> bool {
    argv.iter().any(|arg| arg == "--db")
        || [DB_PATH_ENV.0, DB_PATH_ENV.1]
            .iter()
            .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

pub fn env_var(names: (&str, &str)) -> Option<String> {
    env_or_legacy(&|k| std::env::var(k).ok(), names)
}

pub fn help() -> String {
    let bin = assistant_memory::brand::SERVER_NAME;
    let display = assistant_memory::brand::DISPLAY_NAME;
    let dir = assistant_memory::brand::DATA_DIR_NAME;
    let (db, transport, api_key) = (DB_PATH_ENV.0, TRANSPORT_ENV.0, API_KEY_ENV.0);
    format!(
        "{bin} — Knowledge-graph memory for AI tools

Usage:
  {bin}                   Start the MCP server (stdio)
  {bin} --transport http  Start the streamable HTTP server (default 0.0.0.0:3100/mcp)
  {bin} setup <client>     Configure a client fallback
  {bin} uninstall <client> Remove the {display} client entry
  {bin} protocol           Print the Memory Protocol
  {bin} forage             Print the Forage skill
  {bin} onboard            Print the onboarding skill
  {bin} migrate            Print the migration prompt
  {bin} repair-projects    Preview project identity repairs
  {bin} help              Show this help

Flags:
  --db <path>        Database path (default: ~/{dir}/memory.db; env {db})
  --transport <t>    stdio | http (env {transport})
  --port <n>         HTTP port (env PORT, default 3100)
  --host <h>         HTTP bind host (env HOST; default 0.0.0.0 for http)
  --verbose          Enable verbose logging
  --version          Print version

Command flags:
  setup --forage     Explain the package-native Forage integration
  setup --onboard    Print onboarding after setup
  repair-projects --apply
                     Apply high-confidence repairs and flag the rest

Environment:
  {api_key}   Bearer token required on /mcp when set
"
    )
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn default_db_dir() -> PathBuf {
    assistant_memory::brand::data_dir(&home_dir())
}

pub fn default_db_path() -> String {
    default_db_dir()
        .join("memory.db")
        .to_string_lossy()
        .into_owned()
}

pub fn resolve_db_path(p: &str) -> String {
    if p == ":memory:" {
        return p.to_string();
    }
    // A path inside the old folder, once it has moved, means the same file in the renamed one.
    let canonical = assistant_memory::projects::canonical_path(p);
    assistant_memory::brand::redirect_legacy_path(&home_dir(), &canonical)
        .to_string_lossy()
        .into_owned()
}

pub fn parse_args(argv: Vec<String>) -> Result<Cli, String> {
    parse_with_env(argv, &|k| std::env::var(k).ok())
}

pub fn parse_with_env(
    argv: Vec<String>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Cli, String> {
    match argv.first().map(String::as_str) {
        Some("help") | Some("--help") | Some("-h") => return Ok(Cli::Help),
        Some("setup") => {
            return Ok(Cli::Setup {
                client: argv.get(1).filter(|value| !value.starts_with('-')).cloned(),
                forage: argv.iter().any(|value| value == "--forage"),
                onboard: argv.iter().any(|value| value == "--onboard"),
            });
        }
        Some("uninstall") => {
            return Ok(Cli::Uninstall {
                client: argv.get(1).filter(|value| !value.starts_with('-')).cloned(),
            });
        }
        Some("protocol") => return Ok(Cli::Protocol),
        Some("forage") => return Ok(Cli::Forage),
        Some("onboard") => return Ok(Cli::Onboard),
        Some("migrate") => return Ok(Cli::Migrate),
        Some("repair-projects") => {
            return Ok(Cli::RepairProjects {
                apply: argv.iter().any(|value| value == "--apply"),
            });
        }
        Some(command) if !command.starts_with('-') => {
            return Err(format!("Unknown command: {command}"));
        }
        _ => {}
    }
    let env_transport = env_or_legacy(env, TRANSPORT_ENV);
    let mut cfg = ServeConfig {
        db_path: env_or_legacy(env, DB_PATH_ENV)
            .map(|p| resolve_db_path(&p))
            .unwrap_or_else(default_db_path),
        verbose: false,
        transport: if env_transport.as_deref() == Some("http") {
            Transport::Http
        } else {
            Transport::Stdio
        },
        http_port: env("PORT").and_then(|p| p.parse().ok()).unwrap_or(3100),
        http_host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
        api_key: env_or_legacy(env, API_KEY_ENV),
    };
    let mut explicit_host = false;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--db" => {
                i += 1;
                cfg.db_path = argv
                    .get(i)
                    .map(|p| resolve_db_path(p))
                    .unwrap_or_else(default_db_path);
            }
            "--verbose" => cfg.verbose = true,
            "--log-file" => i += 1, // accepted for compatibility; logs go to stderr
            "--transport" => {
                i += 1;
                cfg.transport = match argv.get(i).map(String::as_str) {
                    Some("stdio") => Transport::Stdio,
                    Some("http") => Transport::Http,
                    other => {
                        return Err(format!(
                            "Invalid transport {:?}. Use \"stdio\" or \"http\".",
                            other.unwrap_or("")
                        ));
                    }
                };
            }
            "--port" => {
                i += 1;
                cfg.http_port = argv.get(i).and_then(|p| p.parse().ok()).unwrap_or(3100);
            }
            "--host" => {
                i += 1;
                cfg.http_host = argv.get(i).cloned().unwrap_or_else(|| "127.0.0.1".into());
                explicit_host = true;
            }
            "--version" => return Ok(Cli::Version),
            _ => {}
        }
        i += 1;
    }
    if cfg.transport == Transport::Http && env("HOST").is_none() && !explicit_host {
        cfg.http_host = "0.0.0.0".into();
    }
    // A key set but empty is a mistake, never a choice to run without authentication.
    if cfg.transport == Transport::Http && cfg.api_key.is_none() {
        let (name, legacy) = API_KEY_ENV;
        if let Some(blank) = [name, legacy]
            .into_iter()
            .find(|key| env(key).is_some_and(|value| value.is_empty()))
        {
            return Err(format!(
                "{blank} is set but empty. Set {name} to a long random secret, or unset it to run without authentication."
            ));
        }
    }
    Ok(Cli::Serve(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_and_flags() {
        let Cli::Serve(c) = parse_with_env(vec![], &no_env).unwrap() else {
            panic!()
        };
        assert_eq!(c.db_path, default_db_path());
        assert_eq!(
            (c.transport, c.http_port, c.http_host.as_str()),
            (Transport::Stdio, 3100, "127.0.0.1")
        );
        let Cli::Serve(c) = parse_with_env(
            vec![
                "--transport".into(),
                "http".into(),
                "--port".into(),
                "4000".into(),
                "--db".into(),
                "/tmp/x.db".into(),
            ],
            &no_env,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            (
                c.transport,
                c.http_port,
                c.http_host.as_str(),
                c.db_path.as_str()
            ),
            (Transport::Http, 4000, "0.0.0.0", "/tmp/x.db")
        );
        assert!(
            parse_with_env(vec!["--transport".into(), "carrier-pigeon".into()], &no_env).is_err()
        );
        assert_eq!(
            parse_with_env(vec!["--version".into()], &no_env).unwrap(),
            Cli::Version
        );
        assert_eq!(
            parse_with_env(
                vec!["setup".into(), "cursor".into(), "--forage".into()],
                &no_env
            )
            .unwrap(),
            Cli::Setup {
                client: Some("cursor".into()),
                forage: true,
                onboard: false,
            }
        );
        assert_eq!(
            parse_with_env(vec!["uninstall".into(), "cursor".into()], &no_env).unwrap(),
            Cli::Uninstall {
                client: Some("cursor".into())
            }
        );
        assert_eq!(
            parse_with_env(vec!["protocol".into()], &no_env).unwrap(),
            Cli::Protocol
        );
        assert_eq!(
            parse_with_env(vec!["forage".into()], &no_env).unwrap(),
            Cli::Forage
        );
        assert_eq!(
            parse_with_env(vec!["onboard".into()], &no_env).unwrap(),
            Cli::Onboard
        );
        assert_eq!(
            parse_with_env(vec!["migrate".into()], &no_env).unwrap(),
            Cli::Migrate
        );
        assert_eq!(
            parse_with_env(vec!["repair-projects".into(), "--apply".into()], &no_env).unwrap(),
            Cli::RepairProjects { apply: true }
        );
    }

    #[test]
    fn env_provides_defaults_and_flags_override() {
        let env = |k: &str| match k {
            "ADVISAUR_TRANSPORT" => Some("http".to_string()),
            "HOST" => Some("10.0.0.5".to_string()),
            "SHELBY_API_KEY" => Some("k".to_string()), // the old name still reads
            _ => None,
        };
        let Cli::Serve(c) = parse_with_env(vec![], &env).unwrap() else {
            panic!()
        };
        assert_eq!(
            (c.transport, c.http_host.as_str(), c.api_key.as_deref()),
            (Transport::Http, "10.0.0.5", Some("k"))
        );
        let Cli::Serve(c) =
            parse_with_env(vec!["--host".into(), "127.0.0.1".into()], &env).unwrap()
        else {
            panic!()
        };
        assert_eq!(c.http_host, "127.0.0.1");
    }

    #[test]
    fn a_blank_new_variable_never_hides_the_old_one() {
        let env = |key: &str| match key {
            "ADVISAUR_API_KEY" => Some(String::new()),
            "SHELBY_API_KEY" => Some("secret".to_string()),
            _ => None,
        };
        let Cli::Serve(cfg) = parse_with_env(vec![], &env).unwrap() else {
            panic!("expected serve")
        };
        assert_eq!(cfg.api_key.as_deref(), Some("secret"));
        let blank = |key: &str| (key == "ADVISAUR_API_KEY").then(String::new);
        let Cli::Serve(cfg) = parse_with_env(vec![], &blank).unwrap() else {
            panic!("expected serve")
        };
        assert_eq!(cfg.api_key, None);
    }

    #[test]
    fn an_empty_api_key_stops_the_http_server() {
        let blank = |key: &str| match key {
            "ADVISAUR_API_KEY" => Some(String::new()),
            _ => None,
        };
        let error = parse_with_env(vec!["--transport".into(), "http".into()], &blank).unwrap_err();
        assert!(
            error.contains("ADVISAUR_API_KEY is set but empty"),
            "{error}"
        );
        // Stdio never uses the key, so it starts.
        assert!(parse_with_env(vec![], &blank).is_ok());
    }
}
