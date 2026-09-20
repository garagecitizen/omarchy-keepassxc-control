use std::fs::File;
use std::io::{self, ErrorKind};
use std::path::Path;

use keepass::db::{fields, Database};
use keepass::DatabaseKey;

pub const FIXTURE_PASSWORD: &str = "spike-only-password";
pub const SEED: u64 = 20260902;
pub const ENTRY_COUNT: usize = 36;

const SERVICES: &[&str] = &[
    "Aurora", "Beacon", "Cedar", "Delta", "Ember", "Fjord", "Garnet", "Harbor", "Indigo",
    "Juniper", "Kestrel", "Lumen", "Mosaic", "Nimbus", "Oak", "Pioneer", "Quartz", "Raven",
    "Summit", "Tundra", "Umber", "Vertex", "Willow", "Xenon", "Yonder", "Zephyr",
];
const ALPHABET: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&*+-=?@_";

// ponytail: xorshift is enough for disposable fixture passwords; switch to a
// counted CSPRNG if this generator is ever used for real secrets.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn password(&mut self, length: usize) -> String {
        (0..length)
            .map(|_| ALPHABET[(self.next_u64() as usize) % ALPHABET.len()] as char)
            .collect()
    }
}

pub fn write_fixture(path: &Path, force: bool) -> io::Result<()> {
    if path.exists() && !force {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("{} already exists; pass --force to replace it", path.display()),
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut rng = Rng::new(SEED);
    let mut database = Database::new();
    {
        let mut root = database.root_mut();
        let mut group = root.add_group();
        group.name = "Synthetic Entries".to_string();
        for index in 0..ENTRY_COUNT {
            let service = SERVICES[index % SERVICES.len()];
            let suffix = index / SERVICES.len() + 1;
            let password = rng.password(24);
            group.add_entry().edit(|entry| {
                entry.set_unprotected(
                    fields::TITLE,
                    format!("{service} Test Account {suffix:02}"),
                );
                entry.set_unprotected(
                    fields::USERNAME,
                    format!("synthetic-{:02}@example.test", index + 1),
                );
                entry.set_protected(fields::PASSWORD, password);
                entry.set_unprotected(
                    fields::URL,
                    format!(
                        "https://{}.example.test/login",
                        service.to_ascii_lowercase()
                    ),
                );
                entry.set_unprotected(
                    fields::NOTES,
                    "Disposable fixture entry; never use this credential.",
                );
            });
        }
    }

    let mut file = File::create(path)?;
    database
        .save(&mut file, DatabaseKey::new().with_password(FIXTURE_PASSWORD))
        .map_err(|err| io::Error::other(err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_thirty_six_named_entries() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("keepassxc-control-synthetic-{nanos}.kdbx"));
        write_fixture(&path, true).expect("write fixture");
        let mut vault = Vault::new();
        vault.open(&path, FIXTURE_PASSWORD).expect("open fixture");
        let entries = vault.list_entries().unwrap();
        let titles: Vec<_> = entries.iter().map(|entry| entry.title.as_str()).collect();
        let _ = std::fs::remove_file(&path);
        assert_eq!(entries.len(), ENTRY_COUNT);
        assert!(titles.contains(&"Aurora Test Account 01"));
        assert!(titles.contains(&"Juniper Test Account 02"));
        let aurora = entries
            .iter()
            .find(|entry| entry.title == "Aurora Test Account 01")
            .expect("aurora");
        assert_eq!(aurora.username, "synthetic-01@example.test");
    }
}
