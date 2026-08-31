use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use heminus_domain::{Host, HostProxy, IdentityKind};

#[derive(Debug)]
pub struct SshRuntime {
    root: PathBuf,
    known_hosts: PathBuf,
    global_known_hosts: PathBuf,
    keys: PathBuf,
    connections: PathBuf,
}

/// How OpenSSH should treat a host key it has never seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Ask in the terminal, where the person can read the fingerprint.
    Ask,
    /// Pin on first use.
    ///
    /// `SSH_ASKPASS_REQUIRE=force` sends *every* prompt to the Heminus askpass
    /// helper, including the host-key question, so `ask` would leave the
    /// session stuck on a prompt nobody can answer.
    AcceptNew,
}

impl HostKeyPolicy {
    pub const fn for_forced_askpass(forced: bool) -> Self {
        if forced { Self::AcceptNew } else { Self::Ask }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AcceptNew => "accept-new",
        }
    }
}

/// One leg of a connection: a jump host, or the host itself.
///
/// Every leg is a separate OpenSSH process with its own `-E` log, which is what
/// lets the connecting screen show real progress per hop instead of one opaque
/// wait — and keeps the inner hops from spraying debug output into the terminal.
#[derive(Clone, Debug)]
pub struct ConnectionHop {
    pub label: String,
    pub log: PathBuf,
}

/// Per-session scratch files that must not outlive the connection.
#[derive(Default)]
pub struct ConnectionArtifacts {
    config: Option<PathBuf>,
    logs: Vec<PathBuf>,
}

impl ConnectionArtifacts {
    pub fn with_logs(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.logs.extend(paths);
        self
    }
}

impl Drop for ConnectionArtifacts {
    fn drop(&mut self) {
        for path in self.config.take().into_iter().chain(self.logs.drain(..)) {
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
            connections: connections_root.join(format!(
                "{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            )),
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
        remove_abandoned_connection_directories(&connections_root, &runtime.connections);
        Ok(runtime)
    }

    pub fn interactive_arguments(&self, policy: HostKeyPolicy) -> Vec<OsString> {
        self.arguments_with_host_key_policy(policy.as_str())
    }

    pub fn sftp_arguments(&self) -> Vec<OsString> {
        self.arguments_with_host_key_policy(HostKeyPolicy::AcceptNew.as_str())
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

    /// Builds the arguments for an interactive session plus one log per hop.
    ///
    /// The last hop is the host itself; its log belongs on the caller's own
    /// `ssh -E`, while the inner hops carry theirs inside the generated config.
    pub fn interactive_connection(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
        policy: HostKeyPolicy,
    ) -> Result<(Vec<OsString>, Vec<ConnectionHop>), String> {
        let mut hops = jump_hosts(database, host)?
            .iter()
            .map(|jump_host| jump_host.label.clone())
            .chain(std::iter::once(host.label.clone()))
            .map(|label| {
                Ok(ConnectionHop {
                    label,
                    log: self.connection_log_path()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let arguments =
            self.host_arguments_with_policy(database, host, policy.as_str(), Some(&mut hops))?;
        Ok((arguments, hops))
    }

    pub fn sftp_host_arguments(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
    ) -> Result<Vec<OsString>, String> {
        self.host_arguments_with_policy(database, host, HostKeyPolicy::AcceptNew.as_str(), None)
    }

    /// Per-host arguments for a connection that does not stream a hop log.
    pub fn host_arguments(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
        policy: HostKeyPolicy,
    ) -> Result<Vec<OsString>, String> {
        self.host_arguments_with_policy(database, host, policy.as_str(), None)
    }

    fn host_arguments_with_policy(
        &self,
        database: &heminus_storage::Database,
        host: &Host,
        host_key_policy: &str,
        hops: Option<&mut Vec<ConnectionHop>>,
    ) -> Result<Vec<OsString>, String> {
        let jump_hosts = jump_hosts(database, host)?;
        let hop_logs = hops.map(|hops| {
            hops.iter()
                .map(|hop| hop.log.clone())
                .collect::<Vec<PathBuf>>()
        });
        // A proxy wraps the very first TCP hop, so a chain tunnels its entry
        // jump host and a direct connection tunnels the host itself.
        let target_proxy = jump_hosts
            .is_empty()
            .then_some(host.proxy.as_ref())
            .flatten();
        let mut arguments = if jump_hosts.is_empty() {
            vec![OsString::from("-F"), OsString::from("none")]
        } else {
            let (config_path, entry) = self.write_jump_host_config(
                database,
                &jump_hosts,
                host_key_policy,
                host.proxy.as_ref().map(|proxy| (proxy, host.id)),
                hop_logs.as_deref(),
            )?;
            // Heminus builds the chain itself rather than using ProxyJump, so
            // each hop can be given its own log file. OpenSSH's own expansion
            // would instead pass -v down and leak the inner hops' debug output
            // into the terminal.
            let hop_command = ssh_hop_command(
                &config_path,
                &entry,
                hop_logs
                    .as_ref()
                    .and_then(|logs| logs.get(jump_hosts.len() - 1)),
            )?;
            vec![
                OsString::from("-F"),
                config_path.into_os_string(),
                OsString::from("-o"),
                OsString::from(format!("ProxyCommand={hop_command}")),
            ]
        };
        if let Some(proxy) = target_proxy {
            arguments.push(OsString::from("-o"));
            arguments.push(OsString::from(format!(
                "ProxyCommand={}",
                proxy_command(proxy, host.id)?
            )));
        }
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

    /// Writes the chain config and returns it with the alias of the last hop.
    fn write_jump_host_config(
        &self,
        database: &heminus_storage::Database,
        jump_hosts: &[Host],
        host_key_policy: &str,
        entry_proxy: Option<(&HostProxy, uuid::Uuid)>,
        hop_logs: Option<&[PathBuf]>,
    ) -> Result<(PathBuf, String), String> {
        let config_path = self
            .connections
            .join(format!("{}.conf", uuid::Uuid::new_v4()));
        let mut config = String::new();
        let mut aliases: Vec<String> = Vec::with_capacity(jump_hosts.len());
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
            // ssh_config takes the rest of the line verbatim for ProxyCommand,
            // so neither the encoded proxy settings nor the nested SSH command
            // needs escaping here.
            if index == 0 {
                if let Some((proxy, host_id)) = entry_proxy {
                    config.push_str(&format!(
                        "  ProxyCommand {}\n",
                        proxy_command(proxy, host_id)?
                    ));
                }
            } else {
                let previous = &aliases[index - 1];
                let hop_command = ssh_hop_command(
                    &config_path,
                    previous,
                    hop_logs.and_then(|logs| logs.get(index - 1)),
                )?;
                config.push_str(&format!("  ProxyCommand {hop_command}\n"));
            }
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

        fs::write(&config_path, config).map_err(|error| {
            format!(
                "Could not write the Heminus SSH connection config {}: {error}",
                config_path.display()
            )
        })?;
        secure_file(&config_path)?;
        let entry = aliases
            .pop()
            .ok_or_else(|| "The connection chain is empty".to_string())?;
        Ok((config_path, entry))
    }

    pub fn known_hosts_path(&self) -> &Path {
        &self.known_hosts
    }

    pub fn keys_directory(&self) -> &Path {
        &self.keys
    }

    /// A private directory for short-lived files that hold key material.
    ///
    /// Validation used to stage decrypted keys in the shared system temp
    /// directory; this keeps them inside the 0700 vault instead.
    pub fn scratch_directory(&self) -> Result<PathBuf, String> {
        let path = self.root.join("tmp");
        fs::create_dir_all(&path)
            .map_err(|error| format!("Could not create the Heminus scratch directory: {error}"))?;
        secure_directory(&path)?;
        Ok(path)
    }

    pub fn owns_key(&self, path: &Path) -> bool {
        match (path.canonicalize(), self.keys.canonicalize()) {
            (Ok(path), Ok(keys)) => path.starts_with(keys),
            _ => false,
        }
    }

    /// Creates the private file OpenSSH `-E` appends its connection log to.
    fn connection_log_path(&self) -> Result<PathBuf, String> {
        let path = self
            .connections
            .join(format!("{}.log", uuid::Uuid::new_v4()));
        ensure_private_file(&path, b"")?;
        Ok(path)
    }

    pub fn connection_artifacts(&self, arguments: &[OsString]) -> ConnectionArtifacts {
        let config = arguments.windows(2).find_map(|pair| {
            (pair[0] == "-F" && pair[1] != "none")
                .then(|| PathBuf::from(&pair[1]))
                .filter(|path| path.starts_with(&self.connections))
        });
        ConnectionArtifacts {
            config,
            logs: Vec::new(),
        }
    }
}

impl Drop for SshRuntime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.connections);
    }
}

/// Clears connection directories left behind by runs that did not shut down.
///
/// `Drop` only removes the current run's directory, and it does not run when the
/// process is killed or aborts, so these accumulated one per launch forever.
/// The directory name carries the owning PID; anything whose process is gone is
/// safe to delete, as is anything in the pre-PID naming scheme.
fn remove_abandoned_connection_directories(connections_root: &Path, current: &Path) {
    let Ok(entries) = fs::read_dir(connections_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current || !path.is_dir() {
            continue;
        }
        let owner = entry
            .file_name()
            .to_str()
            .and_then(|name| name.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<u32>().ok());
        if owner.is_none_or(|pid| !crate::platform::process_is_running(pid)) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Builds the nested `ssh -W` command that reaches one hop of a chain.
///
/// Each hop logs to its own file so the connecting screen can attribute
/// progress, and so no hop writes debug output onto the session's terminal.
fn ssh_hop_command(config: &Path, alias: &str, log: Option<&PathBuf>) -> Result<String, String> {
    let ssh = crate::platform::ssh_executable()?;
    let ssh = quoted_command_word(&ssh.to_string_lossy())?;
    let config = quoted_command_word(&config.to_string_lossy())?;
    let logging = match log {
        Some(log) => format!(
            " -o LogLevel=DEBUG1 -E {}",
            quoted_command_word(&log.to_string_lossy())?
        ),
        None => String::new(),
    };
    Ok(format!("{ssh} -F {config}{logging} -W \"[%h]:%p\" {alias}"))
}

/// Wraps a path for the shell OpenSSH runs `ProxyCommand` through.
///
/// Double quotes work in both `/bin/sh` and `cmd.exe`; a path that contains one
/// cannot be expressed safely, so it is rejected rather than mis-quoted.
fn quoted_command_word(value: &str) -> Result<String, String> {
    if value.contains(['"', '\r', '\n']) {
        return Err(format!(
            "Heminus cannot build an SSH command from a path containing quotes: {value}"
        ));
    }
    Ok(format!("\"{value}\""))
}

/// Builds the `ProxyCommand` that re-executes Heminus as a proxy client.
///
/// The settings travel as one base64url token so no quoting, password, or
/// shell metacharacter ever reaches the command line.
fn proxy_command(proxy: &HostProxy, host_id: uuid::Uuid) -> Result<String, String> {
    proxy.validate().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not resolve the Heminus executable: {error}"))?;
    let executable = quoted_command_word(&executable.to_string_lossy())?;
    let spec = crate::proxy::ProxySpec::from_host_proxy(proxy, host_id).encode()?;
    Ok(format!(
        "{executable} {} {spec} %h %p",
        crate::proxy::connect_flag()
    ))
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

    fn proxy_command_argument(arguments: &[String]) -> &str {
        arguments
            .iter()
            .find_map(|argument| argument.strip_prefix("ProxyCommand="))
            .expect("a chained host should reach its entry hop through a command")
    }

    #[test]
    fn runtime_uses_only_app_private_ssh_files() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let arguments = runtime
            .interactive_arguments(HostKeyPolicy::Ask)
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
            .host_arguments(&database, &host, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(host_arguments.windows(2).any(|pair| pair == ["-F", "none"]));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_clears_connection_directories_from_runs_that_never_shut_down() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let connections_root = root.join("connections");
        fs::create_dir_all(&connections_root).unwrap();
        // A crashed run leaves its directory behind; the PID is long gone.
        let abandoned = connections_root.join("999999-abcdef");
        fs::create_dir_all(&abandoned).unwrap();
        fs::write(abandoned.join("session.conf"), b"Host x").unwrap();
        // Directories from before the PID naming scheme are also abandoned.
        let legacy = connections_root.join(Uuid::new_v4().to_string());
        fs::create_dir_all(&legacy).unwrap();
        // A directory owned by a live process must survive.
        let live = connections_root.join(format!("{}-other", std::process::id()));
        fs::create_dir_all(&live).unwrap();

        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();

        assert!(!abandoned.exists(), "a dead run's config must be removed");
        assert!(!legacy.exists(), "legacy directories must be removed");
        assert!(
            live.exists(),
            "another live window's directory must survive"
        );
        assert!(runtime.connections.is_dir());

        drop(runtime);
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

        let arguments = runtime
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
            .unwrap();
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
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"SetEnv=LANG=\"en_US.UTF-8\"".to_string()));
        assert!(proxy_command_argument(&arguments).contains("heminus-jump-1"));
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
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(proxy_command_argument(&arguments).contains("heminus-jump-1"));
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
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(proxy_command_argument(&arguments).contains("heminus-jump-2"));
        let config = jump_config(&arguments);
        let first_position = config.find("HostName \"192.0.2.10\"").unwrap();
        let second_position = config.find("HostName \"10.0.0.10\"").unwrap();
        assert!(first_position < second_position);
        assert!(config.contains("PreferredAuthentications keyboard-interactive,password"));
        // The second hop reaches the first through a nested SSH command.
        assert!(config.contains("-W \"[%h]:%p\" heminus-jump-1"));

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
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
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
    fn a_direct_host_tunnels_through_its_proxy_command() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut host = Host::new("Behind a proxy", "10.0.0.20", "deploy");
        host.proxy = Some(heminus_domain::HostProxy {
            kind: heminus_domain::ProxyKind::Socks5,
            hostname: "proxy.example".into(),
            port: 1080,
            username: Some("agent".into()),
            secret_stored: true,
        });

        let arguments = runtime
            .host_arguments(&database, &host, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let proxy_command = arguments
            .iter()
            .find_map(|argument| argument.strip_prefix("ProxyCommand="))
            .expect("a direct host should proxy through Heminus");
        assert!(proxy_command.contains(crate::proxy::connect_flag()));
        assert!(proxy_command.ends_with(" %h %p"));
        assert!(
            !proxy_command.contains("proxy.example"),
            "settings stay encoded"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_chained_host_tunnels_only_its_entry_jump_host() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut identity = Identity::new("Gateway password", IdentityKind::Password);
        identity.secret_stored = true;
        database.save_identity(&identity).unwrap();
        let mut first_jump = Host::new("Edge", "192.0.2.10", "edge");
        first_jump.identity_id = Some(identity.id);
        database.save_host(&first_jump).unwrap();
        let mut second_jump = Host::new("Relay", "10.0.0.10", "relay");
        second_jump.identity_id = Some(identity.id);
        database.save_host(&second_jump).unwrap();
        let mut target = Host::new("Private node", "10.0.1.20", "deploy");
        target.jump_host_ids = vec![first_jump.id, second_jump.id];
        target.proxy = Some(heminus_domain::HostProxy {
            kind: heminus_domain::ProxyKind::Http,
            hostname: "proxy.example".into(),
            port: 3128,
            username: None,
            secret_stored: false,
        });

        let arguments = runtime
            .host_arguments(&database, &target, HostKeyPolicy::Ask)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let entry = proxy_command_argument(&arguments);
        assert!(
            entry.contains("heminus-jump-2") && !entry.contains(crate::proxy::connect_flag()),
            "the target should reach the last hop over SSH, not the proxy: {entry}"
        );
        let config = jump_config(&arguments);
        let first = config.find("Host heminus-jump-1").unwrap();
        let second = config.find("Host heminus-jump-2").unwrap();
        let proxy = config
            .find(crate::proxy::connect_flag())
            .expect("the entry hop tunnels through the proxy");
        assert!(
            (first..second).contains(&proxy),
            "only the entry jump host should tunnel through the proxy"
        );
        assert_eq!(config.matches(crate::proxy::connect_flag()).count(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    /// Guards the bug where OpenSSH's own `ProxyJump` expansion inherited `-v`
    /// and sprayed each hop's debug output over the session's terminal.
    #[test]
    fn every_hop_of_a_chain_logs_to_its_own_file() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut identity = Identity::new("Chain password", IdentityKind::Password);
        identity.secret_stored = true;
        database.save_identity(&identity).unwrap();
        let mut edge = Host::new("Edge", "192.0.2.10", "edge");
        edge.identity_id = Some(identity.id);
        database.save_host(&edge).unwrap();
        let mut relay = Host::new("Relay", "10.0.0.10", "relay");
        relay.identity_id = Some(identity.id);
        database.save_host(&relay).unwrap();
        let mut target = Host::new("Private node", "10.0.1.20", "deploy");
        target.jump_host_ids = vec![edge.id, relay.id];

        let (arguments, hops) = runtime
            .interactive_connection(&database, &target, HostKeyPolicy::AcceptNew)
            .unwrap();
        assert_eq!(
            hops.iter()
                .map(|hop| hop.label.as_str())
                .collect::<Vec<_>>(),
            ["Edge", "Relay", "Private node"],
            "the chain runs entry-first and ends at the host itself"
        );
        let logs = hops
            .iter()
            .map(|hop| &hop.log)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(logs.len(), 3, "each hop needs a log of its own");
        assert!(hops.iter().all(|hop| hop.log.is_file()));

        let arguments = arguments
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let config = jump_config(&arguments);
        // Relay is reached by a nested SSH that logs to Edge's file...
        assert!(config.contains(&format!("-E \"{}\"", hops[0].log.display())));
        // ...and the outermost ProxyCommand reaches Relay, logging to its file.
        let entry = proxy_command_argument(&arguments);
        assert!(entry.contains("heminus-jump-2"), "{entry}");
        assert!(
            entry.contains(&format!("-E \"{}\"", hops[1].log.display())),
            "{entry}"
        );
        // The host's own log is left for the caller to pass as its -E.
        assert!(!config.contains(&hops[2].log.display().to_string()));
        assert!(
            !arguments.iter().any(|argument| argument == "-v"),
            "verbosity must not be inherited by the nested hops"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forced_askpass_pins_unknown_host_keys_instead_of_asking() {
        assert_eq!(
            HostKeyPolicy::for_forced_askpass(true).as_str(),
            "accept-new"
        );
        assert_eq!(HostKeyPolicy::for_forced_askpass(false).as_str(), "ask");

        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let arguments = runtime
            .interactive_arguments(HostKeyPolicy::AcceptNew)
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.contains(&"StrictHostKeyChecking=accept-new".to_string()));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connection_artifacts_remove_the_session_log_too() {
        let root = std::env::temp_dir().join(format!("heminus-runtime-test-{}", Uuid::new_v4()));
        let runtime = SshRuntime::initialize_at(root.clone()).unwrap();
        let log = runtime.connection_log_path().unwrap();
        assert!(log.is_file());
        drop(ConnectionArtifacts::default().with_logs([log.clone()]));
        assert!(!log.exists());

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
