//! The product name is held once (ADR-0012 D1): the integrations crate's copy agrees with it,
//! and no other source names the product itself.

use std::path::Path;

#[test]
fn the_integrations_copy_agrees_with_the_brand() {
    assert_eq!(
        assistant_integrations::SERVER_KEY,
        assistant_memory::brand::SERVER_NAME
    );
    assert_eq!(
        assistant_integrations::DISPLAY_NAME,
        assistant_memory::brand::DISPLAY_NAME
    );
}

fn rust_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn no_source_names_the_product_outside_the_brand() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let allowed = [
        "assistant-memory/src/brand.rs",
        "assistant-integrations/src/config.rs",
    ];
    let mut files = Vec::new();
    for krate in std::fs::read_dir(crates).unwrap() {
        let src = krate.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut stray = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if allowed.contains(&relative.as_str())
            || relative.ends_with("/tests.rs")
            || relative.contains("/tests/")
        {
            continue;
        }
        for (number, line) in std::fs::read_to_string(&file).unwrap().lines().enumerate() {
            // A file's own test module comes last; its literals are test data.
            if line.trim() == "#[cfg(test)]" {
                break;
            }
            // Paths to assets are file names, which can't come from a constant.
            if line.contains("include_str!") {
                continue;
            }
            // A comment line may name the product; a URL's `//` in code is still code.
            if line.trim_start().starts_with("//") {
                continue;
            }
            let code = line;
            if ["Advisaur", "advisaur", "ADVISAUR"]
                .iter()
                .any(|name| code.contains(name))
            {
                stray.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "brand literals outside the brand module:\n{}",
        stray.join("\n")
    );
}
