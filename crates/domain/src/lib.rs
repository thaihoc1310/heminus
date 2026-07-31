use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub id: Uuid,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub group_name: Option<String>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub color: HostColor,
    pub identity_id: Option<Uuid>,
    #[serde(default)]
    pub jump_host_ids: Vec<Uuid>,
    #[serde(default)]
    pub environment: Vec<EnvironmentVariable>,
    #[serde(default)]
    pub terminal_theme: TerminalTheme,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const fn default_terminal_font_size() -> u16 {
    14
}

impl Host {
    pub fn new(
        label: impl Into<String>,
        address: impl Into<String>,
        username: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            address: address.into(),
            port: 22,
            username: username.into(),
            group_name: None,
            group_id: None,
            tags: Vec::new(),
            color: HostColor::Amber,
            identity_id: None,
            jump_host_ids: Vec::new(),
            environment: Vec::new(),
            terminal_theme: TerminalTheme::default(),
            terminal_font_size: default_terminal_font_size(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("{}@{}:{}", self.username, self.address, self.port)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.label.trim().is_empty() {
            return Err(ValidationError::EmptyLabel);
        }
        if self.address.trim().is_empty() {
            return Err(ValidationError::EmptyAddress);
        }
        if self.username.trim().is_empty() {
            return Err(ValidationError::EmptyUsername);
        }
        if self.port == 0 {
            return Err(ValidationError::InvalidPort);
        }
        if !(9..=32).contains(&self.terminal_font_size) {
            return Err(ValidationError::InvalidTerminalFontSize);
        }
        if self.jump_host_ids.len() > 16 {
            return Err(ValidationError::TooManyJumpHosts);
        }
        let mut jump_host_ids = std::collections::HashSet::new();
        for jump_host_id in &self.jump_host_ids {
            if *jump_host_id == self.id {
                return Err(ValidationError::RecursiveJumpHost);
            }
            if !jump_host_ids.insert(jump_host_id) {
                return Err(ValidationError::DuplicateJumpHost);
            }
        }
        let mut environment_names = std::collections::HashSet::new();
        for variable in &self.environment {
            variable.validate()?;
            if !environment_names.insert(variable.name.as_str()) {
                return Err(ValidationError::DuplicateEnvironmentVariable);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
}

impl EnvironmentVariable {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut characters = self.name.chars();
        let valid_start = characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
        let valid_rest =
            characters.all(|character| character == '_' || character.is_ascii_alphanumeric());
        if !valid_start
            || !valid_rest
            || self
                .value
                .chars()
                .any(|character| matches!(character, '\0' | '\r' | '\n'))
        {
            return Err(ValidationError::InvalidEnvironmentVariable);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTheme {
    #[default]
    HeminusDark,
    GruvboxDark,
    KanagawaWave,
    HackerBlue,
    PaperLight,
    FlexokiDark,
    FlexokiLight,
    KanagawaLotus,
    HackerGreen,
    HackerRed,
    RosePineMoon,
    RosePineDawn,
    CatppuccinMocha,
    TokyoNight,
    TokyoDay,
    SolarizedDark,
    SolarizedLight,
    Dracula,
    Monokai,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    #[default]
    Agent,
    KeyFile,
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    pub label: String,
    pub kind: IdentityKind,
    pub username: Option<String>,
    pub key_path: Option<String>,
    #[serde(default)]
    pub secret_stored: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

impl Identity {
    pub fn new(label: impl Into<String>, kind: IdentityKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            kind,
            username: None,
            key_path: None,
            secret_stored: false,
            created_at: Utc::now(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.label.trim().is_empty() {
            return Err(ValidationError::EmptyIdentityLabel);
        }
        if self.kind == IdentityKind::KeyFile
            && self
                .key_path
                .as_deref()
                .is_none_or(|path| path.trim().is_empty())
        {
            return Err(ValidationError::MissingKeyPath);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultGroup {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl VaultGroup {
    pub fn new(name: impl Into<String>, parent_id: Option<Uuid>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            parent_id,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::EmptyGroupName);
        }
        if self.parent_id == Some(self.id) {
            return Err(ValidationError::GroupCycle);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspacePane {
    pub id: Uuid,
    pub host_id: Option<Uuid>,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceLayout {
    Pane {
        pane_id: Uuid,
    },
    Split {
        direction: SplitDirection,
        first: Box<WorkspaceLayout>,
        second: Box<WorkspaceLayout>,
    },
}

impl WorkspaceLayout {
    fn collect_panes(&self, panes: &mut std::collections::HashSet<Uuid>) -> bool {
        match self {
            Self::Pane { pane_id } => panes.insert(*pane_id),
            Self::Split { first, second, .. } => {
                first.collect_panes(panes) && second.collect_panes(panes)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub panes: Vec<WorkspacePane>,
    #[serde(default)]
    pub layout: Option<WorkspaceLayout>,
    pub split: bool,
    pub broadcast: bool,
    pub active_pane_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            panes: Vec::new(),
            layout: None,
            split: false,
            broadcast: false,
            active_pane_id: None,
            updated_at: Utc::now(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::EmptyWorkspaceName);
        }
        if self.panes.len() > 16 {
            return Err(ValidationError::TooManyWorkspacePanes);
        }
        let mut pane_ids = std::collections::HashSet::with_capacity(self.panes.len());
        for pane in &self.panes {
            if pane.title.trim().is_empty() || !pane_ids.insert(pane.id) {
                return Err(ValidationError::InvalidWorkspacePane);
            }
        }
        if self
            .active_pane_id
            .is_some_and(|id| !pane_ids.contains(&id))
        {
            return Err(ValidationError::InvalidWorkspacePane);
        }
        if let Some(layout) = &self.layout {
            let mut layout_ids = std::collections::HashSet::with_capacity(self.panes.len());
            if !layout.collect_panes(&mut layout_ids) || layout_ids != pane_ids {
                return Err(ValidationError::InvalidWorkspacePane);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostColor {
    Blue,
    Violet,
    Rose,
    #[default]
    Amber,
    Emerald,
    Slate,
}

impl HostColor {
    pub const fn css_class(self) -> &'static str {
        match self {
            Self::Blue => "host-blue",
            Self::Violet => "host-violet",
            Self::Rose => "host-rose",
            Self::Amber => "host-amber",
            Self::Emerald => "host-emerald",
            Self::Slate => "host-slate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: Uuid,
    pub title: String,
    pub command: String,
    pub description: String,
    pub favorite: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

impl Snippet {
    pub fn new(title: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            command: command.into(),
            description: String::new(),
            favorite: false,
            created_at: Utc::now(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.title.trim().is_empty() {
            return Err(ValidationError::EmptySnippetTitle);
        }
        if self.command.trim().is_empty() {
            return Err(ValidationError::EmptySnippetCommand);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortForward {
    pub id: Uuid,
    pub name: String,
    pub kind: ForwardKind,
    pub bind_host: String,
    pub bind_port: u16,
    pub destination_host: Option<String>,
    pub destination_port: Option<u16>,
    pub host_id: Uuid,
    pub enabled: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

impl PortForward {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::EmptyForwardName);
        }
        if self.bind_host.trim().is_empty() || self.bind_port == 0 {
            return Err(ValidationError::InvalidBindAddress);
        }
        if self.kind != ForwardKind::Dynamic
            && (self
                .destination_host
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
                || self.destination_port.is_none_or(|port| port == 0))
        {
            return Err(ValidationError::InvalidForwardDestination);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: Uuid,
    pub host_id: Option<Uuid>,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("Host name cannot be empty")]
    EmptyLabel,
    #[error("Host address cannot be empty")]
    EmptyAddress,
    #[error("Username cannot be empty")]
    EmptyUsername,
    #[error("Port must be between 1 and 65535")]
    InvalidPort,
    #[error("Terminal text size must be between 9 and 32")]
    InvalidTerminalFontSize,
    #[error("A host cannot use itself as its jump host")]
    RecursiveJumpHost,
    #[error("A jump host can appear only once in a connection chain")]
    DuplicateJumpHost,
    #[error("A connection chain can contain at most 16 jump hosts")]
    TooManyJumpHosts,
    #[error("Environment variable names must use letters, digits, and underscores")]
    InvalidEnvironmentVariable,
    #[error("Environment variable names must be unique")]
    DuplicateEnvironmentVariable,
    #[error("Snippet name cannot be empty")]
    EmptySnippetTitle,
    #[error("Snippet command cannot be empty")]
    EmptySnippetCommand,
    #[error("Forwarding rule name cannot be empty")]
    EmptyForwardName,
    #[error("Bind address and port are required")]
    InvalidBindAddress,
    #[error("A destination host and port are required for this forwarding type")]
    InvalidForwardDestination,
    #[error("Identity name cannot be empty")]
    EmptyIdentityLabel,
    #[error("Select a private key file")]
    MissingKeyPath,
    #[error("Group name cannot be empty")]
    EmptyGroupName,
    #[error("A group cannot contain itself or one of its ancestors")]
    GroupCycle,
    #[error("Workspace name cannot be empty")]
    EmptyWorkspaceName,
    #[error("A workspace can contain at most 16 terminal panes")]
    TooManyWorkspacePanes,
    #[error("Workspace panes must have unique IDs and non-empty titles")]
    InvalidWorkspacePane,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_host_has_safe_defaults() {
        let host = Host::new("production", "10.0.0.5", "ubuntu");
        assert_eq!(host.port, 22);
        assert!(host.tags.is_empty());
        assert_eq!(host.endpoint(), "ubuntu@10.0.0.5:22");
        assert!(host.validate().is_ok());
    }

    #[test]
    fn validation_rejects_blank_fields() {
        let host = Host::new(" ", "10.0.0.5", "ubuntu");
        assert_eq!(host.validate(), Err(ValidationError::EmptyLabel));
    }

    #[test]
    fn host_connection_options_are_validated() {
        let mut host = Host::new("production", "10.0.0.5", "ubuntu");
        host.environment = vec![
            EnvironmentVariable {
                name: "LANG".into(),
                value: "en_US.UTF-8".into(),
            },
            EnvironmentVariable {
                name: "LANG".into(),
                value: "vi_VN.UTF-8".into(),
            },
        ];
        assert_eq!(
            host.validate(),
            Err(ValidationError::DuplicateEnvironmentVariable)
        );

        host.environment[1].name = "INVALID-NAME".into();
        assert_eq!(
            host.validate(),
            Err(ValidationError::InvalidEnvironmentVariable)
        );
    }

    #[test]
    fn jump_chains_reject_self_references_duplicates_and_excessive_hops() {
        let mut host = Host::new("production", "10.0.0.5", "ubuntu");
        host.jump_host_ids = vec![host.id];
        assert_eq!(host.validate(), Err(ValidationError::RecursiveJumpHost));

        let jump_host_id = Uuid::new_v4();
        host.jump_host_ids = vec![jump_host_id, jump_host_id];
        assert_eq!(host.validate(), Err(ValidationError::DuplicateJumpHost));

        host.jump_host_ids = (0..17).map(|_| Uuid::new_v4()).collect();
        assert_eq!(host.validate(), Err(ValidationError::TooManyJumpHosts));
    }

    #[test]
    fn dynamic_forward_does_not_require_a_destination() {
        let rule = PortForward {
            id: Uuid::new_v4(),
            name: "Local proxy".into(),
            kind: ForwardKind::Dynamic,
            bind_host: "127.0.0.1".into(),
            bind_port: 1080,
            destination_host: None,
            destination_port: None,
            host_id: Uuid::new_v4(),
            enabled: false,
            created_at: Utc::now(),
        };
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn groups_reject_a_self_parent() {
        let mut group = VaultGroup::new("Production", None);
        group.parent_id = Some(group.id);
        assert_eq!(group.validate(), Err(ValidationError::GroupCycle));
    }

    #[test]
    fn workspaces_validate_active_panes() {
        let mut workspace = Workspace::new("Morning");
        workspace.active_pane_id = Some(Uuid::new_v4());
        assert_eq!(
            workspace.validate(),
            Err(ValidationError::InvalidWorkspacePane)
        );
    }
}
