use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::ErrorKind};

use crate::fs_store::secret_store_path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedSecretStore {
    #[serde(default)]
    entries: BTreeMap<String, String>,
}

fn entry_key(service: &str, account: &str) -> String {
    format!("{service}:{account}")
}

pub(crate) fn load_fallback_secret(service: &str, account: &str) -> Result<Option<String>, String> {
    let store = read_secret_store()?;
    Ok(store.entries.get(&entry_key(service, account)).cloned())
}

pub(crate) fn save_fallback_secret(
    service: &str,
    account: &str,
    secret: &str,
) -> Result<(), String> {
    let mut store = read_secret_store()?;
    let key = entry_key(service, account);
    if secret.trim().is_empty() {
        store.entries.remove(&key);
    } else {
        store.entries.insert(key, secret.to_string());
    }
    write_secret_store(&store)
}

pub(crate) fn delete_fallback_secret(service: &str, account: &str) -> Result<(), String> {
    let mut store = read_secret_store()?;
    store.entries.remove(&entry_key(service, account));
    write_secret_store(&store)
}

fn read_secret_store() -> Result<PersistedSecretStore, String> {
    let path = secret_store_path();
    match fs::read_to_string(&path) {
        Ok(content) => {
            if content.trim().is_empty() {
                Ok(PersistedSecretStore::default())
            } else {
                serde_json::from_str(&content)
                    .map_err(|err| format!("failed to parse {}: {err}", path.display()))
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(PersistedSecretStore::default()),
        Err(err) => Err(format!("failed to read {}: {err}", path.display())),
    }
}

fn write_secret_store(store: &PersistedSecretStore) -> Result<(), String> {
    let path = secret_store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create storage dir {}: {err}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(store)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, permissions)
            .map_err(|err| format!("failed to protect {}: {err}", path.display()))?;
    }

    #[cfg(windows)]
    {
        // On Windows, %LOCALAPPDATA% is already user-scoped, but we additionally
        // hide the file from casual enumeration by setting the hidden attribute.
        use std::os::windows::fs::MetadataExt;
        if let Ok(metadata) = fs::metadata(&path) {
            let attributes = metadata.file_attributes();
            const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
            if attributes & FILE_ATTRIBUTE_HIDDEN == 0 {
                // Best-effort: if we can't set the hidden bit, the file is still
                // protected by the user-scoped directory.
                let _ = std::process::Command::new("attrib")
                    .args(["+H", &path.to_string_lossy().to_string()])
                    .output();
            }
        }
    }

    Ok(())
}
