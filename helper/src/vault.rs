use std::fs::File;
use std::path::{Path, PathBuf};

use keepass::db::{Database, DatabaseOpenError, EntryRef};
use keepass::error::DatabaseKeyError;
use keepass::DatabaseKey;

use crate::credentials::expand_user;

#[derive(Debug)]
pub enum VaultOpenError {
    NotFound(PathBuf),
    EmptyPassword,
    PasswordRejected,
    Invalid,
    Other,
}

impl std::fmt::Display for VaultOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Database file was not found: {}", path.display()),
            Self::EmptyPassword => write!(f, "A non-empty database password is required"),
            Self::PasswordRejected => write!(f, "The database password was rejected"),
            Self::Invalid => write!(f, "The database file is invalid or unsupported"),
            Self::Other => write!(f, "The database could not be opened"),
        }
    }
}

impl std::error::Error for VaultOpenError {}

#[derive(Debug)]
pub struct VaultLockedError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySummary {
    pub identifier: String,
    pub title: String,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryDetails {
    pub identifier: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
}

pub struct Vault {
    database: Option<Database>,
    entries: Option<Vec<EntrySummary>>,
    path: Option<PathBuf>,
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}

impl Vault {
    pub fn new() -> Self {
        Self {
            database: None,
            entries: None,
            path: None,
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.database.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn open(&mut self, path: impl AsRef<Path>, password: &str) -> Result<(), VaultOpenError> {
        self.lock();

        let database_path = expand_user(path.as_ref());
        if !database_path.is_file() {
            return Err(VaultOpenError::NotFound(database_path));
        }
        if password.is_empty() {
            return Err(VaultOpenError::EmptyPassword);
        }

        let mut file = File::open(&database_path).map_err(|_| VaultOpenError::Other)?;
        let key = DatabaseKey::new().with_password(password);
        let database = Database::open(&mut file, key).map_err(map_open_error)?;
        let resolved = database_path.canonicalize().unwrap_or(database_path);

        self.database = Some(database);
        self.path = Some(resolved);
        self.entries = None;
        Ok(())
    }

    pub fn list_entries(&mut self) -> Result<&[EntrySummary], VaultLockedError> {
        self.require_database()?;
        if self.entries.is_none() {
            let summaries = {
                let database = self.database.as_ref().ok_or(VaultLockedError)?;
                database
                    .iter_all_entries()
                    .map(|entry| EntrySummary {
                        identifier: entry_hex(&entry),
                        title: owned_or(entry.get_title(), "(untitled)"),
                        username: owned_or(entry.get_username(), ""),
                    })
                    .collect()
            };
            self.entries = Some(summaries);
        }
        Ok(self.entries.as_deref().unwrap_or(&[]))
    }

    pub fn get_field(&self, identifier: &str) -> Result<String, VaultFieldError> {
        let entry = self.find_entry(identifier)?;
        Ok(owned_or(entry.get_password(), ""))
    }

    pub fn get_details(&self, identifier: &str) -> Result<EntryDetails, VaultFieldError> {
        let entry = self.find_entry(identifier)?;
        Ok(EntryDetails {
            identifier: entry_hex(&entry),
            username: owned_or(entry.get_username(), ""),
            password: owned_or(entry.get_password(), ""),
            url: owned_or(entry.get_url(), ""),
            notes: owned_or(entry.get("Notes"), ""),
        })
    }

    pub fn lock(&mut self) {
        self.database = None;
        self.entries = None;
        self.path = None;
    }

    fn find_entry(&self, identifier: &str) -> Result<EntryRef<'_>, VaultFieldError> {
        let database = self.database.as_ref().ok_or(VaultFieldError::Locked)?;
        database
            .iter_all_entries()
            .find(|entry| entry_hex(entry) == identifier)
            .ok_or(VaultFieldError::NotFound)
    }

    fn require_database(&self) -> Result<(), VaultLockedError> {
        self.database.as_ref().map(|_| ()).ok_or(VaultLockedError)
    }
}

#[derive(Debug)]
pub enum VaultFieldError {
    Locked,
    NotFound,
}

fn entry_hex(entry: &EntryRef<'_>) -> String {
    entry.id().uuid().as_simple().to_string()
}

fn owned_or(value: Option<&str>, fallback: &str) -> String {
    value
        .filter(|text| !text.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn map_open_error(error: DatabaseOpenError) -> VaultOpenError {
    match error {
        DatabaseOpenError::Key(DatabaseKeyError::IncorrectKey | DatabaseKeyError::EmptyKey) => {
            VaultOpenError::PasswordRejected
        }
        DatabaseOpenError::Io(_) => VaultOpenError::Other,
        DatabaseOpenError::UnexpectedEof
        | DatabaseOpenError::VersionParse(_)
        | DatabaseOpenError::UnsupportedVersion
        | DatabaseOpenError::Cryptography(_)
        | DatabaseOpenError::Format(_) => VaultOpenError::Invalid,
        DatabaseOpenError::Key(_) => VaultOpenError::Other,
        _ => VaultOpenError::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keepass::db::fields;
    use std::time::{SystemTime, UNIX_EPOCH};

    const PASSWORD: &str = "test-only-database-password";

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("keepassxc-control-{name}-{nanos}.kdbx"))
    }

    fn make_database() -> PathBuf {
        let path = temp_path("fixture");
        let mut database = Database::new();
        database.root_mut().add_entry().edit(|entry| {
            entry.set_unprotected(fields::TITLE, "Example");
            entry.set_unprotected(fields::USERNAME, "mike");
            entry.set_protected(fields::PASSWORD, "not-a-real-secret");
            entry.set_unprotected(fields::URL, "https://example.test");
            entry.set_unprotected(fields::NOTES, "Disposable fixture note");
        });
        database.root_mut().add_entry().edit(|entry| {
            entry.set_unprotected(fields::TITLE, "Untitled username");
            entry.set_protected(fields::PASSWORD, "another-test-secret");
        });
        let mut file = File::create(&path).expect("create fixture");
        database
            .save(&mut file, DatabaseKey::new().with_password(PASSWORD))
            .expect("save fixture");
        path
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/synthetic.kdbx")
    }

    #[test]
    fn opens_and_lists_metadata_once() {
        let path = make_database();
        let mut vault = Vault::new();
        vault.open(&path, PASSWORD).unwrap();
        let first: Vec<_> = vault.list_entries().unwrap().to_vec();
        let second: Vec<_> = vault.list_entries().unwrap().to_vec();
        assert!(vault.is_unlocked());
        assert_eq!(first, second);
        let mut titles: Vec<_> = first.iter().map(|entry| entry.title.as_str()).collect();
        titles.sort_unstable();
        assert_eq!(titles, ["Example", "Untitled username"]);
        let example = first.iter().find(|entry| entry.title == "Example").unwrap();
        let untitled = first
            .iter()
            .find(|entry| entry.title == "Untitled username")
            .unwrap();
        assert_eq!(example.username, "mike");
        assert_eq!(untitled.username, "");
        assert!(!format!("{first:?}").contains("not-a-real-secret"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_only_the_requested_field() {
        let path = make_database();
        let mut vault = Vault::new();
        vault.open(&path, PASSWORD).unwrap();
        let identifier = vault
            .list_entries()
            .unwrap()
            .iter()
            .find(|entry| entry.title == "Example")
            .expect("example entry")
            .identifier
            .clone();
        assert_eq!(
            vault.get_field(&identifier).unwrap(),
            "not-a-real-secret"
        );
        let details = vault.get_details(&identifier).unwrap();
        assert_eq!(details.username, "mike");
        assert_eq!(details.password, "not-a-real-secret");
        assert_eq!(details.url, "https://example.test");
        assert_eq!(details.notes, "Disposable fixture note");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn lock_releases_database_and_metadata() {
        let path = make_database();
        let mut vault = Vault::new();
        vault.open(&path, PASSWORD).unwrap();
        vault.list_entries().unwrap();
        vault.lock();
        assert!(!vault.is_unlocked());
        assert!(vault.list_entries().is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn wrong_password_is_a_non_secret_error() {
        let path = make_database();
        let error = Vault::new().open(&path, "wrong-password").unwrap_err();
        assert!(matches!(error, VaultOpenError::PasswordRejected));
        assert!(error.to_string().contains("password was rejected"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_file_is_reported() {
        let path = temp_path("missing");
        let error = Vault::new().open(&path, PASSWORD).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn invalid_database_is_reported() {
        let path = temp_path("invalid");
        std::fs::write(&path, b"not a KeePass database").unwrap();
        let error = Vault::new().open(&path, PASSWORD).unwrap_err();
        assert!(error.to_string().contains("invalid or unsupported"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_entry_is_rejected() {
        let path = make_database();
        let mut vault = Vault::new();
        vault.open(&path, PASSWORD).unwrap();
        assert!(matches!(
            vault.get_field("missing-entry"),
            Err(VaultFieldError::NotFound)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stores_canonical_path_after_open() {
        let path = make_database();
        let canonical = path.canonicalize().unwrap();
        let alias = path
            .parent()
            .unwrap()
            .join(".")
            .join(path.file_name().unwrap());
        let mut vault = Vault::new();
        vault.open(&alias, PASSWORD).unwrap();
        assert_eq!(vault.path().unwrap(), canonical.as_path());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn opens_the_checked_in_synthetic_fixture() {
        let mut vault = Vault::new();
        vault
            .open(fixture_path(), "spike-only-password")
            .expect("synthetic fixture");
        let entries = vault.list_entries().unwrap();
        assert_eq!(entries.len(), 36);
        assert!(entries
            .iter()
            .any(|entry| entry.title == "Aurora Test Account 01"));
    }
}
