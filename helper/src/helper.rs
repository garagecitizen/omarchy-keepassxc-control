use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use serde_json::{json, Map, Value};
use zeroize::Zeroizing;

use crate::clipboard;
use crate::credentials::{
    authenticator_from_environment, normalize_path, paths_match, scrub_leftover_secrets,
    Authenticator, FINGERPRINT_PROMPT,
};
use crate::vault::{Vault, VaultFieldError, VaultLockedError, VaultOpenError};

struct SessionSecret {
    path: String,
    password: Zeroizing<String>,
}

pub struct Helper {
    vault: Vault,
    clock: Box<dyn Fn() -> f64 + Send>,
    idle_timeout_seconds: f64,
    authenticator: Box<dyn Authenticator + Send>,
    database_path: Option<String>,
    last_used: Option<f64>,
    session_secret: Option<SessionSecret>,
    progress: Option<Arc<dyn Fn(Value) + Send + Sync>>,
    verify_cancel: Arc<AtomicBool>,
}

impl Helper {
    pub fn new() -> Self {
        scrub_leftover_secrets();
        let start = Instant::now();
        Self::with_parts(
            Box::new(move || start.elapsed().as_secs_f64()),
            180.0,
            authenticator_from_environment(),
        )
    }

    pub fn with_parts(
        clock: Box<dyn Fn() -> f64 + Send>,
        idle_timeout_seconds: f64,
        authenticator: Box<dyn Authenticator + Send>,
    ) -> Self {
        Self {
            vault: Vault::new(),
            clock,
            idle_timeout_seconds,
            authenticator,
            database_path: None,
            last_used: None,
            session_secret: None,
            progress: None,
            verify_cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_progress(&mut self, emit: Arc<dyn Fn(Value) + Send + Sync>) {
        self.progress = Some(emit);
    }

    pub fn handle(&mut self, request: Value) -> Value {
        let Some(object) = request.as_object() else {
            return error(
                Value::Null,
                "malformed_request",
                "Request must be an object",
            );
        };

        let request_id = object.get("id").cloned().unwrap_or(Value::Null);
        let Some(operation) = object.get("op").and_then(Value::as_str) else {
            return error(request_id, "malformed_request", "Request op is required");
        };

        match operation {
            "configure" => self.configure(request_id, object),
            "unlock" => self.unlock(request_id, object),
            "list" => self.list(request_id),
            "read_field" => self.read_field(request_id, object),
            "read_details" => self.read_details(request_id, object),
            "write_clipboard" => self.write_clipboard(request_id, object),
            "lock" => self.lock(request_id),
            "status" => self.status(request_id, object),
            "quit" => success(request_id, json!({"quitting": true})),
            _ => error(request_id, "unknown_operation", "Unknown operation"),
        }
    }

    pub fn run<R: BufRead + Send, W: Write + Send + 'static>(&mut self, input: R, output: W) -> W {
        let output = Arc::new(Mutex::new(output));
        {
            let writer = output.clone();
            self.set_progress(Arc::new(move |message| {
                write_response(&mut *writer.lock().expect("progress writer"), &message);
            }));
        }

        let (tx, rx) = mpsc::channel();
        let cancel = self.verify_cancel.clone();
        thread::scope(|scope| {
            scope.spawn(move || {
                for line in input.lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    match serde_json::from_str::<Value>(&line) {
                        Ok(request) => {
                            if matches!(
                                request.get("op").and_then(Value::as_str),
                                Some("lock" | "quit")
                            ) {
                                cancel.store(true, Ordering::SeqCst);
                            }
                            if tx.send(Ok(request)).is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            if tx.send(Err(())).is_err() {
                                break;
                            }
                        }
                    }
                }
            });

            for received in rx {
                match received {
                    Ok(request) => {
                        let quitting = request.get("op").and_then(Value::as_str) == Some("quit");
                        let response = self.handle(request);
                        write_response(&mut *output.lock().expect("writer"), &response);
                        self.verify_cancel.store(false, Ordering::SeqCst);
                        if quitting {
                            break;
                        }
                    }
                    Err(()) => {
                        write_response(
                            &mut *output.lock().expect("writer"),
                            &error(Value::Null, "malformed_request", "Request must be JSON"),
                        );
                    }
                }
            }
        });
        self.progress = None;
        self.drop_session_secret();
        self.vault.lock();
        Arc::try_unwrap(output)
            .unwrap_or_else(|_| panic!("progress writer"))
            .into_inner()
            .expect("progress writer")
    }

    fn configure(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        let Some(path) = request.get("path").and_then(Value::as_str).map(str::trim) else {
            return error(request_id, "invalid_request", "A database path is required");
        };
        if path.is_empty() {
            return error(request_id, "invalid_request", "A database path is required");
        }

        if let Err(response) = self.apply_idle_timeout(&request_id, request) {
            return response;
        }

        self.vault.lock();
        if !self.secret_matches_path(path) {
            self.drop_session_secret();
        }
        self.database_path = Some(path.to_string());
        self.last_used = None;
        success(request_id, json!({"configured": true}))
    }

    fn unlock(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        self.expire_if_needed();
        let path = request
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.database_path.clone());
        let Some(path) = path else {
            return error(request_id, "setup_required", "A database path is required");
        };

        if let Err(response) = self.apply_idle_timeout(&request_id, request) {
            return response;
        }

        let normalized = normalize_path(&path);
        if self.vault.is_unlocked()
            && (self.database_path.as_deref() == Some(path.as_str())
                || self.database_path.as_deref() == Some(normalized.as_str()))
        {
            self.touch();
            return success(
                request_id,
                json!({"unlocked": true, "secret_stored": self.has_secret(Some(&path))}),
            );
        }

        let supplied = request
            .get("password")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let typed = supplied.is_some();
        let password = match supplied {
            Some(password) => password,
            None => match self.password_from_session(&request_id, &path) {
                Ok(password) => password,
                Err(response) => return response,
            },
        };

        if let Err(failure) = self.vault.open(&path, &password) {
            if !typed && matches!(failure, VaultOpenError::PasswordRejected) {
                self.drop_session_secret();
            }
            return open_error(request_id, failure);
        }

        let resolved = self
            .vault
            .path()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or(normalized);
        self.retain_session_secret(resolved.clone(), &password);
        self.database_path = Some(resolved);
        self.touch();
        success(
            request_id,
            json!({"unlocked": true, "secret_stored": self.has_secret(self.database_path.as_deref())}),
        )
    }

    fn password_from_session(&self, request_id: &Value, path: &str) -> Result<String, Value> {
        if !self.has_secret(Some(path)) {
            return Err(error(
                request_id.clone(),
                "secret_missing",
                "No stored database password. Enter the KeePassXC password once.",
            ));
        }
        if !self.authenticator.available() {
            return Err(error(
                request_id.clone(),
                "auth_unavailable",
                "Fingerprint verification is not available",
            ));
        }
        self.emit_progress(request_id, FINGERPRINT_PROMPT);
        let mut emit = |message: &str| self.emit_progress(request_id, message);
        if !self.authenticator.verify(&mut emit, &self.verify_cancel) {
            return Err(error(request_id.clone(), "auth_failed", "Try again"));
        }
        match self
            .session_secret
            .as_ref()
            .map(|secret| secret.password.to_string())
        {
            Some(password) if !password.is_empty() => Ok(password),
            _ => Err(error(
                request_id.clone(),
                "secret_missing",
                "No stored database password. Enter the KeePassXC password once.",
            )),
        }
    }

    fn emit_progress(&self, request_id: &Value, message: &str) {
        if message.is_empty() {
            return;
        }
        if let Some(emit) = &self.progress {
            emit(json!({
                "id": request_id,
                "ok": true,
                "progress": true,
                "message": message,
            }));
        }
    }

    fn has_secret(&self, path: Option<&str>) -> bool {
        let Some(path) = path.filter(|value| !value.trim().is_empty()) else {
            return false;
        };
        self.secret_matches_path(path)
    }

    fn secret_matches_path(&self, path: &str) -> bool {
        self.session_secret
            .as_ref()
            .is_some_and(|secret| paths_match(&secret.path, path))
    }

    fn retain_session_secret(&mut self, path: String, password: &str) {
        self.session_secret = Some(SessionSecret {
            path,
            password: Zeroizing::new(password.to_string()),
        });
    }

    fn drop_session_secret(&mut self) {
        self.session_secret = None;
    }

    fn list(&mut self, request_id: Value) -> Value {
        self.expire_if_needed();
        match self.vault.list_entries() {
            Ok(entries) => {
                let payload: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        json!({
                            "id": entry.identifier,
                            "title": entry.title,
                            "username": entry.username,
                        })
                    })
                    .collect();
                self.touch();
                success(request_id, json!({"entries": payload}))
            }
            Err(VaultLockedError) => error(request_id, "locked", "The database is locked"),
        }
    }

    fn read_field(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        self.expire_if_needed();
        let Some(identifier) = request
            .get("entry_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            return error(request_id, "invalid_request", "entry_id is required");
        };
        if let Some(field) = request.get("field").and_then(Value::as_str) {
            if field != "password" {
                return error(
                    request_id,
                    "unsupported_field",
                    "The field is not supported",
                );
            }
        }
        match self.vault.get_field(identifier) {
            Ok(value) => {
                self.touch();
                success(request_id, json!({"value": value}))
            }
            Err(VaultFieldError::Locked) => error(request_id, "locked", "The database is locked"),
            Err(VaultFieldError::NotFound) => {
                error(request_id, "entry_not_found", "The entry was not found")
            }
        }
    }

    fn read_details(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        self.expire_if_needed();
        let Some(identifier) = request
            .get("entry_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            return error(request_id, "invalid_request", "entry_id is required");
        };
        match self.vault.get_details(identifier) {
            Ok(details) => {
                self.touch();
                success(
                    request_id,
                    json!({
                        "entry_id": details.identifier,
                        "username": details.username,
                        "password": details.password,
                        "url": details.url,
                        "notes": details.notes,
                    }),
                )
            }
            Err(VaultFieldError::Locked) => error(request_id, "locked", "The database is locked"),
            Err(VaultFieldError::NotFound) => {
                error(request_id, "entry_not_found", "The entry was not found")
            }
        }
    }

    fn write_clipboard(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        let Some(text) = request.get("text").and_then(Value::as_str) else {
            return error(request_id, "invalid_request", "text is required");
        };
        if text.is_empty() {
            return error(request_id, "invalid_request", "text is required");
        }
        self.touch();
        match clipboard::write_text(text) {
            Ok(()) => success(request_id, json!({"copied": true})),
            Err(message) => error(request_id, "clipboard_failed", message),
        }
    }

    fn lock(&mut self, request_id: Value) -> Value {
        self.vault.lock();
        self.last_used = None;
        success(request_id, json!({"locked": true}))
    }

    fn status(&mut self, request_id: Value, request: &Map<String, Value>) -> Value {
        if let Err(response) = self.apply_idle_timeout(&request_id, request) {
            return response;
        }
        self.expire_if_needed();
        success(
            request_id,
            json!({
                "state": if self.vault.is_unlocked() { "unlocked" } else { "locked" },
                "configured": self.database_path.is_some(),
                "path": self.database_path,
                "has_stored_secret": self.has_secret(self.database_path.as_deref()),
                "fingerprint_available": self.authenticator.available(),
                "idle_remaining_seconds": self.idle_remaining_seconds(),
            }),
        )
    }

    fn idle_remaining_seconds(&self) -> f64 {
        if !self.vault.is_unlocked() {
            return 0.0;
        }
        let Some(last_used) = self.last_used else {
            return 0.0;
        };
        (self.idle_timeout_seconds - ((self.clock)() - last_used)).max(0.0)
    }

    fn expire_if_needed(&mut self) {
        if self.vault.is_unlocked() {
            if let Some(last_used) = self.last_used {
                if (self.clock)() - last_used >= self.idle_timeout_seconds {
                    self.vault.lock();
                    self.last_used = None;
                }
            }
        }
    }

    fn touch(&mut self) {
        self.last_used = Some((self.clock)());
    }

    fn apply_idle_timeout(
        &mut self,
        request_id: &Value,
        request: &Map<String, Value>,
    ) -> Result<(), Value> {
        let Some(timeout) = request.get("idle_timeout_seconds") else {
            return Ok(());
        };
        match as_positive_number(timeout) {
            Some(value) => {
                self.idle_timeout_seconds = value;
                Ok(())
            }
            None => Err(error(
                request_id.clone(),
                "invalid_request",
                "idle_timeout_seconds must be positive",
            )),
        }
    }
}

fn as_positive_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| *number > 0.0)
}

fn success(request_id: Value, result: Value) -> Value {
    json!({"id": request_id, "ok": true, "result": result})
}

fn error(request_id: Value, code: &str, message: impl Into<String>) -> Value {
    json!({
        "id": request_id,
        "ok": false,
        "error": {"code": code, "message": message.into()},
    })
}

fn open_error(request_id: Value, failure: VaultOpenError) -> Value {
    let code = match failure {
        VaultOpenError::NotFound(_) => "database_not_found",
        VaultOpenError::PasswordRejected => "password_rejected",
        _ => "database_open_failed",
    };
    error(request_id, code, failure.to_string())
}

fn write_response<W: Write>(output: &mut W, response: &Value) {
    let encoded = serde_json::to_string(response).expect("json");
    let _ = writeln!(output, "{encoded}");
    let _ = output.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::ConstantAuthenticator;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

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

    struct FakeClock {
        value: Arc<AtomicU64>,
    }

    impl FakeClock {
        fn new() -> (Self, Arc<AtomicU64>) {
            let value = Arc::new(AtomicU64::new(0));
            (
                Self {
                    value: value.clone(),
                },
                value,
            )
        }

        fn handle(self) -> Box<dyn Fn() -> f64 + Send> {
            Box::new(move || self.value.load(Ordering::SeqCst) as f64)
        }
    }

    fn make_helper(authenticator: Box<dyn Authenticator + Send>) -> (Helper, Arc<AtomicU64>) {
        let (clock, value) = FakeClock::new();
        let helper = Helper::with_parts(clock.handle(), 180.0, authenticator);
        (helper, value)
    }

    fn default_helper() -> (Helper, Arc<AtomicU64>) {
        make_helper(Box::new(ConstantAuthenticator::new(false, false)))
    }

    #[test]
    fn protocol_lifecycle_lists_metadata_and_reads_selected_field() {
        let (mut helper, _) = default_helper();
        let configured =
            helper.handle(json!({"id": "configure", "op": "configure", "path": fixture_path()}));
        let unlocked = helper.handle(json!({
            "id": "unlock",
            "op": "unlock",
            "password": FIXTURE_PASSWORD
        }));
        let listed = helper.handle(json!({"id": "list", "op": "list"}));
        let identifier = listed["result"]["entries"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let read_field = helper.handle(json!({
            "id": "read",
            "op": "read_field",
            "entry_id": identifier,
            "field": "password"
        }));
        let encoded_list = listed.to_string();
        assert_eq!(configured["ok"], true);
        assert_eq!(unlocked["ok"], true);
        assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 36);
        assert_eq!(read_field["ok"], true);
        assert!(read_field["result"]["value"].as_str().unwrap().len() > 0);
        assert!(!encoded_list.contains("\"value\""));
        assert!(!encoded_list.contains(FIXTURE_PASSWORD));
    }

    #[test]
    fn read_details_returns_preview_fields_including_entry_password() {
        let (mut helper, _) = default_helper();
        helper.handle(json!({"id": "configure", "op": "configure", "path": fixture_path()}));
        helper.handle(json!({"id": "unlock", "op": "unlock", "password": FIXTURE_PASSWORD}));
        let listed = helper.handle(json!({"id": "list", "op": "list"}));
        let identifier = listed["result"]["entries"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let details =
            helper.handle(json!({"id": "details", "op": "read_details", "entry_id": identifier}));
        assert_eq!(details["ok"], true);
        assert_eq!(details["result"]["entry_id"], identifier);
        assert!(details["result"]["username"]
            .as_str()
            .unwrap()
            .contains("@example.test"));
        assert!(details["result"]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
        assert!(details["result"]["notes"]
            .as_str()
            .unwrap()
            .contains("Disposable fixture"));
        assert!(!details["result"]["password"].as_str().unwrap().is_empty());
        assert_ne!(details["result"]["password"], FIXTURE_PASSWORD);
        assert!(!details.to_string().contains(FIXTURE_PASSWORD));
    }

    #[test]
    fn read_field_rejects_non_password_fields() {
        let (mut helper, _) = default_helper();
        helper.handle(
            json!({"id": 1, "op": "unlock", "path": fixture_path(), "password": FIXTURE_PASSWORD}),
        );
        let listed = helper.handle(json!({"id": 2, "op": "list"}));
        let identifier = listed["result"]["entries"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let username = helper.handle(json!({
            "id": 3,
            "op": "read_field",
            "entry_id": identifier,
            "field": "username"
        }));
        assert_eq!(username["ok"], false);
        assert_eq!(username["error"]["code"], "unsupported_field");
    }

    #[test]
    fn unlock_after_idle_does_not_short_circuit_an_expired_vault() {
        let (mut helper, clock) = default_helper();
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "list"}));
        clock.store(180, Ordering::SeqCst);
        let unlocked = helper.handle(json!({"id": 3, "op": "unlock", "path": fixture_path()}));
        assert_ne!(unlocked["ok"], true);
        assert_ne!(unlocked["result"]["unlocked"], true);
    }

    #[test]
    fn idle_expiry_locks_vault() {
        let (mut helper, clock) = default_helper();
        helper.handle(json!({"id": 1, "op": "configure", "path": fixture_path()}));
        helper.handle(json!({"id": 2, "op": "unlock", "password": FIXTURE_PASSWORD}));
        helper.handle(json!({"id": 3, "op": "list"}));
        clock.store(180, Ordering::SeqCst);
        let status = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(status["result"]["state"], "locked");
        assert_eq!(status["result"]["path"], canonical_fixture_path());
        assert_eq!(status["result"]["has_stored_secret"], true);
        assert_eq!(status["result"]["idle_remaining_seconds"], 0.0);
        let locked_list = helper.handle(json!({"id": 5, "op": "list"}));
        assert_eq!(locked_list["error"]["code"], "locked");
    }

    #[test]
    fn open_and_request_errors_have_stable_codes() {
        let (mut helper, _) = default_helper();
        let missing = helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": "/missing/synthetic.kdbx",
            "password": "x"
        }));
        let wrong_password = helper.handle(json!({
            "id": 2,
            "op": "unlock",
            "path": fixture_path(),
            "password": "wrong"
        }));
        let malformed = helper.handle(json!({"id": 3, "op": "unknown"}));
        assert_eq!(missing["error"]["code"], "database_not_found");
        assert_eq!(wrong_password["error"]["code"], "password_rejected");
        assert_eq!(malformed["error"]["code"], "unknown_operation");
    }

    #[test]
    fn fingerprint_unlock_reuses_stored_secret_after_lock() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        let unlocked = helper.handle(json!({"id": 3, "op": "unlock", "path": fixture_path()}));
        let status = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(unlocked["ok"], true);
        assert_eq!(unlocked["result"]["secret_stored"], true);
        assert_eq!(status["result"]["state"], "unlocked");
        assert_eq!(status["result"]["has_stored_secret"], true);
        assert!(!unlocked.to_string().contains(FIXTURE_PASSWORD));
        assert!(!status.to_string().contains(FIXTURE_PASSWORD));
    }

    #[test]
    fn fingerprint_progress_is_emitted_during_unlock() {
        struct ReportingAuthenticator;
        impl Authenticator for ReportingAuthenticator {
            fn available(&self) -> bool {
                true
            }
            fn verify(&self, on_status: &mut dyn FnMut(&str), _cancel: &AtomicBool) -> bool {
                on_status("Checking fingerprint");
                true
            }
        }

        let (mut helper, _) = make_helper(Box::new(ReportingAuthenticator));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        let request = json!({"id": 3, "op": "unlock", "path": fixture_path()});
        let output = helper.run(format!("{request}\n").as_bytes(), Vec::new());
        let lines: Vec<Value> = String::from_utf8(output)
            .expect("utf8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect();
        let progress_index = lines.iter().position(|item| {
            item.get("progress") == Some(&Value::Bool(true))
                && item.get("message") == Some(&Value::String("Checking fingerprint".into()))
        });
        let response_index = lines
            .iter()
            .position(|item| item.get("id") == Some(&json!(3)) && item.get("progress").is_none());
        assert!(lines.iter().any(|item| {
            item.get("progress") == Some(&Value::Bool(true))
                && item.get("message") == Some(&Value::String(FINGERPRINT_PROMPT.into()))
        }));
        assert!(progress_index.is_some());
        assert!(response_index.is_some());
        assert!(progress_index.unwrap() < response_index.unwrap());
        assert_eq!(lines[response_index.unwrap()]["ok"], true);
    }

    #[test]
    fn lock_cancels_in_flight_fingerprint_verify() {
        struct BlockingAuthenticator;
        impl Authenticator for BlockingAuthenticator {
            fn available(&self) -> bool {
                true
            }
            fn verify(&self, on_status: &mut dyn FnMut(&str), cancel: &AtomicBool) -> bool {
                on_status(FINGERPRINT_PROMPT);
                let deadline = Instant::now() + Duration::from_secs(5);
                while !cancel.load(Ordering::SeqCst) {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                false
            }
        }

        let (mut helper, _) = make_helper(Box::new(BlockingAuthenticator));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"id": 3, "op": "unlock", "path": fixture_path()}),
            json!({"id": 4, "op": "lock"}),
            json!({"id": 5, "op": "status"})
        );
        let started = Instant::now();
        let output = helper.run(input.as_bytes(), Vec::new());
        assert!(started.elapsed() < Duration::from_secs(2));
        let lines: Vec<Value> = String::from_utf8(output)
            .expect("utf8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect();
        let unlocked = lines
            .iter()
            .find(|item| item.get("id") == Some(&json!(3)) && item.get("progress").is_none())
            .expect("unlock");
        let locked = lines
            .iter()
            .find(|item| item.get("id") == Some(&json!(4)))
            .expect("lock");
        let status = lines
            .iter()
            .find(|item| item.get("id") == Some(&json!(5)))
            .expect("status");
        assert_eq!(unlocked["ok"], false);
        assert_eq!(unlocked["error"]["code"], "auth_failed");
        assert_eq!(locked["ok"], true);
        assert_eq!(status["result"]["state"], "locked");
        assert_eq!(status["result"]["has_stored_secret"], true);
    }

    #[test]
    fn fingerprint_failure_leaves_vault_locked() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(false, true)));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        let failed = helper.handle(json!({"id": 3, "op": "unlock", "path": fixture_path()}));
        let status = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(failed["error"]["code"], "auth_failed");
        assert_eq!(failed["error"]["message"], "Try again");
        assert_eq!(status["result"]["state"], "locked");
        assert_eq!(status["result"]["has_stored_secret"], true);
    }

    #[test]
    fn unlock_without_password_requires_stored_secret() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        let missing = helper.handle(json!({"id": 1, "op": "unlock", "path": fixture_path()}));
        assert_eq!(missing["error"]["code"], "secret_missing");
    }

    #[test]
    fn typed_unlock_then_status_stays_unlocked() {
        let (mut helper, _) = default_helper();
        let canonical = PathBuf::from(canonical_fixture_path());
        let alias = canonical
            .parent()
            .expect("parent")
            .join(".")
            .join(canonical.file_name().expect("name"));
        let configured = helper.handle(json!({"id": 1, "op": "configure", "path": alias}));
        let unlocked = helper.handle(json!({
            "id": 2,
            "op": "unlock",
            "path": alias,
            "password": FIXTURE_PASSWORD
        }));
        let listed = helper.handle(json!({"id": 3, "op": "list"}));
        let status = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(configured["ok"], true);
        assert_eq!(unlocked["ok"], true);
        assert_eq!(listed["ok"], true);
        assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 36);
        assert_eq!(status["result"]["state"], "unlocked");
        assert_eq!(status["result"]["path"], canonical_fixture_path());
        assert_ne!(status["result"]["path"], alias.to_string_lossy().as_ref());
    }

    #[test]
    fn write_clipboard_rejects_missing_text() {
        let (mut helper, _) = default_helper();
        let missing = helper.handle(json!({"id": 1, "op": "write_clipboard"}));
        let empty = helper.handle(json!({"id": 2, "op": "write_clipboard", "text": ""}));
        assert_eq!(missing["error"]["code"], "invalid_request");
        assert_eq!(empty["error"]["code"], "invalid_request");
    }

    #[test]
    fn fingerprint_unlock_accepts_non_canonical_path_alias() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        let canonical = PathBuf::from(canonical_fixture_path());
        let alias = canonical
            .parent()
            .expect("parent")
            .join(".")
            .join(canonical.file_name().expect("name"));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": alias,
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        let unlocked = helper.handle(json!({
            "id": 3,
            "op": "unlock",
            "path": fixture_path()
        }));
        let status = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(unlocked["ok"], true);
        assert_eq!(status["result"]["state"], "unlocked");
        assert_eq!(status["result"]["path"], canonical_fixture_path());
        assert_eq!(status["result"]["has_stored_secret"], true);
    }

    #[test]
    fn configure_after_lock_keeps_stored_secret_for_fingerprint() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        helper.handle(json!({
            "id": 3,
            "op": "configure",
            "path": fixture_path()
        }));
        let unlocked = helper.handle(json!({"id": 4, "op": "unlock", "path": fixture_path()}));
        let status = helper.handle(json!({"id": 5, "op": "status"}));
        assert_eq!(unlocked["ok"], true);
        assert_eq!(status["result"]["state"], "unlocked");
        assert_eq!(status["result"]["has_stored_secret"], true);
    }

    #[test]
    fn fresh_helper_has_no_session_secret() {
        let (mut helper, _) = default_helper();
        let status = helper.handle(json!({"id": 1, "op": "status"}));
        assert_eq!(status["result"]["has_stored_secret"], false);
        assert_eq!(status["result"]["state"], "locked");
    }

    #[test]
    fn rejected_typed_password_does_not_retain_a_secret() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        let rejected = helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": "wrong"
        }));
        let missing = helper.handle(json!({
            "id": 2,
            "op": "unlock",
            "path": fixture_path()
        }));
        let status = helper.handle(json!({"id": 3, "op": "status"}));
        assert_eq!(rejected["error"]["code"], "password_rejected");
        assert_eq!(missing["error"]["code"], "secret_missing");
        assert_eq!(status["result"]["has_stored_secret"], false);
    }

    #[test]
    fn unlock_payload_sets_idle_timeout() {
        let (mut helper, clock) = default_helper();
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD,
            "idle_timeout_seconds": 30
        }));
        helper.handle(json!({"id": 2, "op": "list"}));
        clock.store(29, Ordering::SeqCst);
        let early = helper.handle(json!({"id": 3, "op": "status"}));
        clock.store(30, Ordering::SeqCst);
        let expired = helper.handle(json!({"id": 4, "op": "status"}));
        assert_eq!(early["result"]["state"], "unlocked");
        assert_eq!(early["result"]["idle_remaining_seconds"], 1.0);
        assert_eq!(expired["result"]["state"], "locked");
        assert_eq!(expired["result"]["idle_remaining_seconds"], 0.0);
    }

    #[test]
    fn write_clipboard_counts_as_vault_use() {
        let (mut helper, clock) = default_helper();
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "list"}));
        clock.store(100, Ordering::SeqCst);
        helper.handle(json!({"id": 3, "op": "write_clipboard", "text": "copied"}));
        clock.store(180, Ordering::SeqCst);
        let still_open = helper.handle(json!({"id": 4, "op": "status"}));
        clock.store(280, Ordering::SeqCst);
        let expired = helper.handle(json!({"id": 5, "op": "status"}));
        assert_eq!(still_open["result"]["state"], "unlocked");
        assert_eq!(expired["result"]["state"], "locked");
    }

    #[test]
    fn empty_database_list_is_success() {
        use keepass::db::Database;
        use keepass::DatabaseKey;
        use std::fs::File;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("keepassxc-control-empty-{nanos}.kdbx"));
        let database = Database::new();
        let mut file = File::create(&path).expect("create");
        database
            .save(
                &mut file,
                DatabaseKey::new().with_password(FIXTURE_PASSWORD),
            )
            .expect("save");
        drop(file);

        let (mut helper, _) = default_helper();
        let unlocked = helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": path,
            "password": FIXTURE_PASSWORD
        }));
        let listed = helper.handle(json!({"id": 2, "op": "list"}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(unlocked["ok"], true);
        assert_eq!(listed["ok"], true);
        assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 0);
        assert_ne!(listed["error"]["code"], "database_open_failed");
    }

    #[test]
    fn different_path_configure_drops_session_secret() {
        let (mut helper, _) = make_helper(Box::new(ConstantAuthenticator::new(true, true)));
        helper.handle(json!({
            "id": 1,
            "op": "unlock",
            "path": fixture_path(),
            "password": FIXTURE_PASSWORD
        }));
        helper.handle(json!({"id": 2, "op": "lock"}));
        helper.handle(json!({
            "id": 3,
            "op": "configure",
            "path": "/missing/keepassxc-control.kdbx"
        }));
        let missing = helper.handle(json!({
            "id": 4,
            "op": "unlock",
            "path": fixture_path()
        }));
        let status = helper.handle(json!({"id": 5, "op": "status"}));
        assert_eq!(missing["error"]["code"], "secret_missing");
        assert_eq!(status["result"]["has_stored_secret"], false);
    }
}
