use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::ExitCode;
use std::time::Instant;

use assistant_memory::brief::BRIEF_POLICY_VERSION;
use assistant_memory::migrations::CURRENT_SCHEMA_VERSION;
use assistant_memory_eval::comparison::{compare, load_policy};
use assistant_memory_eval::contract::run_contract_suite;
use assistant_memory_eval::fetch::fetch_dataset;
use assistant_memory_eval::longmemeval::{load_manifest, run_longmemeval_suite};
use assistant_memory_eval::manifest::{DatasetProvenance, ResultManifest};
use assistant_memory_eval::report::{render_comparison_report, render_run_report};
use assistant_memory_eval::runner::{RunMetadata, build_result_manifest};
use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};

const CONTRACT_FIXTURE: &str = include_str!("../../../tests/fixtures/Contract Corpus-v1.json");
const PUBLIC_MANIFEST: &str = include_str!("../../../tests/fixtures/LongMemEval PR-v1.json");
const DEFAULT_POLICY: &str = include_str!("../../../tests/fixtures/Memory Eval Policy-v1.json");
const USAGE: &str = "Shelby deterministic memory evaluation\n\n\
Usage:\n\
  assistant-memory-eval fetch longmemeval-pr --cache PATH\n\
  assistant-memory-eval run --suite pr --dataset PATH --output DIR [--policy PATH] [--code-sha SHA]\n\
  assistant-memory-eval compare --base PATH --candidate PATH --candidate-repeat PATH --output DIR [--policy PATH]\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match execute(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("assistant-memory-eval: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(args: &[String]) -> Result<bool, Box<dyn Error>> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h") => {
            print!("{USAGE}");
            Ok(true)
        }
        Some("fetch") => fetch_command(&args[1..]),
        Some("run") => run_command(&args[1..]),
        Some("compare") => compare_command(&args[1..]),
        Some(command) => {
            Err(input_error(format!("unknown command `{command}`; see --help")).into())
        }
    }
}

fn fetch_command(args: &[String]) -> Result<bool, Box<dyn Error>> {
    if args.first().map(String::as_str) != Some("longmemeval-pr") {
        return Err(input_error("fetch requires the suite `longmemeval-pr`").into());
    }
    let options = parse_options(&args[1..], &["--cache"])?;
    let cache = required_path(&options, "--cache")?;
    let manifest = load_manifest(PUBLIC_MANIFEST)?;
    let path = fetch_dataset(&manifest, &cache)?;
    println!("verified dataset cache: {}", path.display());
    Ok(true)
}

fn run_command(args: &[String]) -> Result<bool, Box<dyn Error>> {
    let options = parse_options(
        args,
        &["--suite", "--dataset", "--output", "--policy", "--code-sha"],
    )?;
    if options.get("--suite").map(String::as_str) != Some("pr") {
        return Err(input_error("run requires `--suite pr`").into());
    }
    let dataset_path = required_path(&options, "--dataset")?;
    let output_path = required_path(&options, "--output")?;
    let policy = load_policy_text(options.get("--policy"))?;
    let started = Instant::now();
    let contract = run_contract_suite(CONTRACT_FIXTURE)?;
    let public_manifest = load_manifest(PUBLIC_MANIFEST)?;
    let public = run_longmemeval_suite(&public_manifest, &dataset_path)?;

    let provenance = vec![
        DatasetProvenance {
            name: "Shelby contract corpus".into(),
            source: "repository:tests/fixtures/Contract Corpus-v1.json".into(),
            revision: contract.suite_version.clone(),
            sha256: sha256(CONTRACT_FIXTURE.as_bytes()),
            license: "MIT".into(),
            license_url: None,
            selection_method: Some(
                "Repository-owned exact contracts and curated hard-confuser slices".into(),
            ),
            manifest_sha256: sha256(CONTRACT_FIXTURE.as_bytes()),
            selected_ids: contract.cases.iter().map(|case| case.id.clone()).collect(),
        },
        DatasetProvenance {
            name: public_manifest.dataset.name.clone(),
            source: public_manifest.dataset.source.clone(),
            revision: public_manifest.dataset.revision.clone(),
            sha256: public_manifest.dataset.sha256.clone(),
            license: public_manifest.dataset.license.clone(),
            license_url: Some(public_manifest.dataset.license_url.clone()),
            selection_method: Some(public_manifest.selection_method.clone()),
            manifest_sha256: sha256(PUBLIC_MANIFEST.as_bytes()),
            selected_ids: public_manifest
                .cases
                .iter()
                .map(|case| case.question_id.clone())
                .collect(),
        },
    ];
    let configuration = BTreeMap::from([
        ("evaluator_version".into(), env!("CARGO_PKG_VERSION").into()),
        ("rustc_version".into(), rustc_version()),
        ("metric_version".into(), "retrieval-v1".into()),
        (
            "contract_manifest_sha256".into(),
            sha256(CONTRACT_FIXTURE.as_bytes()),
        ),
        (
            "public_manifest_sha256".into(),
            sha256(PUBLIC_MANIFEST.as_bytes()),
        ),
        (
            "gate_policy_sha256".into(),
            sha256(policy_text(options.get("--policy"))?.as_bytes()),
        ),
        (
            "memory_schema_version".into(),
            CURRENT_SCHEMA_VERSION.to_string(),
        ),
        (
            "brief_policy_version".into(),
            BRIEF_POLICY_VERSION.to_string(),
        ),
        (
            "category_floors".into(),
            serde_json::to_string(&policy.category_floors)?,
        ),
        (
            "max_total_estimated_tokens".into(),
            policy.resources.max_total_estimated_tokens.to_string(),
        ),
        (
            "max_total_serialized_bytes".into(),
            policy.resources.max_total_serialized_bytes.to_string(),
        ),
        (
            "max_case_estimated_tokens".into(),
            policy.resources.max_case_estimated_tokens.to_string(),
        ),
        (
            "max_case_serialized_bytes".into(),
            policy.resources.max_case_serialized_bytes.to_string(),
        ),
        ("public_retrieval_handler".into(), "search_thoughts".into()),
        ("public_retrieval_limit".into(), "10".into()),
        ("precision_recall_cutoff".into(), "5".into()),
        ("ndcg_mrr_cutoff".into(), "10".into()),
    ]);
    let manifest = build_result_manifest(
        contract,
        public,
        provenance,
        RunMetadata {
            code_sha: options
                .get("--code-sha")
                .cloned()
                .or_else(|| std::env::var("GITHUB_SHA").ok())
                .unwrap_or_else(|| "unknown".into()),
            policy_version: policy.policy_version,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            target: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            configuration,
        },
    )?;
    write_run_artifacts(&output_path, &manifest)?;
    println!("wrote evaluation artifacts: {}", output_path.display());
    Ok(true)
}

fn compare_command(args: &[String]) -> Result<bool, Box<dyn Error>> {
    let options = parse_options(
        args,
        &[
            "--base",
            "--candidate",
            "--candidate-repeat",
            "--policy",
            "--output",
        ],
    )?;
    let base: ResultManifest = read_json(&required_path(&options, "--base")?)?;
    let candidate: ResultManifest = read_json(&required_path(&options, "--candidate")?)?;
    let candidate_repeat: ResultManifest =
        read_json(&required_path(&options, "--candidate-repeat")?)?;
    let output_path = required_path(&options, "--output")?;
    let policy = load_policy_text(options.get("--policy"))?;
    let comparison = compare(&base, &candidate, &candidate_repeat, &policy)?;

    fs::create_dir_all(&output_path)?;
    write_pretty_json(&output_path.join("comparison.json"), &comparison)?;
    fs::write(
        output_path.join("comparison.md"),
        render_comparison_report(&comparison),
    )?;
    println!("wrote comparison artifacts: {}", output_path.display());
    Ok(comparison.passed)
}

fn parse_options(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>, io::Error> {
    let mut parsed = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = &args[index];
        if !allowed.contains(&name.as_str()) {
            return Err(input_error(format!("unknown option `{name}`")));
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| input_error(format!("missing value for `{name}`")))?;
        if parsed.insert(name.clone(), value.clone()).is_some() {
            return Err(input_error(format!("duplicate option `{name}`")));
        }
        index += 2;
    }
    Ok(parsed)
}

fn required_path(options: &BTreeMap<String, String>, name: &str) -> Result<PathBuf, io::Error> {
    options
        .get(name)
        .map(PathBuf::from)
        .ok_or_else(|| input_error(format!("missing required option `{name}`")))
}

fn load_policy_text(
    path: Option<&String>,
) -> Result<assistant_memory_eval::comparison::GatePolicy, Box<dyn Error>> {
    let text = policy_text(path)?;
    Ok(load_policy(&text)?)
}

fn policy_text(path: Option<&String>) -> Result<String, io::Error> {
    match path {
        Some(path) => fs::read_to_string(path),
        None => Ok(DEFAULT_POLICY.into()),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_run_artifacts(path: &Path, manifest: &ResultManifest) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path)?;
    write_pretty_json(&path.join("results.json"), manifest)?;
    fs::write(path.join("report.md"), render_run_report(manifest)?)?;
    Ok(())
}

fn write_pretty_json(path: &Path, value: &impl serde::Serialize) -> Result<(), Box<dyn Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn input_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
