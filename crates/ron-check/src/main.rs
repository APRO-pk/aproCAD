use std::fs;
use std::io::Read;
use std::process::exit;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let path: PathBuf = match args.next() {
        Some(p) => p.into(),
        None => {
            // Read from stdin
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).unwrap();
            validate(&buf, "<stdin>");
            return;
        }
    };

    let ron = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
            exit(1);
        }
    };
    validate(&ron, &path.to_string_lossy());
}

fn validate(ron: &str, label: &str) {
    // Strip leading comments for detection, but keep for parsing (ron handles comments)
    let clean = ron.replace(|c: char| c == '\r', "");
    let stripped = clean.trim_start();
    let first_non_comment = stripped
        .lines()
        .find(|l| !l.trim_start().starts_with("//"))
        .unwrap_or("");

    if first_non_comment.trim_start().starts_with("Vehicle(") {
        match ron::from_str::<apro_document::Vehicle>(&clean) {
            Ok(_) => {
                println!("✓ {}: valid Vehicle", label);
                exit(0);
            }
            Err(e) => {
                eprintln!("✗ {}: {}", label, e);
                exit(1);
            }
        }
    } else if first_non_comment.trim_start().starts_with("Component(") {
        match ron::from_str::<apro_document::Component>(&clean) {
            Ok(_) => {
                println!("✓ {}: valid Component", label);
                exit(0);
            }
            Err(e) => {
                eprintln!("✗ {}: {}", label, e);
                exit(1);
            }
        }
    } else {
        // Try Vehicle first, fall back to Component
        match ron::from_str::<apro_document::Vehicle>(&clean) {
            Ok(_) => {
                println!("✓ {}: valid Vehicle", label);
                exit(0);
            }
            Err(_) => match ron::from_str::<apro_document::Component>(&clean) {
                Ok(_) => {
                    println!("✓ {}: valid Component", label);
                    exit(0);
                }
                Err(e) => {
                    eprintln!("✗ {}: {}", label, e);
                    exit(1);
                }
            },
        }
    }
}
