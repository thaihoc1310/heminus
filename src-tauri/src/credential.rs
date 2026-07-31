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
}

fn account(id: Uuid) -> String {
    format!("identity:{id}")
}

fn entry(id: Uuid) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &account(id))
        .map_err(|error| format!("Could not access the operating-system credential store: {error}"))
}

pub fn set(id: Uuid, secret: String) -> Result<(), String> {
    if secret.is_empty() {
        return Err("Password cannot be empty".into());
    }
    let secret = Zeroizing::new(secret);
    entry(id)?
        .set_password(secret.as_str())
        .map_err(|error| format!("Could not save the password in the system keyring: {error}"))
}

pub fn get(id: Uuid) -> Result<Zeroizing<String>, String> {
    entry(id)?
        .get_password()
        .map(Zeroizing::new)
        .map_err(|error| format!("Could not read the password from the system keyring: {error}"))
}

pub fn delete(id: Uuid) -> Result<(), String> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "Could not remove the password from the system keyring: {error}"
        )),
    }
}

pub fn connection_askpass_environment(
    database: &heminus_storage::Database,
    host: &Host,
    target_identity: Option<&Identity>,
) -> Result<Option<(String, String)>, String> {
    let mut candidates = Vec::new();
    if let Some(candidate) = target_identity.and_then(|identity| candidate(identity, host)) {
        candidates.push(candidate);
    }
    if let Some(jump_host_id) = host.jump_host_id {
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
        if let Some(candidate) = jump_identity
            .as_ref()
            .and_then(|identity| candidate(identity, &jump_host))
        {
            candidates.push(candidate);
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

fn candidate(identity: &Identity, host: &Host) -> Option<AskpassCandidate> {
    if !identity.secret_stored {
        return None;
    }
    let needles = match identity.kind {
        IdentityKind::KeyFile => identity.key_path.iter().cloned().collect(),
        IdentityKind::Password => {
            let username = identity.username.as_deref().unwrap_or(&host.username);
            let address = crate::ssh_runtime::effective_address(host);
            vec![format!("{username}@{address}"), address.to_string()]
        }
        IdentityKind::Agent => Vec::new(),
    };
    (!needles.is_empty()).then_some(AskpassCandidate {
        id: identity.id,
        needles,
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

fn choose_candidate(prompt: &str, candidates: &[AskpassCandidate]) -> Result<Uuid, String> {
    let prompt = prompt.to_lowercase();
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
    if matches.is_empty() && candidates.len() == 1 {
        return Ok(candidates[0].id);
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
        };
        let jump = AskpassCandidate {
            id: Uuid::new_v4(),
            needles: vec!["/vault/keys/gateway".into()],
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
    #[ignore = "requires an unlocked desktop Secret Service"]
    fn native_keyring_round_trip() {
        let id = Uuid::new_v4();
        set(id, "temporary-heminus-test-secret".into()).unwrap();
        assert_eq!(get(id).unwrap().as_str(), "temporary-heminus-test-secret");
        delete(id).unwrap();
        assert!(get(id).is_err());
    }
}
