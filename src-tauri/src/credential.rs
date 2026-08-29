use std::io::Write;

use heminus_domain::{Host, Identity, IdentityKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

const SERVICE: &str = "app.heminus.desktop";
const ASKPASS_CANDIDATES_ENV: &str = "HEMINUS_ASKPASS_CANDIDATES";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AskpassCandidate {
    id: Uuid,
    needles: Vec<String>,
    /// Whether this secret is a server-facing password or a local key passphrase.
    ///
    /// A password is meant to be sent to the server, so answering an
    /// unrecognised prompt with one is only as risky as the connection already
    /// is. A key passphrase never leaves the machine, so it must only answer a
    /// prompt that names its own key file.
    #[serde(default)]
    kind: AskpassSecret,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AskpassSecret {
    #[default]
    Password,
    KeyPassphrase,
}

fn account(id: Uuid) -> String {
    format!("identity:{id}")
}

/// Proxy passwords are stored per host, so they never collide with identities.
fn proxy_account(host_id: Uuid) -> String {
    format!("proxy:{host_id}")
}

fn entry(id: Uuid) -> Result<keyring::Entry, String> {
    entry_for(&account(id))
}

fn entry_for(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account)
        .map_err(|error| format!("Could not access the operating-system credential store: {error}"))
}

fn store(entry: &keyring::Entry, secret: String) -> Result<(), String> {
    if secret.is_empty() {
        return Err("Password cannot be empty".into());
    }
    let secret = Zeroizing::new(secret);
    entry
        .set_password(secret.as_str())
        .map_err(|error| format!("Could not save the password in the system keyring: {error}"))
}

fn load(entry: &keyring::Entry) -> Result<Zeroizing<String>, String> {
    entry
        .get_password()
        .map(Zeroizing::new)
        .map_err(|error| format!("Could not read the password from the system keyring: {error}"))
}

fn discard(entry: keyring::Entry) -> Result<(), String> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "Could not remove the password from the system keyring: {error}"
        )),
    }
}

pub fn set(id: Uuid, secret: String) -> Result<(), String> {
    store(&entry(id)?, secret)
}

pub fn get(id: Uuid) -> Result<Zeroizing<String>, String> {
    load(&entry(id)?)
}

pub fn delete(id: Uuid) -> Result<(), String> {
    discard(entry(id)?)
}

pub fn set_proxy(host_id: Uuid, secret: String) -> Result<(), String> {
    store(&entry_for(&proxy_account(host_id))?, secret)
}

pub fn get_proxy(host_id: Uuid) -> Result<Zeroizing<String>, String> {
    load(&entry_for(&proxy_account(host_id))?)
}

pub fn delete_proxy(host_id: Uuid) -> Result<(), String> {
    discard(entry_for(&proxy_account(host_id))?)
}

pub fn connection_askpass_environment(
    database: &heminus_storage::Database,
    host: &Host,
    target_identity: Option<&Identity>,
) -> Result<Option<(String, String)>, String> {
    let mut candidates = Vec::new();
    if let Some(candidate) = target_identity.and_then(|identity| candidate(identity, host)) {
        push_candidate(&mut candidates, candidate);
    }
    for (index, jump_host) in crate::ssh_runtime::jump_hosts(database, host)?
        .into_iter()
        .enumerate()
    {
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
            .transpose()?;
        if let Some(candidate) = jump_identity
            .as_ref()
            .and_then(|identity| candidate(identity, &jump_host))
        {
            push_candidate(&mut candidates, candidate);
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not resolve the Heminus executable: {error}"))?;
    let encoded = serde_json::to_string(&candidates)
        .map_err(|error| format!("Could not prepare SSH credentials: {error}"))?;
    Ok(Some((executable.to_string_lossy().into_owned(), encoded)))
}

fn push_candidate(candidates: &mut Vec<AskpassCandidate>, candidate: AskpassCandidate) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| existing.id == candidate.id)
    {
        for needle in candidate.needles {
            if !existing.needles.contains(&needle) {
                existing.needles.push(needle);
            }
        }
    } else {
        candidates.push(candidate);
    }
}

fn candidate(identity: &Identity, host: &Host) -> Option<AskpassCandidate> {
    if !identity.secret_stored {
        return None;
    }
    let (needles, kind) = match identity.kind {
        IdentityKind::KeyFile => (
            identity.key_path.iter().cloned().collect(),
            AskpassSecret::KeyPassphrase,
        ),
        IdentityKind::Password => {
            let username = identity.username.as_deref().unwrap_or(&host.username);
            let address = crate::ssh_runtime::effective_address(host);
            (
                vec![format!("{username}@{address}"), address.to_string()],
                AskpassSecret::Password,
            )
        }
        IdentityKind::Agent => (Vec::new(), AskpassSecret::Password),
    };
    (!needles.is_empty()).then_some(AskpassCandidate {
        id: identity.id,
        needles,
        kind,
    })
}

pub fn run_askpass_if_requested() -> bool {
    let Ok(candidate_payload) = std::env::var(ASKPASS_CANDIDATES_ENV) else {
        return false;
    };
    let result = serde_json::from_str::<Vec<AskpassCandidate>>(&candidate_payload)
        .map_err(|error| format!("Invalid credential candidates: {error}"))
        .and_then(|candidates| {
            choose_candidate(&std::env::args().nth(1).unwrap_or_default(), &candidates)
        })
        .and_then(get);
    match result {
        Ok(secret) => {
            let _ = std::io::stdout().write_all(secret.as_bytes());
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            std::process::exit(1);
        }
    }
    true
}

/// Prompts that ask a question rather than request a secret.
///
/// `SSH_ASKPASS_REQUIRE=force` routes *every* prompt here, including host-key
/// confirmation and two-factor challenges. Answering those with the saved
/// password would leak it to the wrong reader and, for host keys, loop forever
/// on "Please type 'yes', 'no' or the fingerprint".
fn is_credential_prompt(prompt: &str) -> bool {
    const QUESTIONS: [&str; 6] = [
        "yes/no",
        "fingerprint",
        "authenticity of host",
        "continue connecting",
        "(y/n)",
        "please type",
    ];
    if QUESTIONS.iter().any(|question| prompt.contains(question)) {
        return false;
    }
    prompt.contains("password") || prompt.contains("passphrase")
}

fn choose_candidate(prompt: &str, candidates: &[AskpassCandidate]) -> Result<Uuid, String> {
    let prompt = prompt.to_lowercase();
    if !is_credential_prompt(&prompt) {
        return Err(
            "Heminus only answers password and passphrase prompts. Answer this one in the terminal."
                .into(),
        );
    }
    let matches = candidates
        .iter()
        .filter(|candidate| {
            candidate
                .needles
                .iter()
                .any(|needle| prompt.contains(&needle.to_lowercase()))
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        return Ok(matches[0].id);
    }
    // Falling back to the only saved credential is safe for a password, which
    // is destined for the server anyway. It is not safe for a key passphrase:
    // the server controls keyboard-interactive prompt text, so a hostile host
    // could ask "Enter passphrase:" and be handed the private key's passphrase,
    // which is never supposed to leave this machine. OpenSSH always names the
    // key file when it asks locally, so a genuine prompt matches a needle.
    if matches.is_empty()
        && let [only] = candidates
        && only.kind == AskpassSecret::Password
    {
        return Ok(only.id);
    }
    Err("Could not determine which saved SSH credential was requested".into())
}

pub const fn askpass_candidates_env() -> &'static str {
    ASKPASS_CANDIDATES_ENV
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_accounts_are_scoped_and_stable() {
        let id = Uuid::parse_str("4f75b7ec-c010-4f30-afd2-7e8582fcf071").unwrap();
        assert_eq!(account(id), "identity:4f75b7ec-c010-4f30-afd2-7e8582fcf071");
    }

    #[test]
    fn askpass_selects_target_and_jump_credentials_from_the_prompt() {
        let target = AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["deploy@10.0.0.20".into()],
            kind: AskpassSecret::Password,
        };
        let jump = AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["/vault/keys/gateway".into()],
            kind: AskpassSecret::KeyPassphrase,
        };
        let candidates = vec![target.clone(), jump.clone()];
        assert_eq!(
            choose_candidate("deploy@10.0.0.20's password:", &candidates).unwrap(),
            target.id
        );
        assert_eq!(
            choose_candidate(
                "Enter passphrase for key '/vault/keys/gateway':",
                &candidates
            )
            .unwrap(),
            jump.id
        );
    }

    #[test]
    fn askpass_refuses_host_key_confirmation_instead_of_replying_with_the_password() {
        let candidates = vec![AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["deploy@10.0.0.20".into(), "10.0.0.20".into()],
            kind: AskpassSecret::Password,
        }];
        for prompt in [
            "The authenticity of host '10.0.0.20 (10.0.0.20)' can't be established.\n\
             ED25519 key fingerprint is SHA256:abc.\n\
             Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "Please type 'yes', 'no' or the fingerprint: ",
            "Verification code: ",
        ] {
            assert!(
                choose_candidate(prompt, &candidates).is_err(),
                "answered a non-credential prompt: {prompt}"
            );
        }
        assert!(choose_candidate("deploy@10.0.0.20's password: ", &candidates).is_ok());
        assert!(choose_candidate("Password: ", &candidates).is_ok());
    }

    #[test]
    fn a_lone_key_passphrase_is_never_given_to_an_unrecognised_prompt() {
        let key = AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["/vault/keys/deploy".into()],
            kind: AskpassSecret::KeyPassphrase,
        };
        let candidates = vec![key.clone()];

        // The server picks the keyboard-interactive prompt text, so a hostile
        // host could ask for a "passphrase" and collect the private key's.
        for prompt in [
            "Enter passphrase: ",
            "Please enter your passphrase to continue: ",
            "password: ",
        ] {
            assert!(
                choose_candidate(prompt, &candidates).is_err(),
                "a key passphrase must not answer a prompt that does not name its key: {prompt}"
            );
        }

        // OpenSSH names the key file when it asks locally, so real prompts work.
        assert_eq!(
            choose_candidate("Enter passphrase for key '/vault/keys/deploy': ", &candidates)
                .unwrap(),
            key.id
        );
    }

    #[test]
    fn a_lone_password_still_answers_a_generic_server_prompt() {
        let password = AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["deploy@10.0.0.20".into()],
            kind: AskpassSecret::Password,
        };
        let candidates = vec![password.clone()];

        // A password is sent to that server either way, so the fallback stays.
        assert_eq!(
            choose_candidate("Password: ", &candidates).unwrap(),
            password.id
        );
    }

    #[test]
    fn candidate_kind_follows_the_identity() {
        let host = Host::new("Server", "10.0.0.20", "deploy");
        let mut password = Identity::new("Password", IdentityKind::Password);
        password.secret_stored = true;
        let mut key = Identity::new("Key", IdentityKind::KeyFile);
        key.secret_stored = true;
        key.key_path = Some("/vault/keys/deploy".into());

        assert_eq!(
            candidate(&password, &host).unwrap().kind,
            AskpassSecret::Password
        );
        assert_eq!(
            candidate(&key, &host).unwrap().kind,
            AskpassSecret::KeyPassphrase
        );
    }

    #[test]
    fn proxy_secrets_use_their_own_keyring_account() {
        let host_id = Uuid::parse_str("4f75b7ec-c010-4f30-afd2-7e8582fcf071").unwrap();
        assert_eq!(
            proxy_account(host_id),
            "proxy:4f75b7ec-c010-4f30-afd2-7e8582fcf071"
        );
        assert_ne!(proxy_account(host_id), account(host_id));
    }

    #[test]
    fn askpass_environment_includes_every_unique_chain_identity() {
        let database = heminus_storage::Database::in_memory().unwrap();
        let mut shared_identity = Identity::new("Shared password", IdentityKind::Password);
        shared_identity.secret_stored = true;
        database.save_identity(&shared_identity).unwrap();
        let mut relay_identity = Identity::new("Relay password", IdentityKind::Password);
        relay_identity.secret_stored = true;
        database.save_identity(&relay_identity).unwrap();

        let mut first_jump = Host::new("Edge", "192.0.2.10", "edge");
        first_jump.identity_id = Some(shared_identity.id);
        database.save_host(&first_jump).unwrap();
        let mut second_jump = Host::new("Relay", "10.0.0.10", "relay");
        second_jump.identity_id = Some(relay_identity.id);
        database.save_host(&second_jump).unwrap();
        let mut target = Host::new("Target", "10.0.1.20", "deploy");
        target.identity_id = Some(shared_identity.id);
        target.jump_host_ids = vec![first_jump.id, second_jump.id];

        let (_, encoded) =
            connection_askpass_environment(&database, &target, Some(&shared_identity))
                .unwrap()
                .unwrap();
        let candidates = serde_json::from_str::<Vec<AskpassCandidate>>(&encoded).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, shared_identity.id);
        assert_eq!(candidates[1].id, relay_identity.id);
        assert!(
            candidates[0]
                .needles
                .iter()
                .any(|needle| needle == "edge@192.0.2.10")
        );
    }

    #[test]
    #[cfg_attr(unix, ignore = "requires an unlocked desktop Secret Service")]
    fn native_keyring_round_trip() {
        let id = Uuid::new_v4();
        let result = (|| {
            set(id, "temporary-heminus-test-secret".into())?;
            if get(id)?.as_str() != "temporary-heminus-test-secret" {
                return Err("The native credential store returned the wrong secret".into());
            }
            Ok::<_, String>(())
        })();
        let cleanup = delete(id);
        result.unwrap();
        cleanup.unwrap();
        assert!(get(id).is_err());
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires the release GUI-subsystem executable"]
    fn release_gui_executable_returns_askpass_secret() {
        let id = Uuid::new_v4();
        let executable = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("target/release/heminus.exe");
        assert!(executable.is_file(), "build the Windows release first");
        let candidates = serde_json::to_string(&vec![AskpassCandidate {
            id,
            needles: vec!["deploy@example.invalid".into()],
        }])
        .unwrap();
        set(id, "temporary-release-askpass-secret".into()).unwrap();
        let output = std::process::Command::new(executable)
            .arg("deploy@example.invalid's password:")
            .env(ASKPASS_CANDIDATES_ENV, candidates)
            .output();
        let cleanup = delete(id);
        let output = output.unwrap();
        cleanup.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"temporary-release-askpass-secret");
    }
}
