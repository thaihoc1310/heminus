use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use heminus_domain::{
    EnvironmentVariable, ForwardKind, Host, HostColor, Identity, IdentityKind, PortForward,
    SessionRecord, SessionStatus, Snippet, TerminalTheme, VaultGroup, Workspace,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub struct Database {
    connection: Connection,
    path: Option<PathBuf>,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let project_dirs = ProjectDirs::from("app", "heminus", "Heminus")
            .context("Could not resolve the user data directory")?;
        let data_dir = project_dirs.data_local_dir();
        fs::create_dir_all(data_dir)
            .with_context(|| format!("Could not create {}", data_dir.display()))?;
        Self::open(data_dir.join("heminus.db"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open(&path)
            .with_context(|| format!("Could not open database {}", path.display()))?;
        let database = Self {
            connection,
            path: Some(path),
        };
        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    pub fn in_memory() -> Result<Self> {
        let database = Self {
            connection: Connection::open_in_memory()?,
            path: None,
        };
        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    fn configure(&self) -> Result<()> {
        self.connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
            ",
        )?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS hosts (
                id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                address TEXT NOT NULL,
                port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
                username TEXT NOT NULL,
                group_name TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                color TEXT NOT NULL DEFAULT 'amber',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_hosts_label ON hosts(label COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_hosts_address ON hosts(address);

            CREATE TABLE IF NOT EXISTS snippets (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                command TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS port_forwards (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                bind_host TEXT NOT NULL,
                bind_port INTEGER NOT NULL,
                destination_host TEXT,
                destination_port INTEGER,
                host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
                enabled INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_records (
                id TEXT PRIMARY KEY NOT NULL,
                host_id TEXT REFERENCES hosts(id) ON DELETE SET NULL,
                title TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                status TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_started ON session_records(started_at DESC);

            DELETE FROM session_records
            WHERE id NOT IN (
                SELECT latest.id
                FROM session_records AS latest
                WHERE latest.id = (
                    SELECT candidate.id
                    FROM session_records AS candidate
                    WHERE candidate.host_id = latest.host_id
                       OR (candidate.host_id IS NULL AND latest.host_id IS NULL)
                    ORDER BY candidate.started_at DESC, candidate.rowid DESC
                    LIMIT 1
                )
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_host_latest
                ON session_records(host_id)
                WHERE host_id IS NOT NULL;
            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_local_latest
                ON session_records((1))
                WHERE host_id IS NULL;

            CREATE TABLE IF NOT EXISTS command_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                host_id TEXT REFERENCES hosts(id) ON DELETE CASCADE,
                command TEXT NOT NULL,
                executed_at TEXT NOT NULL,
                successful INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_command_history_host_time
                ON command_history(host_id, executed_at DESC);

            ",
        )?;
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS identities (
                id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                kind TEXT NOT NULL,
                username TEXT,
                key_path TEXT,
                secret_stored INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS vault_groups (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                parent_id TEXT REFERENCES vault_groups(id) ON DELETE SET NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(parent_id, name COLLATE NOCASE)
            );
            CREATE INDEX IF NOT EXISTS idx_vault_groups_parent
                ON vault_groups(parent_id);

            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                panes_json TEXT NOT NULL DEFAULT '[]',
                layout_json TEXT,
                split INTEGER NOT NULL DEFAULT 0,
                broadcast INTEGER NOT NULL DEFAULT 0,
                active_pane_id TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workspaces_updated
                ON workspaces(updated_at DESC);
            ",
        )?;
        if !self.table_has_column("hosts", "identity_id")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN identity_id TEXT REFERENCES identities(id) ON DELETE SET NULL",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "group_id")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN group_id TEXT REFERENCES vault_groups(id) ON DELETE SET NULL",
                [],
            )?;
        }
        if !self.table_has_column("identities", "secret_stored")? {
            self.connection.execute(
                "ALTER TABLE identities ADD COLUMN secret_stored INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        for table in ["identities", "snippets", "port_forwards"] {
            if !self.table_has_column(table, "created_at")? {
                self.connection.execute(
                    &format!("ALTER TABLE {table} ADD COLUMN created_at TEXT"),
                    [],
                )?;
                self.connection.execute(
                    &format!(
                        "UPDATE {table}
                         SET created_at = strftime(
                             '%Y-%m-%dT%H:%M:%fZ',
                             'now',
                             '-' || ((SELECT max(rowid) FROM {table}) - rowid) || ' seconds'
                         )
                         WHERE created_at IS NULL"
                    ),
                    [],
                )?;
            }
        }
        if !self.table_has_column("workspaces", "layout_json")? {
            self.connection
                .execute("ALTER TABLE workspaces ADD COLUMN layout_json TEXT", [])?;
        }
        if !self.table_has_column("command_history", "successful")? {
            self.connection.execute(
                "ALTER TABLE command_history
                 ADD COLUMN successful INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        self.connection.execute(
            "
            DELETE FROM command_history
            WHERE id NOT IN (
                SELECT id
                FROM command_history
                ORDER BY executed_at DESC, id DESC
                LIMIT 500
            )
            ",
            [],
        )?;
        if !self.table_has_column("hosts", "startup_snippet_id")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN startup_snippet_id TEXT REFERENCES snippets(id) ON DELETE SET NULL",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "jump_host_id")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN jump_host_id TEXT REFERENCES hosts(id) ON DELETE SET NULL",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "jump_host_ids_json")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN jump_host_ids_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
            self.connection.execute(
                "UPDATE hosts
                 SET jump_host_ids_json = '[\"' || jump_host_id || '\"]'
                 WHERE jump_host_id IS NOT NULL
                   AND jump_host_ids_json = '[]'",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "environment_json")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN environment_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "terminal_theme")? {
            self.connection.execute(
                "ALTER TABLE hosts ADD COLUMN terminal_theme TEXT NOT NULL DEFAULT 'heminus_dark'",
                [],
            )?;
        }
        if !self.table_has_column("hosts", "terminal_font_size")? {
            self.connection.execute(
                "ALTER TABLE hosts
                 ADD COLUMN terminal_font_size INTEGER NOT NULL DEFAULT 14",
                [],
            )?;
        }
        self.migrate_legacy_groups()?;
        self.connection.execute(
            "INSERT OR REPLACE INTO schema_meta(key, value) VALUES ('version', '9')",
            [],
        )?;
        Ok(())
    }

    fn migrate_legacy_groups(&self) -> Result<()> {
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT group_name
            FROM hosts
            WHERE group_id IS NULL
              AND group_name IS NOT NULL
              AND trim(group_name) <> ''
            ",
        )?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for name in names {
            let existing: Option<String> = self
                .connection
                .query_row(
                    "
                    SELECT id FROM vault_groups
                    WHERE parent_id IS NULL AND name = ?1 COLLATE NOCASE
                    ",
                    [&name],
                    |row| row.get(0),
                )
                .optional()?;
            let id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
            self.connection.execute(
                "
                INSERT OR IGNORE INTO vault_groups (
                    id, name, parent_id, created_at, updated_at
                ) VALUES (?1, ?2, NULL, ?3, ?3)
                ",
                params![id, name, Utc::now().to_rfc3339()],
            )?;
            self.connection.execute(
                "
                UPDATE hosts SET group_id = ?1
                WHERE group_id IS NULL AND group_name = ?2
                ",
                params![id, name],
            )?;
        }
        Ok(())
    }

    fn table_has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let names = statement.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn host_count(&self) -> Result<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM hosts", [], |row| row.get::<_, i64>(0))?;
        Ok(count as usize)
    }

    pub fn list_hosts(&self, query: Option<&str>) -> Result<Vec<Host>> {
        let search = query.map(str::trim).filter(|value| !value.is_empty());
        let sql = if search.is_some() {
            "
            SELECT id, label, address, port, username, group_name, tags_json, color,
                   identity_id, group_id, jump_host_id, jump_host_ids_json,
                   environment_json, terminal_theme, terminal_font_size,
                   created_at, updated_at
            FROM hosts
            WHERE label LIKE ?1 ESCAPE '\\'
               OR address LIKE ?1 ESCAPE '\\'
               OR username LIKE ?1 ESCAPE '\\'
               OR (username || '@' || address) LIKE ?1 ESCAPE '\\'
               OR (address || '@' || username) LIKE ?1 ESCAPE '\\'
               OR group_name LIKE ?1 ESCAPE '\\'
            ORDER BY label COLLATE NOCASE
            "
        } else {
            "
            SELECT id, label, address, port, username, group_name, tags_json, color,
                   identity_id, group_id, jump_host_id, jump_host_ids_json,
                   environment_json, terminal_theme, terminal_font_size,
                   created_at, updated_at
            FROM hosts
            ORDER BY label COLLATE NOCASE
            "
        };

        let mut statement = self.connection.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Host> {
            let id: String = row.get(0)?;
            let tags_json: String = row.get(6)?;
            let color: String = row.get(7)?;
            let identity_id: Option<String> = row.get(8)?;
            let group_id: Option<String> = row.get(9)?;
            let jump_host_id: Option<String> = row.get(10)?;
            let jump_host_ids_json: String = row.get(11)?;
            let environment_json: String = row.get(12)?;
            let terminal_theme: String = row.get(13)?;
            let created_at: String = row.get(15)?;
            let updated_at: String = row.get(16)?;
            let mut jump_host_ids =
                serde_json::from_str::<Vec<Uuid>>(&jump_host_ids_json).unwrap_or_default();
            if jump_host_ids.is_empty()
                && let Some(jump_host_id) = jump_host_id.as_deref().map(parse_uuid).transpose()?
            {
                jump_host_ids.push(jump_host_id);
            }
            Ok(Host {
                id: parse_uuid(&id)?,
                label: row.get(1)?,
                address: row.get(2)?,
                port: row.get::<_, u16>(3)?,
                username: row.get(4)?,
                group_name: row.get(5)?,
                group_id: group_id.as_deref().map(parse_uuid).transpose()?,
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                color: parse_color(&color),
                identity_id: identity_id.as_deref().map(parse_uuid).transpose()?,
                jump_host_ids,
                environment: serde_json::from_str::<Vec<EnvironmentVariable>>(&environment_json)
                    .unwrap_or_default(),
                terminal_theme: parse_terminal_theme(&terminal_theme),
                terminal_font_size: row.get::<_, u16>(14)?,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            })
        };

        let rows = if let Some(search) = search {
            let escaped = search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            statement.query_map([pattern], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read the host list")
    }

    pub fn find_host(&self, id: Uuid) -> Result<Option<Host>> {
        let host = self
            .connection
            .query_row(
                "
                SELECT id, label, address, port, username, group_name, tags_json, color,
                       identity_id, group_id, jump_host_id, jump_host_ids_json,
                       environment_json, terminal_theme, terminal_font_size,
                       created_at, updated_at
                FROM hosts WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    let raw_id: String = row.get(0)?;
                    let tags_json: String = row.get(6)?;
                    let color: String = row.get(7)?;
                    let identity_id: Option<String> = row.get(8)?;
                    let group_id: Option<String> = row.get(9)?;
                    let jump_host_id: Option<String> = row.get(10)?;
                    let jump_host_ids_json: String = row.get(11)?;
                    let environment_json: String = row.get(12)?;
                    let terminal_theme: String = row.get(13)?;
                    let created_at: String = row.get(15)?;
                    let updated_at: String = row.get(16)?;
                    let mut jump_host_ids =
                        serde_json::from_str::<Vec<Uuid>>(&jump_host_ids_json).unwrap_or_default();
                    if jump_host_ids.is_empty()
                        && let Some(jump_host_id) =
                            jump_host_id.as_deref().map(parse_uuid).transpose()?
                    {
                        jump_host_ids.push(jump_host_id);
                    }
                    Ok(Host {
                        id: parse_uuid(&raw_id)?,
                        label: row.get(1)?,
                        address: row.get(2)?,
                        port: row.get::<_, u16>(3)?,
                        username: row.get(4)?,
                        group_name: row.get(5)?,
                        group_id: group_id.as_deref().map(parse_uuid).transpose()?,
                        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                        color: parse_color(&color),
                        identity_id: identity_id.as_deref().map(parse_uuid).transpose()?,
                        jump_host_ids,
                        environment: serde_json::from_str::<Vec<EnvironmentVariable>>(
                            &environment_json,
                        )
                        .unwrap_or_default(),
                        terminal_theme: parse_terminal_theme(&terminal_theme),
                        terminal_font_size: row.get::<_, u16>(14)?,
                        created_at: parse_datetime(&created_at)?,
                        updated_at: parse_datetime(&updated_at)?,
                    })
                },
            )
            .optional()?;
        Ok(host)
    }

    pub fn save_host(&self, host: &Host) -> Result<()> {
        host.validate()?;
        for (index, jump_host_id) in host.jump_host_ids.iter().enumerate() {
            if self.find_host(*jump_host_id)?.is_none() {
                anyhow::bail!("Jump host {} no longer exists", index + 1);
            }
        }
        self.connection.execute(
            "
            INSERT INTO hosts (
                id, label, address, port, username, group_name, tags_json, color,
                identity_id, group_id, jump_host_id, jump_host_ids_json,
                environment_json, terminal_theme, terminal_font_size,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17
            )
            ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                address = excluded.address,
                port = excluded.port,
                username = excluded.username,
                group_name = excluded.group_name,
                tags_json = excluded.tags_json,
                color = excluded.color,
                identity_id = excluded.identity_id,
                group_id = excluded.group_id,
                startup_snippet_id = NULL,
                jump_host_id = excluded.jump_host_id,
                jump_host_ids_json = excluded.jump_host_ids_json,
                environment_json = excluded.environment_json,
                terminal_theme = excluded.terminal_theme,
                terminal_font_size = excluded.terminal_font_size,
                updated_at = excluded.updated_at
            ",
            params![
                host.id.to_string(),
                host.label,
                host.address,
                host.port,
                host.username,
                host.group_name,
                serde_json::to_string(&host.tags)?,
                color_name(host.color),
                host.identity_id.map(|id| id.to_string()),
                host.group_id.map(|id| id.to_string()),
                host.jump_host_ids.first().map(Uuid::to_string),
                serde_json::to_string(&host.jump_host_ids)?,
                serde_json::to_string(&host.environment)?,
                terminal_theme_name(host.terminal_theme),
                host.terminal_font_size,
                host.created_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_host(&self, id: Uuid) -> Result<bool> {
        for mut host in self.list_hosts(None)? {
            if host.id != id && host.jump_host_ids.contains(&id) {
                host.jump_host_ids
                    .retain(|jump_host_id| *jump_host_id != id);
                self.save_host(&host)?;
            }
        }
        let affected = self
            .connection
            .execute("DELETE FROM hosts WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    pub fn list_groups(&self) -> Result<Vec<VaultGroup>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, name, parent_id, created_at, updated_at
            FROM vault_groups
            ORDER BY name COLLATE NOCASE
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let parent_id: Option<String> = row.get(2)?;
            let created_at: String = row.get(3)?;
            let updated_at: String = row.get(4)?;
            Ok(VaultGroup {
                id: parse_uuid(&id)?,
                name: row.get(1)?,
                parent_id: parent_id.as_deref().map(parse_uuid).transpose()?,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read vault groups")
    }

    pub fn find_group(&self, id: Uuid) -> Result<Option<VaultGroup>> {
        Ok(self.list_groups()?.into_iter().find(|group| group.id == id))
    }

    pub fn save_group(&self, group: &VaultGroup) -> Result<()> {
        group.validate()?;
        let duplicate: Option<String> = self
            .connection
            .query_row(
                "
                SELECT id FROM vault_groups
                WHERE name = ?1 COLLATE NOCASE
                  AND ((parent_id IS NULL AND ?2 IS NULL) OR parent_id = ?2)
                  AND id <> ?3
                LIMIT 1
                ",
                params![
                    group.name.trim(),
                    group.parent_id.map(|id| id.to_string()),
                    group.id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()?;
        if duplicate.is_some() {
            anyhow::bail!("A group with this name already exists at this level");
        }

        let mut ancestor = group.parent_id;
        while let Some(id) = ancestor {
            if id == group.id {
                anyhow::bail!("A group cannot contain itself or one of its ancestors");
            }
            ancestor = self
                .connection
                .query_row(
                    "SELECT parent_id FROM vault_groups WHERE id = ?1",
                    [id.to_string()],
                    |row| {
                        let parent: Option<String> = row.get(0)?;
                        parent.as_deref().map(parse_uuid).transpose()
                    },
                )
                .optional()?
                .ok_or_else(|| anyhow::anyhow!("The parent group no longer exists"))?;
        }

        self.connection.execute(
            "
            INSERT INTO vault_groups (id, name, parent_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                parent_id = excluded.parent_id,
                updated_at = excluded.updated_at
            ",
            params![
                group.id.to_string(),
                group.name.trim(),
                group.parent_id.map(|id| id.to_string()),
                group.created_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        self.refresh_host_group_paths()?;
        Ok(())
    }

    pub fn delete_group(&self, id: Uuid) -> Result<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM vault_groups WHERE id = ?1", [id.to_string()])?;
        if affected > 0 {
            self.refresh_host_group_paths()?;
        }
        Ok(affected > 0)
    }

    fn refresh_host_group_paths(&self) -> Result<()> {
        self.connection.execute_batch(
            "
            WITH RECURSIVE group_paths(id, path) AS (
                SELECT id, name
                FROM vault_groups
                WHERE parent_id IS NULL
                UNION ALL
                SELECT child.id, group_paths.path || ' / ' || child.name
                FROM vault_groups AS child
                JOIN group_paths ON child.parent_id = group_paths.id
            )
            UPDATE hosts
            SET group_name = (
                SELECT path FROM group_paths WHERE group_paths.id = hosts.group_id
            );
            ",
        )?;
        Ok(())
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, name, panes_json, layout_json, split, broadcast, active_pane_id, updated_at
            FROM workspaces
            ORDER BY updated_at DESC
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let panes_json: String = row.get(2)?;
            let layout_json: Option<String> = row.get(3)?;
            let active_pane_id: Option<String> = row.get(6)?;
            let updated_at: String = row.get(7)?;
            Ok(Workspace {
                id: parse_uuid(&id)?,
                name: row.get(1)?,
                panes: serde_json::from_str(&panes_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        panes_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                layout: layout_json
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                json.len(),
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })
                    .transpose()?,
                split: row.get(4)?,
                broadcast: row.get(5)?,
                active_pane_id: active_pane_id.as_deref().map(parse_uuid).transpose()?,
                updated_at: parse_datetime(&updated_at)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read workspaces")
    }

    pub fn find_workspace(&self, id: Uuid) -> Result<Option<Workspace>> {
        Ok(self
            .list_workspaces()?
            .into_iter()
            .find(|workspace| workspace.id == id))
    }

    pub fn save_workspace(&self, workspace: &Workspace) -> Result<()> {
        workspace.validate()?;
        self.connection.execute(
            "
            INSERT INTO workspaces (
                id, name, panes_json, layout_json, split, broadcast, active_pane_id, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                panes_json = excluded.panes_json,
                layout_json = excluded.layout_json,
                split = excluded.split,
                broadcast = excluded.broadcast,
                active_pane_id = excluded.active_pane_id,
                updated_at = excluded.updated_at
            ",
            params![
                workspace.id.to_string(),
                workspace.name.trim(),
                serde_json::to_string(&workspace.panes)?,
                workspace
                    .layout
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                workspace.split,
                workspace.broadcast,
                workspace.active_pane_id.map(|id| id.to_string()),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_workspace(&self, id: Uuid) -> Result<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM workspaces WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    pub fn list_identities(&self) -> Result<Vec<Identity>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, label, kind, username, key_path, secret_stored, created_at
            FROM identities
            ORDER BY label COLLATE NOCASE
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let kind: String = row.get(2)?;
            Ok(Identity {
                id: parse_uuid(&id)?,
                label: row.get(1)?,
                kind: parse_identity_kind(&kind),
                username: row.get(3)?,
                key_path: row.get(4)?,
                secret_stored: row.get(5)?,
                created_at: parse_datetime(&row.get::<_, String>(6)?)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read identities")
    }

    pub fn find_identity(&self, id: Uuid) -> Result<Option<Identity>> {
        Ok(self
            .list_identities()?
            .into_iter()
            .find(|identity| identity.id == id))
    }

    pub fn save_identity(&self, identity: &Identity) -> Result<()> {
        identity.validate()?;
        self.connection.execute(
            "
            INSERT INTO identities (id, label, kind, username, key_path, secret_stored, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                kind = excluded.kind,
                username = excluded.username,
                key_path = excluded.key_path,
                secret_stored = excluded.secret_stored
            ",
            params![
                identity.id.to_string(),
                identity.label,
                identity_kind_name(identity.kind),
                identity.username,
                identity.key_path,
                identity.secret_stored,
                identity.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn set_identity_secret_stored(&self, id: Uuid, stored: bool) -> Result<bool> {
        let affected = self.connection.execute(
            "UPDATE identities SET secret_stored = ?2 WHERE id = ?1",
            params![id.to_string(), stored],
        )?;
        Ok(affected > 0)
    }

    pub fn delete_identity(&self, id: Uuid) -> Result<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM identities WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    pub fn list_snippets(&self) -> Result<Vec<Snippet>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, title, command, description, favorite, created_at
            FROM snippets
            ORDER BY favorite DESC, title COLLATE NOCASE
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(Snippet {
                id: parse_uuid(&id)?,
                title: row.get(1)?,
                command: row.get(2)?,
                description: row.get(3)?,
                favorite: row.get(4)?,
                created_at: parse_datetime(&row.get::<_, String>(5)?)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read snippets")
    }

    pub fn save_snippet(&self, snippet: &Snippet) -> Result<()> {
        snippet.validate()?;
        self.connection.execute(
            "
            INSERT INTO snippets (id, title, command, description, favorite, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                command = excluded.command,
                description = excluded.description,
                favorite = excluded.favorite
            ",
            params![
                snippet.id.to_string(),
                snippet.title,
                snippet.command,
                snippet.description,
                snippet.favorite,
                snippet.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_snippet(&self, id: Uuid) -> Result<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM snippets WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    pub fn list_port_forwards(&self) -> Result<Vec<PortForward>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, name, kind, bind_host, bind_port, destination_host,
                   destination_port, host_id, enabled, created_at
            FROM port_forwards
            ORDER BY name COLLATE NOCASE
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let kind: String = row.get(2)?;
            let host_id: String = row.get(7)?;
            Ok(PortForward {
                id: parse_uuid(&id)?,
                name: row.get(1)?,
                kind: parse_forward_kind(&kind),
                bind_host: row.get(3)?,
                bind_port: row.get(4)?,
                destination_host: row.get(5)?,
                destination_port: row.get(6)?,
                host_id: parse_uuid(&host_id)?,
                enabled: row.get(8)?,
                created_at: parse_datetime(&row.get::<_, String>(9)?)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read port forwarding rules")
    }

    pub fn find_port_forward(&self, id: Uuid) -> Result<Option<PortForward>> {
        Ok(self
            .list_port_forwards()?
            .into_iter()
            .find(|rule| rule.id == id))
    }

    pub fn save_port_forward(&self, rule: &PortForward) -> Result<()> {
        rule.validate()?;
        self.connection.execute(
            "
            INSERT INTO port_forwards (
                id, name, kind, bind_host, bind_port, destination_host,
                destination_port, host_id, enabled, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                bind_host = excluded.bind_host,
                bind_port = excluded.bind_port,
                destination_host = excluded.destination_host,
                destination_port = excluded.destination_port,
                host_id = excluded.host_id,
                enabled = excluded.enabled
            ",
            params![
                rule.id.to_string(),
                rule.name,
                forward_kind_name(rule.kind),
                rule.bind_host,
                rule.bind_port,
                rule.destination_host,
                rule.destination_port,
                rule.host_id.to_string(),
                rule.enabled,
                rule.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_port_forward(&self, id: Uuid) -> Result<bool> {
        let affected = self
            .connection
            .execute("DELETE FROM port_forwards WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    pub fn list_command_history(&self, host_id: Option<Uuid>, limit: usize) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "
            SELECT command
            FROM command_history
            WHERE host_id IS ?1
              AND successful = 1
            GROUP BY command
            ORDER BY MAX(executed_at) DESC, MAX(id) DESC
            LIMIT ?2
            ",
        )?;
        let host_id = host_id.map(|id| id.to_string());
        let rows = statement.query_map(params![host_id, limit.min(1000) as i64], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read command history")
    }

    pub fn list_all_command_history(&self, limit: usize) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "
            SELECT command
            FROM command_history
            WHERE successful = 1
            GROUP BY command
            ORDER BY MAX(executed_at) DESC, MAX(id) DESC
            LIMIT ?1
            ",
        )?;
        let rows = statement.query_map([limit.min(500) as i64], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read global command history")
    }

    pub fn record_command(&self, host_id: Option<Uuid>, command: &str) -> Result<bool> {
        let command = command.trim();
        if command.is_empty()
            || command
                .chars()
                .any(|character| matches!(character, '\0' | '\n' | '\r'))
        {
            return Ok(false);
        }
        self.connection.execute(
            "
            INSERT INTO command_history (host_id, command, executed_at, successful)
            VALUES (?1, ?2, ?3, 1)
            ",
            params![
                host_id.map(|id| id.to_string()),
                command,
                Utc::now().to_rfc3339(),
            ],
        )?;
        self.connection.execute(
            "
            DELETE FROM command_history
            WHERE id NOT IN (
                SELECT id
                FROM command_history
                ORDER BY executed_at DESC, id DESC
                LIMIT 500
            )
            ",
            [],
        )?;
        Ok(true)
    }

    pub fn delete_command_history(&self, command: &str) -> Result<bool> {
        let command = command.trim();
        if command.is_empty() {
            return Ok(false);
        }
        let affected = self.connection.execute(
            "
            DELETE FROM command_history
            WHERE command = ?1
              AND successful = 1
            ",
            [command],
        )?;
        Ok(affected > 0)
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, host_id, title, started_at, ended_at, status
            FROM session_records
            WHERE id IN (
                SELECT latest.id
                FROM session_records AS latest
                WHERE latest.id = (
                    SELECT candidate.id
                    FROM session_records AS candidate
                    WHERE candidate.host_id = latest.host_id
                       OR (candidate.host_id IS NULL AND latest.host_id IS NULL)
                    ORDER BY candidate.started_at DESC, candidate.rowid DESC
                    LIMIT 1
                )
            )
            ORDER BY started_at DESC
            LIMIT ?1
            ",
        )?;
        let rows = statement.query_map([limit.min(1000) as i64], |row| {
            let id: String = row.get(0)?;
            let host_id: Option<String> = row.get(1)?;
            let started_at: String = row.get(3)?;
            let ended_at: Option<String> = row.get(4)?;
            let status: String = row.get(5)?;
            Ok(SessionRecord {
                id: parse_uuid(&id)?,
                host_id: host_id.as_deref().map(parse_uuid).transpose()?,
                title: row.get(2)?,
                started_at: parse_datetime(&started_at)?,
                ended_at: ended_at.as_deref().map(parse_datetime).transpose()?,
                status: parse_session_status(&status),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("Could not read session history")
    }

    pub fn save_session(&self, session: &SessionRecord) -> Result<()> {
        self.connection.execute(
            "
            INSERT INTO session_records (
                id, host_id, title, started_at, ended_at, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                ended_at = excluded.ended_at,
                status = excluded.status
            ",
            params![
                session.id.to_string(),
                session.host_id.map(|id| id.to_string()),
                session.title,
                session.started_at.to_rfc3339(),
                session.ended_at.map(|value| value.to_rfc3339()),
                session_status_name(session.status),
            ],
        )?;
        Ok(())
    }

    pub fn start_session(&self, host_id: Option<Uuid>, title: impl Into<String>) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let host_id_text = host_id.map(|value| value.to_string());
        self.connection.execute(
            "
            DELETE FROM session_records
            WHERE host_id = ?1 OR (host_id IS NULL AND ?1 IS NULL)
            ",
            params![host_id_text.as_deref()],
        )?;
        self.save_session(&SessionRecord {
            id,
            host_id,
            title: title.into(),
            started_at: Utc::now(),
            ended_at: None,
            status: SessionStatus::Connected,
        })?;
        Ok(id)
    }

    pub fn finish_session(&self, id: Uuid, status: SessionStatus) -> Result<()> {
        self.connection.execute(
            "
            UPDATE session_records
            SET ended_at = ?2, status = ?3
            WHERE id = ?1
            ",
            params![
                id.to_string(),
                Utc::now().to_rfc3339(),
                session_status_name(status)
            ],
        )?;
        Ok(())
    }

    pub fn reconcile_active_sessions(&self) -> Result<usize> {
        self.connection
            .execute(
                "
                UPDATE session_records
                SET ended_at = COALESCE(ended_at, ?1), status = 'disconnected'
                WHERE ended_at IS NULL
                  AND status IN ('connecting', 'connected')
                ",
                [Utc::now().to_rfc3339()],
            )
            .context("Could not reconcile interrupted terminal sessions")
    }

    pub fn seed_welcome_hosts(&self) -> Result<()> {
        if self.host_count()? > 0 {
            return Ok(());
        }
        let mut host = Host::new("Local Ubuntu", "127.0.0.1", whoami());
        host.color = HostColor::Blue;
        host.group_name = Some("This device".into());
        self.save_host(&host)?;
        Ok(())
    }
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

fn color_name(color: HostColor) -> &'static str {
    match color {
        HostColor::Blue => "blue",
        HostColor::Violet => "violet",
        HostColor::Rose => "rose",
        HostColor::Amber => "amber",
        HostColor::Emerald => "emerald",
        HostColor::Slate => "slate",
    }
}

fn parse_color(value: &str) -> HostColor {
    match value {
        "blue" => HostColor::Blue,
        "violet" => HostColor::Violet,
        "rose" => HostColor::Rose,
        "emerald" => HostColor::Emerald,
        "slate" => HostColor::Slate,
        _ => HostColor::Amber,
    }
}

fn terminal_theme_name(theme: TerminalTheme) -> &'static str {
    match theme {
        TerminalTheme::HeminusDark => "heminus_dark",
        TerminalTheme::GruvboxDark => "gruvbox_dark",
        TerminalTheme::KanagawaWave => "kanagawa_wave",
        TerminalTheme::HackerBlue => "hacker_blue",
        TerminalTheme::PaperLight => "paper_light",
        TerminalTheme::FlexokiDark => "flexoki_dark",
        TerminalTheme::FlexokiLight => "flexoki_light",
        TerminalTheme::KanagawaLotus => "kanagawa_lotus",
        TerminalTheme::HackerGreen => "hacker_green",
        TerminalTheme::HackerRed => "hacker_red",
        TerminalTheme::RosePineMoon => "rose_pine_moon",
        TerminalTheme::RosePineDawn => "rose_pine_dawn",
        TerminalTheme::CatppuccinMocha => "catppuccin_mocha",
        TerminalTheme::TokyoNight => "tokyo_night",
        TerminalTheme::TokyoDay => "tokyo_day",
        TerminalTheme::SolarizedDark => "solarized_dark",
        TerminalTheme::SolarizedLight => "solarized_light",
        TerminalTheme::Dracula => "dracula",
        TerminalTheme::Monokai => "monokai",
    }
}

fn parse_terminal_theme(value: &str) -> TerminalTheme {
    match value {
        "gruvbox_dark" => TerminalTheme::GruvboxDark,
        "kanagawa_wave" => TerminalTheme::KanagawaWave,
        "hacker_blue" => TerminalTheme::HackerBlue,
        "paper_light" => TerminalTheme::PaperLight,
        "flexoki_dark" => TerminalTheme::FlexokiDark,
        "flexoki_light" => TerminalTheme::FlexokiLight,
        "kanagawa_dragon" => TerminalTheme::KanagawaWave,
        "kanagawa_lotus" => TerminalTheme::KanagawaLotus,
        "hacker_green" => TerminalTheme::HackerGreen,
        "hacker_red" => TerminalTheme::HackerRed,
        "everforest_dark" => TerminalTheme::FlexokiDark,
        "everforest_light" => TerminalTheme::SolarizedLight,
        "night_owl" => TerminalTheme::HackerBlue,
        "light_owl" => TerminalTheme::PaperLight,
        "rose_pine" => TerminalTheme::RosePineMoon,
        "rose_pine_moon" => TerminalTheme::RosePineMoon,
        "rose_pine_dawn" => TerminalTheme::RosePineDawn,
        "catppuccin_mocha" => TerminalTheme::CatppuccinMocha,
        "catppuccin_latte" => TerminalTheme::RosePineDawn,
        "tokyo_night" => TerminalTheme::TokyoNight,
        "tokyo_day" => TerminalTheme::TokyoDay,
        "solarized_dark" => TerminalTheme::SolarizedDark,
        "solarized_light" => TerminalTheme::SolarizedLight,
        "dracula" => TerminalTheme::Dracula,
        "monokai" => TerminalTheme::Monokai,
        "atom_one_dark" => TerminalTheme::HeminusDark,
        "atom_one_light" => TerminalTheme::PaperLight,
        _ => TerminalTheme::HeminusDark,
    }
}

fn forward_kind_name(kind: ForwardKind) -> &'static str {
    match kind {
        ForwardKind::Local => "local",
        ForwardKind::Remote => "remote",
        ForwardKind::Dynamic => "dynamic",
    }
}

fn parse_forward_kind(value: &str) -> ForwardKind {
    match value {
        "remote" => ForwardKind::Remote,
        "dynamic" => ForwardKind::Dynamic,
        _ => ForwardKind::Local,
    }
}

fn identity_kind_name(kind: IdentityKind) -> &'static str {
    match kind {
        IdentityKind::Agent => "agent",
        IdentityKind::KeyFile => "key_file",
        IdentityKind::Password => "password",
    }
}

fn parse_identity_kind(value: &str) -> IdentityKind {
    match value {
        "key_file" => IdentityKind::KeyFile,
        "password" => IdentityKind::Password,
        _ => IdentityKind::Agent,
    }
}

fn session_status_name(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Connecting => "connecting",
        SessionStatus::Connected => "connected",
        SessionStatus::Disconnected => "disconnected",
        SessionStatus::Failed => "failed",
    }
}

fn parse_session_status(value: &str) -> SessionStatus {
    match value {
        "connecting" => SessionStatus::Connecting,
        "connected" => SessionStatus::Connected,
        "failed" => SessionStatus::Failed,
        _ => SessionStatus::Disconnected,
    }
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn parse_datetime(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                value.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_round_trip_and_search() {
        let database = Database::in_memory().unwrap();
        let first_jump_host = Host::new("Gateway", "10.42.0.1", "bastion");
        let second_jump_host = Host::new("Relay", "10.42.0.2", "relay");
        database.save_host(&first_jump_host).unwrap();
        database.save_host(&second_jump_host).unwrap();
        let mut host = Host::new("Kubernetes control plane", "10.42.0.10", "ubuntu");
        host.tags = vec!["k8s".into(), "production".into()];
        host.jump_host_ids = vec![first_jump_host.id, second_jump_host.id];
        host.environment = vec![EnvironmentVariable {
            name: "LANG".into(),
            value: "en_US.UTF-8".into(),
        }];
        host.terminal_theme = TerminalTheme::KanagawaWave;
        database.save_host(&host).unwrap();

        let loaded = database.find_host(host.id).unwrap().unwrap();
        assert_eq!(loaded.label, host.label);
        assert_eq!(loaded.tags, host.tags);
        assert_eq!(
            loaded.jump_host_ids,
            vec![first_jump_host.id, second_jump_host.id]
        );
        assert_eq!(loaded.environment, host.environment);
        assert_eq!(loaded.terminal_theme, TerminalTheme::KanagawaWave);
        assert_eq!(database.list_hosts(Some("control")).unwrap().len(), 1);
        assert_eq!(
            database
                .list_hosts(Some("ubuntu@10.42.0.10"))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            database
                .list_hosts(Some("10.42.0.10@ubuntu"))
                .unwrap()
                .len(),
            1
        );
        assert!(database.list_hosts(Some("missing")).unwrap().is_empty());
    }

    #[test]
    fn legacy_single_jump_host_migrates_to_an_ordered_chain() {
        let path = std::env::temp_dir().join(format!("heminus-storage-test-{}.db", Uuid::new_v4()));
        let first_jump_host_id = Uuid::new_v4();
        let target_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE hosts (
                        id TEXT PRIMARY KEY NOT NULL,
                        label TEXT NOT NULL,
                        address TEXT NOT NULL,
                        port INTEGER NOT NULL,
                        username TEXT NOT NULL,
                        group_name TEXT,
                        tags_json TEXT NOT NULL DEFAULT '[]',
                        color TEXT NOT NULL DEFAULT 'amber',
                        jump_host_id TEXT,
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hosts (
                        id, label, address, port, username, jump_host_id, created_at, updated_at
                     ) VALUES (?1, 'Gateway', '192.0.2.10', 22, 'edge', NULL, ?3, ?3),
                              (?2, 'Target', '10.0.0.20', 22, 'deploy', ?1, ?3, ?3)",
                    params![first_jump_host_id.to_string(), target_id.to_string(), now],
                )
                .unwrap();
        }

        let database = Database::open(&path).unwrap();
        assert_eq!(
            database
                .find_host(target_id)
                .unwrap()
                .unwrap()
                .jump_host_ids,
            vec![first_jump_host_id]
        );
        drop(database);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn delete_host_is_idempotent() {
        let database = Database::in_memory().unwrap();
        let first_jump_host = Host::new("Gateway", "10.0.0.1", "root");
        let second_jump_host = Host::new("Relay", "10.0.0.2", "root");
        database.save_host(&first_jump_host).unwrap();
        database.save_host(&second_jump_host).unwrap();
        let mut target = Host::new("Target", "10.0.0.3", "root");
        target.jump_host_ids = vec![first_jump_host.id, second_jump_host.id];
        database.save_host(&target).unwrap();

        assert!(database.delete_host(first_jump_host.id).unwrap());
        assert_eq!(
            database
                .find_host(target.id)
                .unwrap()
                .unwrap()
                .jump_host_ids,
            vec![second_jump_host.id]
        );
        assert!(!database.delete_host(first_jump_host.id).unwrap());
    }

    #[test]
    fn snippets_and_forwarding_rules_round_trip() {
        let database = Database::in_memory().unwrap();
        let host = Host::new("Gateway", "10.0.0.2", "admin");
        database.save_host(&host).unwrap();

        let snippet = Snippet::new("Disk usage", "df -h");
        database.save_snippet(&snippet).unwrap();
        assert_eq!(database.list_snippets().unwrap(), vec![snippet]);

        let rule = PortForward {
            id: Uuid::new_v4(),
            name: "Database".into(),
            kind: ForwardKind::Local,
            bind_host: "127.0.0.1".into(),
            bind_port: 5432,
            destination_host: Some("db.internal".into()),
            destination_port: Some(5432),
            host_id: host.id,
            enabled: false,
            created_at: Utc::now(),
        };
        database.save_port_forward(&rule).unwrap();
        assert_eq!(database.find_port_forward(rule.id).unwrap(), Some(rule));
    }

    #[test]
    fn command_history_is_scoped_to_the_host_and_deduplicated_by_recency() {
        let database = Database::in_memory().unwrap();
        let host = Host::new("Production", "10.0.0.2", "admin");
        database.save_host(&host).unwrap();
        database
            .connection
            .execute(
                "INSERT INTO command_history (host_id, command, executed_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    host.id.to_string(),
                    "definitely-not-a-real-command",
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        database
            .record_command(Some(host.id), "ping google.com")
            .unwrap();
        database.record_command(Some(host.id), "uname -a").unwrap();
        database
            .record_command(Some(host.id), "ping google.com")
            .unwrap();
        database.record_command(None, "pwd").unwrap();

        assert_eq!(
            database.list_command_history(Some(host.id), 20).unwrap(),
            vec!["ping google.com", "uname -a"]
        );
        assert_eq!(
            database.list_command_history(None, 20).unwrap(),
            vec!["pwd"]
        );
        assert_eq!(
            database.list_all_command_history(20).unwrap(),
            vec!["pwd", "ping google.com", "uname -a"]
        );
        assert!(!database.record_command(Some(host.id), "  ").unwrap());

        assert!(database.delete_command_history("ping google.com").unwrap());
        assert_eq!(
            database.list_command_history(Some(host.id), 20).unwrap(),
            vec!["uname -a"]
        );
        assert_eq!(
            database.list_all_command_history(20).unwrap(),
            vec!["pwd", "uname -a"]
        );
        assert!(!database.delete_command_history("missing").unwrap());
    }

    #[test]
    fn identity_can_be_assigned_to_a_host() {
        let database = Database::in_memory().unwrap();
        let mut identity = Identity::new("Default agent", IdentityKind::Agent);
        identity.username = Some("deploy".into());
        database.save_identity(&identity).unwrap();

        let mut host = Host::new("Production", "prod.example.com", "ubuntu");
        host.identity_id = Some(identity.id);
        database.save_host(&host).unwrap();

        assert_eq!(
            database.find_identity(identity.id).unwrap(),
            Some(identity.clone())
        );
        assert_eq!(
            database.find_host(host.id).unwrap().unwrap().identity_id,
            Some(identity.id)
        );

        database.delete_identity(identity.id).unwrap();
        assert_eq!(
            database.find_host(host.id).unwrap().unwrap().identity_id,
            None
        );
    }

    #[test]
    fn session_history_is_ordered_and_can_be_finished() {
        let database = Database::in_memory().unwrap();
        let session = SessionRecord {
            id: Uuid::new_v4(),
            host_id: None,
            title: "Local Terminal".into(),
            started_at: Utc::now(),
            ended_at: None,
            status: SessionStatus::Connected,
        };
        database.save_session(&session).unwrap();
        database
            .finish_session(session.id, SessionStatus::Disconnected)
            .unwrap();

        let loaded = database.list_sessions(10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].status, SessionStatus::Disconnected);
        assert!(loaded[0].ended_at.is_some());
    }

    #[test]
    fn interrupted_sessions_are_reconciled_on_startup() {
        let database = Database::in_memory().unwrap();
        let active_host = Host::new("Active", "active.example.com", "root");
        let finished_host = Host::new("Finished", "finished.example.com", "root");
        database.save_host(&active_host).unwrap();
        database.save_host(&finished_host).unwrap();
        let active_id = database
            .start_session(Some(active_host.id), "Interrupted SSH")
            .unwrap();
        let finished_id = database
            .start_session(Some(finished_host.id), "Finished SSH")
            .unwrap();
        database
            .finish_session(finished_id, SessionStatus::Disconnected)
            .unwrap();

        assert_eq!(database.reconcile_active_sessions().unwrap(), 1);
        let loaded = database.list_sessions(10).unwrap();
        let active = loaded
            .iter()
            .find(|session| session.id == active_id)
            .unwrap();
        let finished = loaded
            .iter()
            .find(|session| session.id == finished_id)
            .unwrap();
        assert_eq!(active.status, SessionStatus::Disconnected);
        assert!(active.ended_at.is_some());
        assert_eq!(finished.status, SessionStatus::Disconnected);
        assert!(finished.ended_at.is_some());
    }

    #[test]
    fn a_new_session_replaces_the_previous_session_for_the_same_host() {
        let database = Database::in_memory().unwrap();
        let host = Host::new("Server", "server.example.com", "root");
        database.save_host(&host).unwrap();
        let first = database
            .start_session(Some(host.id), "Server at 10:00")
            .unwrap();
        database
            .finish_session(first, SessionStatus::Disconnected)
            .unwrap();
        let second = database
            .start_session(Some(host.id), "Server at 12:00")
            .unwrap();

        let loaded = database.list_sessions(10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, second);
        assert_eq!(loaded[0].title, "Server at 12:00");
    }

    #[test]
    fn legacy_collection_rows_receive_stable_creation_times() {
        let path = std::env::temp_dir().join(format!("heminus-legacy-{}.db", Uuid::new_v4()));
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "
                    CREATE TABLE snippets (
                        id TEXT PRIMARY KEY NOT NULL,
                        title TEXT NOT NULL,
                        command TEXT NOT NULL,
                        description TEXT NOT NULL DEFAULT '',
                        favorite INTEGER NOT NULL DEFAULT 0
                    );
                    ",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO snippets (id, title, command) VALUES (?1, 'First', 'one')",
                    [first_id.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO snippets (id, title, command) VALUES (?1, 'Second', 'two')",
                    [second_id.to_string()],
                )
                .unwrap();
        }

        let database = Database::open(&path).unwrap();
        let snippets = database.list_snippets().unwrap();
        let first = snippets.iter().find(|item| item.id == first_id).unwrap();
        let second = snippets.iter().find(|item| item.id == second_id).unwrap();
        assert!(second.created_at > first.created_at);
        drop(database);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn nested_groups_round_trip_and_reject_cycles() {
        let database = Database::in_memory().unwrap();
        let mut root = VaultGroup::new("Infrastructure", None);
        let child = VaultGroup::new("Production", Some(root.id));
        database.save_group(&root).unwrap();
        database.save_group(&child).unwrap();
        assert_eq!(database.list_groups().unwrap().len(), 2);

        root.parent_id = Some(child.id);
        assert!(database.save_group(&root).is_err());
    }

    #[test]
    fn host_group_paths_follow_nested_renames_moves_and_deletes() {
        let database = Database::in_memory().unwrap();
        let parent = VaultGroup::new("Production", None);
        database.save_group(&parent).unwrap();
        let mut child = VaultGroup::new("Databases", Some(parent.id));
        database.save_group(&child).unwrap();
        let mut host = Host::new("Postgres", "db.example.com", "postgres");
        host.group_id = Some(child.id);
        host.group_name = Some("Production / Databases".into());
        database.save_host(&host).unwrap();

        child.name = "Storage".into();
        database.save_group(&child).unwrap();
        assert_eq!(
            database.find_host(host.id).unwrap().unwrap().group_name,
            Some("Production / Storage".into())
        );

        child.parent_id = None;
        database.save_group(&child).unwrap();
        assert_eq!(
            database.find_host(host.id).unwrap().unwrap().group_name,
            Some("Storage".into())
        );

        database.delete_group(child.id).unwrap();
        let loaded = database.find_host(host.id).unwrap().unwrap();
        assert_eq!(loaded.group_id, None);
        assert_eq!(loaded.group_name, None);
    }

    #[test]
    fn workspaces_round_trip_split_and_broadcast_state() {
        let database = Database::in_memory().unwrap();
        let mut workspace = Workspace::new("Kubernetes");
        let pane = heminus_domain::WorkspacePane {
            id: Uuid::new_v4(),
            host_id: None,
            title: "Local Terminal".into(),
        };
        workspace.panes.push(pane.clone());
        workspace.layout = Some(heminus_domain::WorkspaceLayout::Pane { pane_id: pane.id });
        workspace.active_pane_id = Some(pane.id);
        workspace.split = true;
        workspace.broadcast = true;
        database.save_workspace(&workspace).unwrap();

        let loaded = database.find_workspace(workspace.id).unwrap().unwrap();
        assert_eq!(loaded.name, "Kubernetes");
        assert_eq!(loaded.panes, vec![pane]);
        assert_eq!(loaded.layout, workspace.layout);
        assert!(loaded.split);
        assert!(loaded.broadcast);
    }

    #[test]
    fn every_terminal_theme_name_round_trips() {
        let themes = [
            TerminalTheme::HeminusDark,
            TerminalTheme::GruvboxDark,
            TerminalTheme::KanagawaWave,
            TerminalTheme::HackerBlue,
            TerminalTheme::PaperLight,
            TerminalTheme::FlexokiDark,
            TerminalTheme::FlexokiLight,
            TerminalTheme::KanagawaLotus,
            TerminalTheme::HackerGreen,
            TerminalTheme::HackerRed,
            TerminalTheme::RosePineMoon,
            TerminalTheme::RosePineDawn,
            TerminalTheme::CatppuccinMocha,
            TerminalTheme::TokyoNight,
            TerminalTheme::TokyoDay,
            TerminalTheme::SolarizedDark,
            TerminalTheme::SolarizedLight,
            TerminalTheme::Dracula,
            TerminalTheme::Monokai,
        ];
        for theme in themes {
            assert_eq!(parse_terminal_theme(terminal_theme_name(theme)), theme);
        }
    }

    #[test]
    fn removed_near_duplicate_themes_migrate_to_curated_equivalents() {
        let aliases = [
            ("kanagawa_dragon", TerminalTheme::KanagawaWave),
            ("everforest_dark", TerminalTheme::FlexokiDark),
            ("everforest_light", TerminalTheme::SolarizedLight),
            ("night_owl", TerminalTheme::HackerBlue),
            ("light_owl", TerminalTheme::PaperLight),
            ("rose_pine", TerminalTheme::RosePineMoon),
            ("catppuccin_latte", TerminalTheme::RosePineDawn),
            ("atom_one_dark", TerminalTheme::HeminusDark),
            ("atom_one_light", TerminalTheme::PaperLight),
        ];
        for (stored_name, replacement) in aliases {
            assert_eq!(parse_terminal_theme(stored_name), replacement);
        }
    }

    #[test]
    fn version_two_databases_migrate_legacy_group_names() {
        let path =
            std::env::temp_dir().join(format!("heminus-migration-test-{}.db", Uuid::new_v4()));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE hosts (
                    id TEXT PRIMARY KEY NOT NULL,
                    label TEXT NOT NULL,
                    address TEXT NOT NULL,
                    port INTEGER NOT NULL,
                    username TEXT NOT NULL,
                    group_name TEXT,
                    tags_json TEXT NOT NULL DEFAULT '[]',
                    color TEXT NOT NULL DEFAULT 'amber',
                    identity_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE identities (
                    id TEXT PRIMARY KEY NOT NULL,
                    label TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    username TEXT,
                    key_path TEXT
                );
                ",
            )
            .unwrap();
        let host = Host::new("Migrated", "migrated.example", "ubuntu");
        connection
            .execute(
                "
                INSERT INTO hosts (
                    id, label, address, port, username, group_name, tags_json,
                    color, identity_id, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 22, ?4, 'Legacy / Production', '[]',
                          'blue', NULL, ?5, ?5)
                ",
                params![
                    host.id.to_string(),
                    host.label,
                    host.address,
                    host.username,
                    host.created_at.to_rfc3339()
                ],
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let loaded = database.find_host(host.id).unwrap().unwrap();
        assert!(loaded.group_id.is_some());
        assert_eq!(
            database.list_groups().unwrap()[0].name,
            "Legacy / Production"
        );
        drop(database);
        fs::remove_file(path).unwrap();
    }
}
