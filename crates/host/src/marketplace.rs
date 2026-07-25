use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;

pub const MARKETPLACE_URL: &str =
    "https://raw.githubusercontent.com/AlpaGit/dofus3-native-host/main/marketplace.json";
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_MOD_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceCatalog {
    pub schema_version: u32,
    pub updated_at: String,
    pub mods: Vec<MarketplaceMod>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceMod {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub abi_version: u32,
    pub file_name: String,
    pub download_url: String,
    pub sha256: String,
    #[serde(default)]
    pub homepage: String,
}

#[derive(Debug)]
pub enum WorkerResult {
    Catalog(Result<MarketplaceCatalog, String>),
    Downloaded {
        entry: MarketplaceMod,
        temporary_path: PathBuf,
    },
    DownloadFailed {
        id: String,
        error: String,
    },
}

pub fn refresh_async(sender: Sender<WorkerResult>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let result = download_catalog();
        let _ = sender.send(WorkerResult::Catalog(result));
    })
}

pub fn download_mod_async(
    entry: MarketplaceMod,
    mod_directory: PathBuf,
    sender: Sender<WorkerResult>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let id = entry.id.clone();
        match download_mod(&entry, &mod_directory) {
            Ok(temporary_path) => {
                let _ = sender.send(WorkerResult::Downloaded {
                    entry,
                    temporary_path,
                });
            }
            Err(error) => {
                let _ = sender.send(WorkerResult::DownloadFailed { id, error });
            }
        }
    })
}

pub fn validate_entry(entry: &MarketplaceMod) -> Result<(), String> {
    if entry.id.is_empty()
        || !entry
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("identifiant de mod invalide".to_owned());
    }
    if entry.name.trim().is_empty()
        || entry.author.trim().is_empty()
        || entry.version.trim().is_empty()
    {
        return Err(format!("métadonnées incomplètes pour '{}'", entry.id));
    }
    if !(1..=dofus_native_mod_api::DNH_ABI_VERSION).contains(&entry.abi_version) {
        return Err(format!(
            "ABI v{} non supportée pour '{}'",
            entry.abi_version, entry.id
        ));
    }
    validate_file_name(&entry.file_name)?;
    if !entry.download_url.starts_with("https://") {
        return Err(format!("URL non HTTPS pour '{}'", entry.id));
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("SHA-256 invalide pour '{}'", entry.id));
    }
    Ok(())
}

pub fn validate_file_name(file_name: &str) -> Result<(), String> {
    let path = Path::new(file_name);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || !path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    {
        return Err("le nom de fichier du mod doit être une DLL sans chemin".to_owned());
    }
    Ok(())
}

fn download_catalog() -> Result<MarketplaceCatalog, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent
        .get(MARKETPLACE_URL)
        .header("User-Agent", "Dofus3NativeHost")
        .call()
        .map_err(|error| format!("téléchargement du catalogue impossible : {error}"))?;
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_CATALOG_BYTES)
        .read_to_string()
        .map_err(|error| format!("lecture du catalogue impossible : {error}"))?;
    let catalog: MarketplaceCatalog = serde_json::from_str(&text)
        .map_err(|error| format!("JSON du catalogue invalide : {error}"))?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "version de catalogue non supportée : {}",
            catalog.schema_version
        ));
    }
    for entry in &catalog.mods {
        validate_entry(entry)?;
    }
    Ok(catalog)
}

fn download_mod(entry: &MarketplaceMod, mod_directory: &Path) -> Result<PathBuf, String> {
    validate_entry(entry)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut response = agent
        .get(&entry.download_url)
        .header("User-Agent", "Dofus3NativeHost")
        .call()
        .map_err(|error| format!("téléchargement de '{}' impossible : {error}", entry.name))?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_MOD_BYTES)
        .reader();

    let temporary_path = mod_directory.join(format!(
        ".{}.{}.download",
        entry.file_name,
        std::process::id()
    ));
    let mut output = File::create(&temporary_path)
        .map_err(|error| format!("création du fichier temporaire impossible : {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let copy_result = (|| -> Result<(), String> {
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|error| format!("lecture de la DLL impossible : {error}"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("écriture de la DLL impossible : {error}"))?;
        }
        output
            .flush()
            .map_err(|error| format!("finalisation de la DLL impossible : {error}"))
    })();
    if let Err(error) = copy_result {
        drop(output);
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }

    let actual_hash = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual_hash.eq_ignore_ascii_case(&entry.sha256) {
        drop(output);
        let _ = std::fs::remove_file(&temporary_path);
        return Err(format!(
            "SHA-256 incorrect pour '{}' (attendu {}, reçu {})",
            entry.name, entry.sha256, actual_hash
        ));
    }
    Ok(temporary_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> MarketplaceMod {
        MarketplaceMod {
            id: "dofus3.native-tactical".to_owned(),
            name: "Native Tactical".to_owned(),
            author: "Nexytrus".to_owned(),
            description: "Mode tactique".to_owned(),
            version: "4.1.0".to_owned(),
            abi_version: 4,
            file_name: "DofusNativeTactical.dll".to_owned(),
            download_url: "https://github.com/example/mod.dll".to_owned(),
            sha256: "a".repeat(64),
            homepage: String::new(),
        }
    }

    #[test]
    fn accepts_valid_entry() {
        assert!(validate_entry(&entry()).is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        let mut value = entry();
        value.file_name = "..\\evil.dll".to_owned();
        assert!(validate_entry(&value).is_err());
    }

    #[test]
    fn rejects_non_https_downloads() {
        let mut value = entry();
        value.download_url = "http://example.com/mod.dll".to_owned();
        assert!(validate_entry(&value).is_err());
    }

    #[test]
    fn repository_catalog_matches_schema() {
        let catalog: MarketplaceCatalog =
            serde_json::from_str(include_str!("../../../marketplace.json"))
                .expect("repository marketplace.json");
        assert_eq!(catalog.schema_version, 1);
        assert!(!catalog.mods.is_empty());
        for entry in &catalog.mods {
            validate_entry(entry).expect("valid marketplace entry");
        }
    }
}
