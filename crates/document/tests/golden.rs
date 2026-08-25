use std::fs;
use std::path::Path;

/// Load and parse every .ron file in tests/golden/ to verify it round-trips
/// through the RON parser. This is both a regression suite and a corpus of
/// parser-verified examples for AI reference.
#[test]
fn golden_files_parse() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let mut count = 0;
    let mut errors = Vec::new();

    for entry in fs::read_dir(&dir).expect("golden dir") {
        let entry = entry.expect("entry");
        if entry.file_name().to_string_lossy().ends_with(".ron") {
            let path = entry.path();
            let ron = fs::read_to_string(&path).expect("read file");
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            // Strip leading comments to detect root type
            let stripped = ron.lines()
                .find(|l| !l.trim_start().starts_with("//"))
                .unwrap_or("")
                .trim_start();

            let result = if stripped.starts_with("Vehicle(") {
                ron::from_str::<apro_document::Vehicle>(&ron)
                    .map(|_| format!("Vehicle"))
                    .map_err(|e| e.to_string())
            } else {
                ron::from_str::<apro_document::Component>(&ron)
                    .map(|_| format!("Component"))
                    .map_err(|e| e.to_string())
            };

            match result {
                Ok(kind) => {
                    count += 1;
                    eprintln!("  ✓ {} -> {}", name, kind);
                }
                Err(msg) => {
                    errors.push(format!("  ✗ {}: {}", name, msg));
                }
            }
        }
    }

    for err in &errors {
        eprintln!("{}", err);
    }
    assert!(errors.is_empty(), "{}/{} golden files failed", errors.len(), count + errors.len());
    assert!(count > 0, "no golden files found");
    eprintln!("  All {} golden files parse OK", count);
}
