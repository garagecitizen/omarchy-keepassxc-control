use std::env;
use std::path::PathBuf;
use std::process;

use keepassxc_control_helper::synthetic::{write_fixture, ENTRY_COUNT, FIXTURE_PASSWORD};

fn default_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/synthetic.kdbx")
}

fn main() {
    let mut force = false;
    let mut path = None;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--force" => force = true,
            "-h" | "--help" => {
                eprintln!("create-synthetic-db [--force] [path]");
                return;
            }
            flag if flag.starts_with('-') => {
                eprintln!("unknown argument: {flag}");
                process::exit(2);
            }
            value if path.is_some() => {
                eprintln!("unexpected extra path: {value}");
                process::exit(2);
            }
            value => path = Some(PathBuf::from(value)),
        }
    }

    let path = path.unwrap_or_else(default_path);
    if let Err(err) = write_fixture(&path, force) {
        eprintln!("{err}");
        process::exit(1);
    }
    println!("Created {ENTRY_COUNT} synthetic entries at {}", path.display());
    println!("Fixture password: {FIXTURE_PASSWORD}");
}
