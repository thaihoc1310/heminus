use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use heminus_domain::{Host, IdentityKind};

#[derive(Clone, Debug)]
pub struct SshRuntime {
    root: PathBuf,
    known_hosts: PathBuf,
    global_known_hosts: PathBuf,
    keys: PathBuf,
}

impl SshRuntime {
    pub fn initialize() -> Result<Self, String> {
        let project_dirs = ProjectDirs::from("app", "heminus", "Heminus")
            .ok_or_else(|| "Could not resolve the Heminus data directory".to_string())?;
        Self::initialize_at(project_dirs.data_local_dir().join("ssh"))
    }

    fn initialize_at(root: PathBuf) -> Result<Self, String> {
        let runtime = Self {
            known_hosts: root.join("known_hosts"),
            global_known_hosts: root.join("global_known_hosts"),
            keys: root.join("keys"),
            root,
        };
        fs::create_dir_all(&runtime.keys)
            .map_err(|error| format!("Could not create the Heminus SSH vault: {error}"))?;
        secure_directory(&runtime.root)?;
        secure_directory(&runtime.keys)?;
        ensure_private_file(&runtime.known_hosts, b"")?;
        ensure_private_file(&runtime.global_known_hosts, b"")?;
        Ok(runtime)
    }

    pub fn arguments(&self) -> Vec<OsString> {
        self.arguments_with_host_key_policy("ask")
    }

    pub fn sftp_arguments(&self) -> Vec<OsString> {
        self.arguments_with_host_key_policy("accept-new")
    }

    fn arguments_with_host_key_policy(&self, policy: &str) -> Vec<OsString> {
        [
            OsString::from("-F"),
            OsString::from("none"),
            OsString::from("-o"),
            OsString::from(format!(
                "UserKnownHostsFile={}",
                self.known_hosts.to_string_lossy()
            )),
            OsString::from("-o"),
            OsString::from(format!(
                "GlobalKnownHostsFile={}",
                self.global_known_hosts.to_string_lossy()
            )),
            OsString::from("-o"),
            OsString::from(format!("StrictHostKeyChecking={policy}")),
            OsString::from("-o"),
            OsString::from("HashKnownHosts=yes"),
            OsString::from("-o"),
            OsString::from("IdentitiesOnly=yes"),
            OsString::from("-o"),
            OsString::from("IdentityAgent=none"),
            OsString::from("-o"),
            OsString::from("IdentityFile=none"),
            OsString::from("-o"),
            OsString::from("AddKeysToAgent=no"),
        ]
        .into_iter()
        .collect()
    }

    pub fn host_arguments(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
    ) -> Result<Vec<OsString>, String> {
        self.host_arguments_with_policy(database, host, "ask")
    }

    pub fn sftp_host_arguments(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
    ) -> Result<Vec<OsString>, String> {
        self.host_arguments_with_policy(database, host, "accept-new")
    }

    fn host_arguments_with_policy(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
        host_key_policy: &str,
    ) -> Result<Vec<OsString>, String> {
        let mut arguments = Vec::new();
        for variable in &host.environment {
            let value = variable.value.replace(['\r', '\n'], " ");
            let quoted = value.replace('\\', "\\\\").replace('"', "\\\"");
            arguments.push(OsString::from("-o"));
            arguments.push(OsString::from(format!(
                "SetEnv={}=\"{quoted}\"",
                variable.name
            )));
        }

        let Some(jump_host_id) = host.jump_host_id else {
            return Ok(arguments);
        };
        let jump_host = database
            .find_host(jump_host_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "The selected jump host no longer exists".to_string())?;
        let jump_identity = jump_host
            .identity_id
            .map(|id| {
                database
                    .find_identity(id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "The jump host identity no longer exists".to_string())
            })
            .transpose()?;
        let jump_identity = jump_identity.ok_or_else(|| {
            "Save a password or key identity on the jump host before using it in a chain"
                .to_string()
        })?;
        let username = jump_identity
            .username
            .as_deref()
            .unwrap_or(&jump_host.username);
        let mut proxy_parts = vec![OsString::from("ssh")];
        proxy_parts.extend(self.arguments_with_host_key_policy(host_key_policy));
        match jump_identity.kind {
            IdentityKind::KeyFile => {
                let key_path = jump_identity
                    .key_path
                    .as_deref()
                    .ok_or_else(|| "The jump host key is missing".to_string())?;
                proxy_parts.extend([
                    OsString::from("-o"),
                    OsString::from(if jump_identity.secret_stored {
                        "BatchMode=no"
                    } else {
                        "BatchMode=yes"
                    }),
                    OsString::from("-i"),
                    OsString::from(key_path),
                ]);
            }
            IdentityKind::Password => {
                if !jump_identity.secret_stored {
                    return Err("The jump host password has not been saved".to_string());
                }
                proxy_parts.extend([
                    OsString::from("-o"),
                    OsString::from("BatchMode=no"),
                    OsString::from("-o"),
                    OsString::from("PreferredAuthentications=keyboard-interactive,password"),
                    OsString::from("-o"),
                    OsString::from("PubkeyAuthentication=no"),
                    OsString::from("-o"),
                    OsString::from("NumberOfPasswordPrompts=1"),
                ]);
            }
            IdentityKind::Agent => {
                return Err("System SSH-agent identities are not supported".to_string());
            }
        }
        proxy_parts.extend([
            OsString::from("-p"),
            OsString::from(jump_host.port.to_string()),
            OsString::from("-W"),
            OsString::from("%h:%p"),
            OsString::from("--"),
            OsString::from(format!("{username}@{}", effective_address(&jump_host))),
        ]);
        let proxy_command = proxy_parts
            .iter()
            .map(|part| shell_quote(&part.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ");
        arguments.push(OsString::from("-o"));
        arguments.push(OsString::from(format!("ProxyCommand={proxy_command}")));
        Ok(arguments)
    }

    pub fn known_hosts_path(&self) -> &Path {
        &self.known_hosts
    }

    pub fn keys_directory(&self) -> &Path {
        &self.keys
    }

    pub fn owns_key(&self, path: &Path) -> bool {
        path.canonicalize()
            .ok()
            .is_some_and(|path| path.starts_with(&self.keys))
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn effective_address(host: &Host) -> &str {
    host.tags
        .iter()
        .find_map(|tag| tag.strip_prefix("hostname:"))
        .unwrap_or(&host.address)
}

pub fn migrate_legacy_hosts(database: &heminus_storage::Database) -> Result<usize, String> {
    let mut migrated = 0;
    for mut host in database
        .list_hosts(None)
        .map_err(|error| error.to_string())?
    {
        let hostname = host
            .tags
            .iter()
            .find_map(|tag| tag.strip_prefix("hostname:"))
            .map(str::to_owned);
        let has_legacy_metadata = hostname.is_some()
            || host.tags.iter().any(|tag| {
                tag == "openssh"
                    || tag.starts_with("proxyjump:")
                    || tag.starts_with("proxycommand:")
            });
        if !has_legacy_metadata {
            continue;
        }
        if let Some(hostname) = hostname {
            host.address = hostname;
        }
        host.tags.retain(|tag| {
            tag != "openssh"
                && !tag.starts_with("hostname:")
                && !tag.starts_with("proxyjump:")
                && !tag.starts_with("proxycommand:")
        });
        database
            .save_host(&host)
            .map_err(|error| error.to_string())?;
        migrated += 1;
    }
    Ok(migrated)
}

fn ensure_private_file(path: &Path, initial_contents: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(initial_contents)
                .map_err(|error| format!("Could not initialize {}: {error}", path.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!("Could not initialize {}: {error}", path.display()));
        }
    }
    secure_file(path)
}

fn secure_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not secure {}: {error}", path.display()))?;
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use heminus_domain::{EnvironmentVariable, Identity};
    use uuid::Uuid;

    #[test]
    fn runtime_uses_only_app_private_ssh_files() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let arguments = runtime
            .arguments()
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(arguments.windows(2).any(|pair| pair == ["-F", "none"]));
        assert!(arguments.iter().any(|value| {
            value
                == &format!(
                    "UserKnownHostsFile={}",
                    runtime.known_hosts.to_string_lossy()
                )
        }));
        assert!(arguments.contains(&"IdentityAgent=none".to_string()));
        assert!(arguments.contains(&"IdentityFile=none".to_string()));
        assert!(runtime.keys_directory().is_dir());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sftp_uses_tofu_only_inside_the_app_known_host_vault() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let arguments = runtime
            .sftp_arguments()
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(arguments.contains(&"StrictHostKeyChecking=accept-new".to_string()));
        assert!(arguments.iter().any(|value| {
            value
                == &format!(
                    "UserKnownHostsFile={}",
                    runtime.known_hosts.to_string_lossy()
                )
        }));
        assert!(arguments.contains(&"IdentityAgent=none".to_string()));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_options_use_app_owned_environment_and_jump_host() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut identity = Identity::new("Gateway key", IdentityKind::KeyFile);
        identity.key_path = Some(
            runtime
                .keys_directory()
                .join("gateway")
                .display()
                .to_string(),
        );
        database.save_identity(&identity).unwrap();
        let mut jump_host = Host::new("Gateway", "192.0.2.10", "bastion");
        jump_host.identity_id = Some(identity.id);
        database.save_host(&jump_host).unwrap();
        let mut target = Host::new("Private node", "10.0.0.20", "deploy");
        target.jump_host_id = Some(jump_host.id);
        target.environment.push(EnvironmentVariable {
            name: "LANG".into(),
            value: "en_US.UTF-8".into(),
        });

        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"SetEnv=LANG=\"en_US.UTF-8\"".to_string()));
        let proxy = arguments
            .iter()
            .find(|argument| argument.starts_with("ProxyCommand="))
            .unwrap();
        assert!(proxy.contains("'-F' 'none'"));
        assert!(proxy.contains("'bastion@192.0.2.10'"));
        assert!(proxy.contains("'IdentityAgent=none'"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_chain_supports_a_saved_password_on_the_jump_host() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut identity = Identity::new("Gateway password", IdentityKind::Password);
        identity.secret_stored = true;
        database.save_identity(&identity).unwrap();
        let mut jump_host = Host::new("Gateway", "192.0.2.10", "bastion");
        jump_host.identity_id = Some(identity.id);
        database.save_host(&jump_host).unwrap();
        let mut target = Host::new("Private node", "10.0.0.20", "deploy");
        target.jump_host_id = Some(jump_host.id);

        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let proxy = arguments
            .iter()
            .find(|argument| argument.starts_with("ProxyCommand="))
            .unwrap();
        assert!(proxy.contains("'BatchMode=no'"));
        assert!(proxy.contains("'PreferredAuthentications=keyboard-interactive,password'"));
        assert!(proxy.contains("'PubkeyAuthentication=no'"));
        assert!(proxy.contains("'bastion@192.0.2.10'"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_chain_enables_askpass_for_a_protected_jump_key() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let key_path = runtime.keys_directory().join("gateway");
        let mut identity = Identity::new("Protected gateway key", IdentityKind::KeyFile);
        identity.key_path = Some(key_path.display().to_string());
        identity.secret_stored = true;
        database.save_identity(&identity).unwrap();
        let mut jump_host = Host::new("Gateway", "192.0.2.10", "bastion");
        jump_host.identity_id = Some(identity.id);
        database.save_host(&jump_host).unwrap();
        let mut target = Host::new("Private node", "10.0.0.20", "deploy");
        target.jump_host_id = Some(jump_host.id);

        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let proxy = arguments
            .iter()
            .find(|argument| argument.starts_with("ProxyCommand="))
            .unwrap();
        assert!(proxy.contains("'BatchMode=no'"));
        assert!(proxy.contains(&format!("'{}'", key_path.display())));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_imported_hosts_resolve_without_system_config() {
        let mut host = Host::new("Gateway", "gateway-alias", "deploy");
        host.tags.push("hostname:10.0.0.42".into());
        assert_eq!(effective_address(&host), "10.0.0.42");
    }

    #[test]
    fn legacy_import_metadata_is_folded_into_app_owned_hosts() {
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut host = Host::new("Gateway", "gateway-alias", "deploy");
        host.tags.extend([
            "openssh".into(),
            "hostname:10.0.0.42".into(),
            "proxyjump:bastion".into(),
            "production".into(),
        ]);
        database.save_host(&host).unwrap();

        assert_eq!(migrate_legacy_hosts(&database).unwrap(), 1);
        let migrated = database.list_hosts(None).unwrap().pop().unwrap();
        assert_eq!(migrated.address, "10.0.0.42");
        assert_eq!(migrated.tags, vec!["production"]);
    }
}
