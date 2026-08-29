use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use heminus_domain::{Identity, IdentityKind};
use serde::Serialize;
use ssh_key::{Algorithm, LineEnding, PrivateKey, rand_core::OsRng};
use tauri::State;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateKeyImport {
    identity: Identity,
    format: String,
    encrypted: bool,
    already_present: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyMaterial {
    private_key: String,
    public_key: Option<String>,
    certificate: Option<String>,
}

#[tauri::command]
pub async fn read_key_material(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<KeyMaterial, String> {
    let identity = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .find_identity(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected key no longer exists".to_string())?;
    if identity.kind != IdentityKind::KeyFile {
        return Err("The selected identity is not a private key".into());
    }
    let path = identity
        .key_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "The key does not have private material".to_string())?;
    if !state.ssh.owns_key(&path) {
        return Err("The private key is outside the Heminus vault".into());
    }
    let private_key = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read the private key: {error}"))?;
    let public_path = public_key_path(&path);
    let mut public_key = fs::read_to_string(&public_path).ok();
    if public_key.is_none() {
        let passphrase = identity
            .secret_stored
            .then(|| crate::credential::get(identity.id))
            .and_then(Result::ok);
        // A key whose passphrase is not saved simply has no derivable public
        // half here; the pane renders without it rather than failing.
        public_key = derive_public_key(&path, passphrase.as_ref().map(|value| value.as_str())).ok();
    }
    let certificate = fs::read_to_string(certificate_path(&path)).ok();
    Ok(KeyMaterial {
        private_key,
        public_key,
        certificate,
    })
}

pub fn migrate_external_keys(
    database: &heminus_storage::Database,
    runtime: &crate::ssh_runtime::SshRuntime,
) -> Result<usize, String> {
    let mut migrated = 0;
    for mut identity in database
        .list_identities()
        .map_err(|error| error.to_string())?
    {
        if identity.kind != IdentityKind::KeyFile {
            continue;
        }
        let Some(source) = identity.key_path.as_deref().map(PathBuf::from) else {
            continue;
        };
        if runtime.owns_key(&source) || !source.is_file() {
            continue;
        }
        let destination = imported_key_path(runtime.keys_directory(), &source)?;
        fs::copy(&source, &destination)
            .map_err(|error| format!("Could not migrate {}: {error}", source.display()))?;
        if let Err(error) = secure_private_key_permissions(&destination) {
            remove_key_files(&destination);
            return Err(error);
        }
        let source_public = public_key_path(&source);
        if source_public.is_file() {
            let _ = fs::copy(source_public, public_key_path(&destination));
        }
        let source_certificate = certificate_path(&source);
        if source_certificate.is_file() {
            let _ = fs::copy(source_certificate, certificate_path(&destination));
        }
        identity.key_path = Some(destination.to_string_lossy().into_owned());
        if let Err(error) = database.save_identity(&identity) {
            remove_key_files(&destination);
            return Err(error.to_string());
        }
        migrated += 1;
    }
    Ok(migrated)
}

#[tauri::command]
pub async fn generate_ssh_key(
    state: State<'_, AppState>,
    label: String,
    file_name: String,
    comment: String,
    passphrase: Option<String>,
) -> Result<Identity, String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("Key name cannot be empty".into());
    }
    let id = Uuid::new_v4();
    let file_name = if file_name.trim().is_empty() {
        format!("heminus_ed25519_{}", id.simple())
    } else {
        file_name
    };
    let path = private_key_path(state.ssh.keys_directory(), &file_name)?;
    if path.exists() || public_key_path(&path).exists() {
        return Err("A key with this file name already exists".into());
    }
    let path_for_generation = path.clone();
    let comment = if comment.trim().is_empty() {
        "heminus@vault".to_string()
    } else {
        comment.trim().to_string()
    };
    let has_passphrase = passphrase.as_deref().is_some_and(|value| !value.is_empty());
    let passphrase = passphrase.map(Zeroizing::new);

    tauri::async_runtime::spawn_blocking(move || {
        generate_ed25519_files(
            &path_for_generation,
            &comment,
            passphrase.as_deref().map(String::as_str),
        )?;
        let store_result = passphrase
            .filter(|value| !value.is_empty())
            .map(|passphrase| crate::credential::set(id, passphrase.to_string()));
        if let Some(Err(error)) = store_result {
            remove_key_files(&path_for_generation);
            return Err(error);
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| error.to_string())??;

    let mut identity = Identity::new(label, IdentityKind::KeyFile);
    identity.id = id;
    identity.key_path = Some(path.to_string_lossy().into_owned());
    identity.secret_stored = has_passphrase;
    if let Err(error) = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .save_identity(&identity)
    {
        let _ = crate::credential::delete(id);
        remove_key_files(&path);
        return Err(error.to_string());
    }
    Ok(identity)
}

#[tauri::command]
pub async fn import_private_key(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<PrivateKeyImport, String> {
    let scratch = state.ssh.scratch_directory()?;
    let inspected =
        tauri::async_runtime::spawn_blocking(move || inspect_private_key(&path, &scratch))
            .await
            .map_err(|error| error.to_string())??;
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let source_contents = fs::read(&inspected.path)
        .map_err(|error| format!("Could not read the private key: {error}"))?;

    if let Some(identity) = database
        .list_identities()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|identity| {
            identity.kind == IdentityKind::KeyFile
                && identity
                    .key_path
                    .as_deref()
                    .and_then(|path| fs::read(path).ok())
                    .is_some_and(|contents| contents == source_contents)
        })
    {
        return Ok(PrivateKeyImport {
            identity,
            format: inspected.format,
            encrypted: inspected.encrypted,
            already_present: true,
        });
    }

    let label = inspected
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Imported private key")
        .to_string();
    let destination = imported_key_path(state.ssh.keys_directory(), &inspected.path)?;
    fs::copy(&inspected.path, &destination)
        .map_err(|error| format!("Could not copy the key into the Heminus vault: {error}"))?;
    if let Err(error) = secure_private_key_permissions(&destination) {
        remove_key_files(&destination);
        return Err(error);
    }
    let source_public = public_key_path(&inspected.path);
    if source_public.is_file() {
        let _ = fs::copy(source_public, public_key_path(&destination));
    }
    let source_certificate = certificate_path(&inspected.path);
    if source_certificate.is_file() {
        let _ = fs::copy(source_certificate, certificate_path(&destination));
    }
    let mut identity = Identity::new(label, IdentityKind::KeyFile);
    identity.key_path = Some(destination.to_string_lossy().into_owned());
    if let Err(error) = database.save_identity(&identity) {
        remove_key_files(&destination);
        return Err(error.to_string());
    }

    Ok(PrivateKeyImport {
        identity,
        format: inspected.format,
        encrypted: inspected.encrypted,
        already_present: false,
    })
}

struct InspectedPrivateKey {
    path: PathBuf,
    format: String,
    encrypted: bool,
}

fn inspect_private_key(path: &Path, scratch: &Path) -> Result<InspectedPrivateKey, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("Could not open the dropped key file: {error}"))?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("Could not inspect the key file: {error}"))?;
    if !metadata.is_file() {
        return Err("Drop a private key file, not a folder".into());
    }
    if metadata.len() == 0 || metadata.len() > 1024 * 1024 {
        return Err("Private key files must be between 1 byte and 1 MiB".into());
    }

    let contents =
        fs::read_to_string(&path).map_err(|_| "The private key must be a text file".to_string())?;
    let trimmed = contents.trim();
    let first_line = trimmed.lines().next().unwrap_or_default().trim();
    let (format, mut encrypted, end_marker) = match first_line {
        "-----BEGIN OPENSSH PRIVATE KEY-----" => {
            let key = PrivateKey::from_openssh(trimmed)
                .map_err(|error| format!("Invalid OpenSSH private key: {error}"))?;
            (
                "OpenSSH",
                key.is_encrypted(),
                "-----END OPENSSH PRIVATE KEY-----",
            )
        }
        "-----BEGIN RSA PRIVATE KEY-----" => ("RSA PEM", false, "-----END RSA PRIVATE KEY-----"),
        "-----BEGIN DSA PRIVATE KEY-----" => ("DSA PEM", false, "-----END DSA PRIVATE KEY-----"),
        "-----BEGIN EC PRIVATE KEY-----" => ("EC PEM", false, "-----END EC PRIVATE KEY-----"),
        "-----BEGIN PRIVATE KEY-----" => ("PKCS#8 PEM", false, "-----END PRIVATE KEY-----"),
        "-----BEGIN ENCRYPTED PRIVATE KEY-----" => (
            "Encrypted PKCS#8 PEM",
            true,
            "-----END ENCRYPTED PRIVATE KEY-----",
        ),
        _ => {
            return Err(
                "Unsupported key format. Drop an OpenSSH, RSA, DSA, EC, or PKCS#8 PEM private key"
                    .into(),
            );
        }
    };
    if !trimmed.ends_with(end_marker) {
        return Err("The private key is incomplete or has an invalid footer".into());
    }
    encrypted |= contents.contains("Proc-Type: 4,ENCRYPTED") || contents.contains("DEK-Info:");

    if !encrypted {
        validate_unencrypted_private_key(&contents, scratch)?;
    }
    Ok(InspectedPrivateKey {
        path,
        format: format.to_string(),
        encrypted,
    })
}

fn validate_unencrypted_private_key(contents: &str, scratch: &Path) -> Result<(), String> {
    let temporary_path = scratch.join(format!("heminus-key-validation-{}", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let validation = (|| {
        let mut file = options
            .open(&temporary_path)
            .map_err(|error| format!("Could not validate the private key: {error}"))?;
        file.write_all(contents.as_bytes())
            .map_err(|error| format!("Could not validate the private key: {error}"))?;
        drop(file);
        crate::platform::secure_private_file(&temporary_path)?;
        let mut command = Command::new(crate::platform::ssh_keygen_executable()?);
        crate::platform::configure_background_command(&mut command);
        let output = command
            .arg("-y")
            .arg("-P")
            .arg("")
            .arg("-f")
            .arg(&temporary_path)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("Could not run ssh-keygen: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error("Invalid or unsupported private key", &output))
        }
    })();
    let _ = fs::remove_file(temporary_path);
    validation
}

fn secure_private_key_permissions(path: &Path) -> Result<(), String> {
    crate::platform::secure_private_file(path)
}

fn private_key_path(keys_directory: &Path, file_name: &str) -> Result<PathBuf, String> {
    let file_name = file_name.trim();
    if file_name.is_empty()
        || file_name.starts_with('.')
        || !file_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.".contains(character))
    {
        return Err("Use a simple key file name without folders".into());
    }
    Ok(keys_directory.join(file_name))
}

fn imported_key_path(keys_directory: &Path, source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("imported-key");
    let safe_name = file_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "_-.".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let candidate = keys_directory.join(&safe_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    Ok(keys_directory.join(format!("{}-{safe_name}", Uuid::new_v4())))
}

fn generate_ed25519_files(
    path: &Path,
    comment: &str,
    passphrase: Option<&str>,
) -> Result<(), String> {
    let private_key =
        PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|error| error.to_string())?;
    let mut private_key = if let Some(passphrase) = passphrase.filter(|value| !value.is_empty()) {
        private_key
            .encrypt(&mut OsRng, passphrase)
            .map_err(|error| error.to_string())?
    } else {
        private_key
    };
    private_key.set_comment(comment);
    private_key
        .write_openssh_file(path, LineEnding::LF)
        .map_err(|error| error.to_string())?;
    secure_private_key_permissions(path)?;
    let public_path = public_key_path(path);
    let mut public_key = private_key
        .public_key()
        .to_openssh()
        .map_err(|error| error.to_string())?;
    if !comment.is_empty() {
        public_key.push(' ');
        public_key.push_str(comment);
    }
    fs::write(&public_path, format!("{public_key}\n")).map_err(|error| error.to_string())?;
    Ok(())
}

fn remove_key_files(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(public_key_path(path));
    let _ = fs::remove_file(certificate_path(path));
}

fn public_key_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.pub",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("id_ed25519")
    ))
}

fn certificate_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("id_ed25519");
    path.with_file_name(format!("{name}-cert.pub"))
}

fn command_error(context: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if stderr.is_empty() {
        format!("{context} (exit code {:?})", output.status.code())
    } else {
        format!("{context}: {stderr}")
    }
}

/// Checks a passphrase without ever putting it on a command line.
///
/// This used to shell out to `ssh-keygen -P <passphrase>`, which publishes the
/// secret in `/proc/<pid>/cmdline` where every process on the machine can read
/// it. `ssh_key` decrypts in-process instead.
pub fn validate_private_key_passphrase(path: &Path, passphrase: &str) -> Result<(), String> {
    let key = PrivateKey::read_openssh_file(path)
        .map_err(|error| format!("Could not read the private key: {error}"))?;
    if !key.is_encrypted() {
        return Ok(());
    }
    key.decrypt(passphrase)
        .map(|_| ())
        .map_err(|_| "The private key passphrase is incorrect".to_string())
}

/// Derives the public key for a private key held in the Heminus vault.
///
/// Kept in-process for the same reason as the passphrase check above.
fn derive_public_key(path: &Path, passphrase: Option<&str>) -> Result<String, String> {
    let key = PrivateKey::read_openssh_file(path)
        .map_err(|error| format!("Could not read the private key: {error}"))?;
    let key = if key.is_encrypted() {
        let passphrase =
            passphrase.ok_or_else(|| "The private key passphrase is required".to_string())?;
        key.decrypt(passphrase)
            .map_err(|_| "The private key passphrase is incorrect".to_string())?
    } else {
        key
    };
    key.public_key()
        .to_openssh()
        .map(|value| format!("{}\n", value.trim()))
        .map_err(|error| format!("Could not derive the public key: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The passphrase used to be passed as `ssh-keygen -P <secret>`, which puts
    /// it in `/proc/<pid>/cmdline` for every process on the machine to read.
    #[test]
    fn passphrase_checks_and_public_keys_stay_in_process() {
        let directory = std::env::temp_dir().join(format!("heminus-inproc-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("id_ed25519");
        generate_ed25519_files(&path, "in-process", Some("correct horse")).unwrap();
        // Remove the generated .pub so the public key has to be derived.
        fs::remove_file(public_key_path(&path)).unwrap();

        assert!(validate_private_key_passphrase(&path, "correct horse").is_ok());
        assert!(validate_private_key_passphrase(&path, "wrong").is_err());

        let public = derive_public_key(&path, Some("correct horse")).unwrap();
        assert!(public.starts_with("ssh-ed25519 "));
        assert!(public.ends_with('\n'));
        assert!(
            derive_public_key(&path, Some("wrong")).is_err(),
            "a bad passphrase must not yield a public key"
        );
        assert!(
            derive_public_key(&path, None).is_err(),
            "an encrypted key needs its passphrase"
        );

        remove_key_files(&path);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unencrypted_keys_validate_without_a_passphrase() {
        let directory = std::env::temp_dir().join(format!("heminus-plain-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("id_ed25519");
        generate_ed25519_files(&path, "plain", None).unwrap();

        assert!(validate_private_key_passphrase(&path, "").is_ok());
        assert!(derive_public_key(&path, None).unwrap().starts_with("ssh-ed25519 "));

        remove_key_files(&path);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn key_file_names_cannot_escape_ssh_directory() {
        let keys = Path::new("/tmp/heminus-keys");
        assert!(private_key_path(keys, "../id_ed25519").is_err());
        assert!(private_key_path(keys, "/tmp/id_ed25519").is_err());
    }

    #[test]
    fn generated_ed25519_files_are_openssh_compatible() {
        let directory = std::env::temp_dir().join(format!("heminus-key-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("id_ed25519");
        generate_ed25519_files(&path, "heminus-test", Some("correct horse battery staple"))
            .unwrap();
        let private_key = PrivateKey::read_openssh_file(&path).unwrap();
        assert!(private_key.is_encrypted());
        assert!(private_key.decrypt("correct horse battery staple").is_ok());
        assert!(validate_private_key_passphrase(&path, "correct horse battery staple").is_ok());
        assert!(validate_private_key_passphrase(&path, "wrong passphrase").is_err());
        assert!(
            fs::read_to_string(public_key_path(&path))
                .unwrap()
                .ends_with(" heminus-test\n")
        );
        remove_key_files(&path);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn imported_openssh_keys_are_inspected_without_mutating_the_source() {
        let directory =
            std::env::temp_dir().join(format!("heminus-import-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("imported.pem");
        generate_ed25519_files(&path, "import-test", None).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        let inspected = inspect_private_key(&path, &directory).unwrap();
        assert_eq!(inspected.format, "OpenSSH");
        assert!(!inspected.encrypted);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }

        remove_key_files(&path);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn imported_rsa_pem_keys_are_supported() {
        let directory = std::env::temp_dir().join(format!("heminus-pem-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("legacy.pem");
        let output = Command::new(crate::platform::ssh_keygen_executable().unwrap())
            .args(["-q", "-t", "rsa", "-b", "2048", "-m", "PEM", "-N", ""])
            .arg("-f")
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());

        let inspected = inspect_private_key(&path, &directory).unwrap();
        assert_eq!(inspected.format, "RSA PEM");
        assert!(!inspected.encrypted);

        remove_key_files(&path);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn public_keys_and_incomplete_pem_files_are_rejected() {
        let directory =
            std::env::temp_dir().join(format!("heminus-reject-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let public = directory.join("id.pub");
        fs::write(&public, "ssh-ed25519 AAAA test\n").unwrap();
        assert!(inspect_private_key(&public, &directory).is_err());

        let incomplete = directory.join("broken.pem");
        fs::write(&incomplete, "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n").unwrap();
        assert!(inspect_private_key(&incomplete, &directory).is_err());

        fs::remove_file(public).unwrap();
        fs::remove_file(incomplete).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
