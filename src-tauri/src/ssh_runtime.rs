use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use heminus_domain::{Host, IdentityKind};

#[derive(Debug)]
pub struct SshRuntime {
    root: PathBuf,
    known_hosts: PathBuf,
    global_known_hosts: PathBuf,
    keys: PathBuf,
    connections: PathBuf,
}

#[derive(Default)]
pub struct ConnectionArtifacts {
    config: Option<PathBuf>,
}

impl Drop for ConnectionArtifacts {
    fn drop(&mut self) {
        if let Some(path) = self.config.take() {
            let _ = fs::remove_file(path);
        }
    }
}

impl SshRuntime {
    pub fn initialize() -> Result<Self, String> {
        let project_dirs = ProjectDirs::from("app", "heminus", "Heminus")
            .ok_or_else(|| "Could not resolve the Heminus data directory".to_string())?;
        Self::initialize_at(project_dirs.data_local_dir().join("ssh"))
    }

    fn initialize_at(root: PathBuf) -> Result<Self, String> {
        let connections_root = root.join("connections");
        let runtime = Self {
            known_hosts: root.join("known_hosts"),
            global_known_hosts: root.join("global_known_hosts"),
            keys: root.join("keys"),
            connections: connections_root.join(uuid::Uuid::new_v4().to_string()),
            root,
        };
        fs::create_dir_all(&runtime.keys)
            .map_err(|error| format!("Could not create the Heminus SSH vault: {error}"))?;
        fs::create_dir_all(&runtime.connections).map_err(|error| {
            format!("Could not create the Heminus SSH connection directory: {error}")
        })?;
        secure_directory(&runtime.root)?;
        secure_directory(&runtime.keys)?;
        secure_directory(&connections_root)?;
        secure_directory(&runtime.connections)?;
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
        let jump_hosts = jump_hosts(database, host)?;
        let mut arguments = if jump_hosts.is_empty() {
            vec![OsString::from("-F"), OsString::from("none")]
        } else {
            let (config_path, aliases) =
                self.write_jump_host_config(database, &jump_hosts, host_key_policy)?;
            vec![
                OsString::from("-F"),
                config_path.into_os_string(),
                OsString::from("-o"),
                OsString::from(format!("ProxyJump={}", aliases.join(","))),
            ]
        };
        for variable in &host.environment {
            let value = variable.value.replace(['\r', '\n'], " ");
            let quoted = value.replace('\\', "\\\\").replace('"', "\\\"");
            arguments.push(OsString::from("-o"));
            arguments.push(OsString::from(format!(
                "SetEnv={}=\"{quoted}\"",
                variable.name
            )));
        }

        Ok(arguments)
    }

    fn write_jump_host_config(
        &self,
        database: &heminus_storage::Database,
        jump_hosts: &[Host],
        host_key_policy: &str,
    ) -> Result<(PathBuf, Vec<String>), String> {
        let mut config = String::new();
        let mut aliases = Vec::with_capacity(jump_hosts.len());
        for (index, jump_host) in jump_hosts.iter().enumerate() {
            let jump_identity = jump_host
                .identity_id
                .map(|id| {
                    database
                        .find_identity(id)
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| {
                            format!("The identity for jump host {} no longer exists", index + 1)
                        })
                })
                .transpose()?
                .ok_or_else(|| {
                    format!(
                        "Save a password or key identity on jump host {} before using it in a chain",
                        index + 1
                    )
                })?;
            let username = jump_identity
                .username
                .as_deref()
                .unwrap_or(&jump_host.username);
            let alias = format!("heminus-jump-{}", index + 1);
            aliases.push(alias.clone());
            config.push_str(&format!("Host {alias}\n"));
            push_config_value(&mut config, "HostName", effective_address(jump_host))?;
            push_config_value(&mut config, "User", username)?;
            config.push_str(&format!("  Port {}\n", jump_host.port));
            push_config_value(
                &mut config,
                "UserKnownHostsFile",
                &self.known_hosts.to_string_lossy(),
            )?;
            push_config_value(
                &mut config,
                "GlobalKnownHostsFile",
                &self.global_known_hosts.to_string_lossy(),
            )?;
            config.push_str(&format!(
                "  StrictHostKeyChecking {host_key_policy}\n  HashKnownHosts yes\n  IdentitiesOnly yes\n  IdentityAgent none\n  IdentityFile none\n  AddKeysToAgent no\n"
            ));
            match jump_identity.kind {
                IdentityKind::KeyFile => {
                    let key_path = jump_identity
                        .key_path
                        .as_deref()
                        .ok_or_else(|| format!("The key for jump host {} is missing", index + 1))?;
                    config.push_str(if jump_identity.secret_stored {
                        "  BatchMode no\n"
                    } else {
                        "  BatchMode yes\n"
                    });
                    push_config_value(&mut config, "IdentityFile", key_path)?;
                }
                IdentityKind::Password => {
                    if !jump_identity.secret_stored {
                        return Err(format!(
                            "The password for jump host {} has not been saved",
                            index + 1
                        ));
                    }
                    config.push_str(
                        "  BatchMode no\n  PreferredAuthentications keyboard-interactive,password\n  PubkeyAuthentication no\n  NumberOfPasswordPrompts 1\n",
                    );
                }
                IdentityKind::Agent => {
                    return Err("System SSH-agent identities are not supported".to_string());
                }
            }
            config.push('\n');
        }

        let config_path = self
            .connections
            .join(format!("{}.conf", uuid::Uuid::new_v4()));
        fs::write(&config_path, config).map_err(|error| {
            format!(
                "Could not write the Heminus SSH connection config {}: {error}",
                config_path.display()
            )
        })?;
        secure_file(&config_path)?;
        Ok((config_path, aliases))
    }

    pub fn known_hosts_path(&self) -> &Path {
        &self.known_hosts
    }

    pub fn keys_directory(&self) -> &Path {
        &self.keys
    }

    pub fn owns_key(&self, path: &Path) -> bool {
        match (path.canonicalize(), self.keys.canonicalize()) {
            (Ok(path), Ok(keys)) => path.starts_with(keys),
            _ => false,
        }
    }

    pub fn connection_artifacts(&self, arguments: &[OsString]) -> ConnectionArtifacts {
        let config = arguments.windows(2).find_map(|pair| {
            (pair[0] == "-F" && pair[1] != "none")
                .then(|| PathBuf::from(&pair[1]))
                .filter(|path| path.starts_with(&self.connections))
        });
        ConnectionArtifacts { config }
    }
}

impl Drop for SshRuntime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.connections);
    }
}

fn push_config_value(config: &mut String, name: &str, value: &str) -> Result<(), String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(format!(
            "SSH {name} contains an unsupported control character"
        ));
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    config.push_str(&format!("  {name} \"{escaped}\"\n"));
    Ok(())
}

pub fn effective_address(host: &Host) -> &str {
    host.tags
        .iter()
        .find_map(|tag| tag.strip_prefix("hostname:"))
        .unwrap_or(&host.address)
}

pub(crate) fn jump_hosts(
    database: &heminus_storage::Database,
    host: &Host,
) -> Result<Vec<Host>, String> {
    host.jump_host_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            database
                .find_host(*id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("Jump host {} no longer exists", index + 1))
        })
        .collect()
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
    crate::platform::secure_private_directory(path)
}

fn secure_file(path: &Path) -> Result<(), String> {
    crate::platform::secure_private_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use heminus_domain::{EnvironmentVariable, Identity};
    use uuid::Uuid;

    fn jump_config_path(arguments: &[String]) -> &Path {
        let index = arguments
            .iter()
            .position(|argument| argument == "-F")
            .expect("host arguments should select an SSH config");
        Path::new(&arguments[index + 1])
    }

    fn jump_config(arguments: &[String]) -> String {
        fs::read_to_string(jump_config_path(arguments))
            .expect("jump-host config should be readable")
    }

    #[test]
    fn runtime_uses_only_app_private_ssh_files() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let arguments = runtime
            .arguments()
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(!arguments.iter().any(|argument| argument == "-F"));
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

        let database = heminus_storage::Database::in_memory().unwrap();
        let host = Host::new("Server", "192.0.2.20", "deploy");
        let host_arguments = runtime
            .host_arguments(&database, &host)
            .unwrap()
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(host_arguments.windows(2).any(|pair| pair == ["-F", "none"]));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owns_only_canonicalized_keys_inside_the_vault() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let managed_key = runtime.keys_directory().join("managed-key");
        let external_key = root.join("external-key");
        fs::write(&managed_key, b"managed").unwrap();
        fs::write(&external_key, b"external").unwrap();

        assert!(runtime.owns_key(&managed_key));
        assert!(!runtime.owns_key(&external_key));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connection_config_is_removed_with_its_session_artifacts() {
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
        target.jump_host_ids = vec![jump_host.id];

        let arguments = runtime.host_arguments(&database, &target).unwrap();
        let artifacts = runtime.connection_artifacts(&arguments);
        let config = artifacts.config.clone().expect("connection config");
        assert!(config.is_file());
        drop(artifacts);
        assert!(!config.exists());

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
        target.jump_host_ids = vec![jump_host.id];
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
        assert!(arguments.contains(&"ProxyJump=heminus-jump-1".to_string()));
        let config = jump_config(&arguments);
        assert!(config.contains("Host heminus-jump-1"));
        assert!(config.contains("HostName \"192.0.2.10\""));
        assert!(config.contains("User \"bastion\""));
        assert!(config.contains("IdentityAgent none"));
        assert!(!config.contains("ProxyCommand"));
        #[cfg(windows)]
        {
            let output = std::process::Command::new(crate::platform::ssh_executable().unwrap())
                .arg("-G")
                .arg("-F")
                .arg(jump_config_path(&arguments))
                .arg("heminus-jump-1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let parsed = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            assert!(parsed.contains("hostname 192.0.2.10"));
            assert!(parsed.contains("user bastion"));
            assert!(parsed.contains("identityagent none"));
            assert!(parsed.contains("identityfile none"));
            assert!(parsed.contains("gateway"));
        }

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
        target.jump_host_ids = vec![jump_host.id];

        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"ProxyJump=heminus-jump-1".to_string()));
        let config = jump_config(&arguments);
        assert!(config.contains("BatchMode no"));
        assert!(config.contains("PreferredAuthentications keyboard-interactive,password"));
        assert!(config.contains("PubkeyAuthentication no"));
        assert!(config.contains("User \"bastion\""));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_chain_nests_multiple_jump_hosts_in_order() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();

        let mut first_identity = Identity::new("Edge key", IdentityKind::KeyFile);
        first_identity.key_path = Some(runtime.keys_directory().join("edge").display().to_string());
        database.save_identity(&first_identity).unwrap();
        let mut first_jump = Host::new("Edge", "192.0.2.10", "edge");
        first_jump.identity_id = Some(first_identity.id);
        database.save_host(&first_jump).unwrap();

        let mut second_identity = Identity::new("Relay password", IdentityKind::Password);
        second_identity.secret_stored = true;
        database.save_identity(&second_identity).unwrap();
        let mut second_jump = Host::new("Relay", "10.0.0.10", "relay");
        second_jump.identity_id = Some(second_identity.id);
        database.save_host(&second_jump).unwrap();

        let mut target = Host::new("Private node", "10.0.1.20", "deploy");
        target.jump_host_ids = vec![first_jump.id, second_jump.id];
        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"ProxyJump=heminus-jump-1,heminus-jump-2".to_string()));
        let config = jump_config(&arguments);
        let first_position = config.find("HostName \"192.0.2.10\"").unwrap();
        let second_position = config.find("HostName \"10.0.0.10\"").unwrap();
        assert!(first_position < second_position);
        assert!(config.contains("PreferredAuthentications keyboard-interactive,password"));
        assert!(!config.contains("ProxyCommand"));

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
        target.jump_host_ids = vec![jump_host.id];

        let arguments = runtime
            .host_arguments(&database, &target)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let config = jump_config(&arguments);
        assert!(config.contains("BatchMode no"));
        assert!(config.contains("IdentityFile"));
        assert!(config.contains("gateway\""));

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
