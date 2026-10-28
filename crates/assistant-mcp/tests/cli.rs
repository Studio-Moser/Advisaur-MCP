use assistant_memory::Memory;
use assistant_memory::thoughts::{ThoughtInput, get_thought, insert_thought};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static NEXT_HOME: AtomicU64 = AtomicU64::new(1);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("shelby-cli-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_advisaur-mcp"))
            .args(args)
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .env("PATH", "")
            .output()
            .expect("run advisaur-mcp")
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn version_is_stdout_only() {
    let output = TempHome::new().run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output), "advisaur-mcp v0.4.0\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn library_consumers_can_construct_the_server_and_http_router() {
    let memory = Arc::new(Mutex::new(Memory::open_in_memory().unwrap()));
    let _server = assistant_mcp::server::MemoryServer::new(memory.clone());
    let config = assistant_mcp::config::ServeConfig {
        db_path: ":memory:".into(),
        verbose: false,
        transport: assistant_mcp::config::Transport::Http,
        http_port: 3100,
        http_host: "127.0.0.1".into(),
        api_key: None,
    };
    let _router = assistant_mcp::http::router(memory, &config);
}

#[test]
fn setup_and_uninstall_validate_the_client_before_writing() {
    for args in [
        vec!["setup"],
        vec!["setup", "unknown"],
        vec!["uninstall"],
        vec!["uninstall", "unknown"],
    ] {
        let output = TempHome::new().run(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(stdout(&output).is_empty(), "{args:?}");
        assert!(
            stderr(&output).contains("Usage:"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn static_commands_keep_payloads_on_stdout_and_guidance_on_stderr() {
    let cases = [
        ("protocol", "# ShelbyMCP — Memory Protocol", false),
        ("forage", "# Shelby Forage — Memory Maintenance Skill", true),
        (
            "onboard",
            "# Shelby Onboard — First-Run Memory Seeding",
            true,
        ),
        ("migrate", "I'm moving my AI memory to a new system.", true),
    ];
    let home = TempHome::new();
    for (command, marker, has_guidance) in cases {
        let output = home.run(&[command]);
        assert!(output.status.success(), "{command}: {}", stderr(&output));
        let payload = stdout(&output);
        assert!(payload.contains(marker), "{command}");
        assert!(!payload.starts_with("---\nname:"), "{command}");
        assert_eq!(!stderr(&output).is_empty(), has_guidance, "{command}");
    }
}

#[test]
fn cursor_fallback_setup_and_uninstall_are_real_and_idempotent() {
    let home = TempHome::new();
    let path = home.0.join(".cursor/mcp.json");

    let first = home.run(&["setup", "cursor"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("configured"));
    assert!(stderr(&first).is_empty());
    assert_eq!(
        read_json(&path)["mcpServers"]["shelbymcp"]["command"],
        "npx"
    );

    let second = home.run(&["setup", "cursor"]);
    assert!(second.status.success());
    assert!(stdout(&second).contains("already configured"));

    let removed = home.run(&["uninstall", "cursor"]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(stdout(&removed).contains("removed"));
    assert!(read_json(&path)["mcpServers"].get("shelbymcp").is_none());

    let absent = home.run(&["uninstall", "cursor"]);
    assert!(absent.status.success());
    assert!(stdout(&absent).contains("already absent"));
}

#[test]
fn unavailable_cli_returns_manual_action_exit_two() {
    let output = TempHome::new().run(&["setup", "claude-code"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("Manual action required"));
}

#[test]
fn repair_projects_dry_run_then_apply_uses_the_rust_engine() {
    let home = TempHome::new();
    let shelby_dir = home.0.join(".shelbymcp");
    fs::create_dir_all(&shelby_dir).unwrap();
    fs::write(
        shelby_dir.join("projects.seed.json"),
        r#"{"projects":[{"slug":"shelby","displayName":"Shelby","sourceAliases":[]}],"topicClusters":{"memory":"shelby"}}"#,
    )
    .unwrap();
    let database_path = home.0.join("memory.db");
    let memory = Memory::open(&database_path).unwrap();
    let id = insert_thought(
        &memory.conn,
        &ThoughtInput {
            content: "Rust migration decision".into(),
            topics: Some(vec!["memory".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    drop(memory);

    let run = |apply: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_advisaur-mcp"));
        command
            .arg("repair-projects")
            .env("HOME", &home.0)
            .env("USERPROFILE", &home.0)
            .env("SHELBY_DB_PATH", &database_path)
            .env("PATH", "");
        if apply {
            command.arg("--apply");
        }
        command.output().unwrap()
    };

    let dry_run = run(false);
    assert!(dry_run.status.success(), "{}", stderr(&dry_run));
    assert!(stdout(&dry_run).is_empty());
    assert!(stderr(&dry_run).contains("DRY RUN"));
    let memory = Memory::open(&database_path).unwrap();
    assert!(
        get_thought(&memory.conn, &id)
            .unwrap()
            .unwrap()
            .project_id
            .is_none()
    );
    drop(memory);

    let apply = run(true);
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(stdout(&apply).is_empty());
    assert!(stderr(&apply).contains("APPLIED"));
    let memory = Memory::open(&database_path).unwrap();
    let thought = get_thought(&memory.conn, &id).unwrap().unwrap();
    assert_eq!(thought.project_identifier.as_deref(), Some("shelby"));
    assert_eq!(
        thought.metadata.unwrap()["repaired_by"],
        "integrity-project-v1"
    );
}
