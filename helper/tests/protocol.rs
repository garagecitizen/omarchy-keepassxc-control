use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const FIXTURE_PASSWORD: &str = "spike-only-password";

fn fixture_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/synthetic.kdbx")
        .to_string_lossy()
        .into_owned()
}

fn canonical_fixture_path() -> String {
    PathBuf::from(fixture_path())
        .canonicalize()
        .expect("fixture")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn subprocess_preserves_request_ids_across_multiple_requests() {
    let requests = [
        serde_json::json!({"id": "a", "op": "configure", "path": fixture_path()}),
        serde_json::json!({"id": "b", "op": "unlock", "password": FIXTURE_PASSWORD}),
        serde_json::json!({"id": "c", "op": "list"}),
        serde_json::json!({"id": "d", "op": "status"}),
        serde_json::json!({"id": "e", "op": "quit"}),
    ];
    let input = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect::<String>();

    let output = Command::new(env!("CARGO_BIN_EXE_keepassxc-control-helper"))
        .env("KEEPASSXC_CONTROL_AUTH", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut process| {
            process
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(input.as_bytes())?;
            process.wait_with_output()
        })
        .expect("helper process");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let responses: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect();
    let ids: Vec<_> = responses
        .iter()
        .map(|response| response["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b", "c", "d", "e"]);
    assert_eq!(
        responses[2]["result"]["entries"].as_array().unwrap().len(),
        36
    );
    assert_eq!(responses[3]["result"]["state"], "unlocked");
    assert_eq!(responses[3]["result"]["path"], canonical_fixture_path());
    assert!(!stdout.contains(FIXTURE_PASSWORD));
}
