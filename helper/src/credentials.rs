use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use secret_service::blocking::SecretService;
use secret_service::EncryptionType;

pub const APP_ID: &str = "keepassxc-control";
pub const FINGERPRINT_PROMPT: &str = "Put your finger on the sensor";
pub const FPRINTD_VERIFY: &str = "/usr/bin/fprintd-verify";
const STDBUF: &str = "/usr/bin/stdbuf";

pub trait Authenticator {
    fn available(&self) -> bool;
    fn verify(&self, on_status: &mut dyn FnMut(&str), cancel: &AtomicBool) -> bool;
}

pub fn normalize_path(path: &str) -> String {
    let expanded = expand_user(Path::new(path));
    match expanded.canonicalize() {
        Ok(resolved) => resolved.to_string_lossy().into_owned(),
        Err(_) => expanded.to_string_lossy().into_owned(),
    }
}

pub fn paths_match(left: &str, right: &str) -> bool {
    left == right || normalize_path(left) == normalize_path(right)
}

pub fn expand_user(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

pub fn scrub_leftover_secrets() {
    thread::spawn(|| {
        let Ok(service) = SecretService::connect(EncryptionType::Dh) else {
            return;
        };
        let Ok(found) = service.search_items(HashMap::from([("application", APP_ID)])) else {
            return;
        };
        for item in found.unlocked.into_iter().chain(found.locked) {
            if item.is_locked().unwrap_or(false) {
                let _ = item.unlock();
            }
            let _ = item.delete();
        }
    });
}

pub struct ConstantAuthenticator {
    ok: bool,
    available: bool,
}

impl ConstantAuthenticator {
    pub fn new(ok: bool, available: bool) -> Self {
        Self { ok, available }
    }
}

impl Authenticator for ConstantAuthenticator {
    fn available(&self) -> bool {
        self.available
    }

    fn verify(&self, _on_status: &mut dyn FnMut(&str), _cancel: &AtomicBool) -> bool {
        self.ok
    }
}

pub fn fingerprint_status_message(line: &str) -> Option<&'static str> {
    let text = line.trim().to_ascii_lowercase();
    if text.contains("verify started") {
        return Some(FINGERPRINT_PROMPT);
    }
    if text.contains("verify-match") {
        return Some("Checking fingerprint");
    }
    if text.contains("verify-no-match")
        || text.contains("verify-retry")
        || text.contains("verify-swipe")
        || text.contains("verify-finger-not")
        || text.contains("verify-remove")
        || text.contains("verify-too-short")
        || text.contains("verify-disconnected")
        || text.contains("verify-unknown-error")
    {
        return Some("Try again");
    }
    None
}

pub struct FprintAuthenticator;

fn line_reports_match(line: &str) -> bool {
    line.to_ascii_lowercase().contains("verify-match")
}

impl FprintAuthenticator {
    fn verifier_present() -> bool {
        Path::new(FPRINTD_VERIFY).is_file()
    }

    fn command() -> Option<Command> {
        if !Self::verifier_present() {
            return None;
        }
        let mut command = if Path::new(STDBUF).is_file() {
            let mut command = Command::new(STDBUF);
            command.args(["-oL", "-eL", FPRINTD_VERIFY]);
            command
        } else {
            Command::new(FPRINTD_VERIFY)
        };
        command.env_remove("PATH");
        Some(command)
    }
}

impl Authenticator for FprintAuthenticator {
    fn available(&self) -> bool {
        Self::verifier_present()
    }

    fn verify(&self, on_status: &mut dyn FnMut(&str), cancel: &AtomicBool) -> bool {
        let Some(mut command) = Self::command() else {
            return false;
        };
        on_status(FINGERPRINT_PROMPT);

        let mut process = match command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(process) => process,
            Err(_) => return false,
        };

        let Some(stdout) = process.stdout.take() else {
            let _ = process.kill();
            return false;
        };

        let process = Arc::new(Mutex::new(process));
        let stop = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_secs(45);
        let watcher_process = process.clone();
        let watcher_stop = stop.clone();
        thread::scope(|scope| {
            scope.spawn(|| {
                while !watcher_stop.load(Ordering::SeqCst) {
                    if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline {
                        let _ = watcher_process.lock().expect("fprintd").kill();
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            });

            let mut matched = false;
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline {
                    let _ = process.lock().expect("fprintd").kill();
                    break;
                }
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(message) = fingerprint_status_message(&line) {
                            on_status(message);
                        }
                        if line_reports_match(&line) {
                            matched = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            stop.store(true, Ordering::SeqCst);
            let wait = process.lock().expect("fprintd").wait();
            if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline || wait.is_err() {
                on_status(FINGERPRINT_PROMPT);
                return false;
            }
            matched
        })
    }
}

pub fn authenticator_from_environment() -> Box<dyn Authenticator + Send> {
    #[cfg(debug_assertions)]
    {
        match std::env::var("KEEPASSXC_CONTROL_AUTH")
            .unwrap_or_else(|_| "fprintd".into())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "allow" | "yes" => return Box::new(ConstantAuthenticator::new(true, true)),
            "deny" | "off" | "none" => return Box::new(ConstantAuthenticator::new(false, false)),
            _ => {}
        }
    }
    Box::new(FprintAuthenticator)
}

#[cfg(test)]
mod tests {
    use super::{
        fingerprint_status_message, line_reports_match, paths_match,
        FPRINTD_VERIFY, STDBUF,
    };
    use std::path::Path;

    #[test]
    fn started_asks_for_a_touch() {
        assert_eq!(
            fingerprint_status_message("Verify started!"),
            Some("Put your finger on the sensor")
        );
    }

    #[test]
    fn match_is_checking() {
        assert_eq!(
            fingerprint_status_message("Verify result: verify-match (done)"),
            Some("Checking fingerprint")
        );
    }

    #[test]
    fn retry_and_mismatch_ask_to_try_again() {
        assert_eq!(
            fingerprint_status_message("Verify result: verify-retry-scan (not done)"),
            Some("Try again")
        );
        assert_eq!(
            fingerprint_status_message("Verify result: verify-no-match (done)"),
            Some("Try again")
        );
    }

    #[test]
    fn successful_exit_without_match_is_rejected() {
        assert!(line_reports_match("Verify result: verify-match (done)"));
        assert!(!line_reports_match("Verify result: verify-no-match (done)"));
    }

    #[test]
    fn verifier_command_uses_absolute_paths() {
        let Some(command) = super::FprintAuthenticator::command() else {
            assert!(!Path::new(FPRINTD_VERIFY).is_file());
            return;
        };
        if Path::new(STDBUF).is_file() {
            assert_eq!(command.get_program(), STDBUF);
            let args: Vec<_> = command.get_args().collect();
            assert_eq!(args, ["-oL", "-eL", FPRINTD_VERIFY]);
        } else {
            assert_eq!(command.get_program(), FPRINTD_VERIFY);
            assert_eq!(command.get_args().len(), 0);
        }
    }

    #[test]
    fn path_aliases_match_when_the_file_exists() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/synthetic.kdbx");
        let canonical = path.canonicalize().unwrap();
        let alias = canonical
            .parent()
            .unwrap()
            .join(".")
            .join(canonical.file_name().unwrap());
        assert!(paths_match(
            canonical.to_str().unwrap(),
            alias.to_str().unwrap()
        ));
        assert!(!paths_match(
            canonical.to_str().unwrap(),
            "/missing/keepassxc-control.kdbx"
        ));
    }
}
