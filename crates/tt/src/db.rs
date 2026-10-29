//! SQLite persistence: users, login sessions, saved hosts and trusted host keys.
use std::{path::Path, sync::Mutex};

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub created_at: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// What the API returns for a saved host (never contains secrets).
#[derive(Debug, Clone, Serialize)]
pub struct HostView {
    pub id: i64,
    pub name: String,
    pub folder: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: String,
    pub profile_id: String,
    pub legacy: bool,
    pub has_secret: bool,
    pub has_passphrase: bool,
    pub has_enable: bool,
    pub notes: String,
}

/// What the API accepts. Empty secret fields keep the stored value on update.
#[derive(Debug, Clone, Deserialize)]
pub struct HostInput {
    pub name: String,
    #[serde(default)]
    pub folder: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: String,
    #[serde(default)]
    pub profile_id: String,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub enable_secret: String,
}

/// Stored row including encrypted secrets (server-side use only).
pub struct HostRow {
    pub view: HostView,
    pub secret_enc: String,
    pub passphrase_enc: String,
    pub enable_enc: String,
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS users(
  id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE COLLATE NOCASE,
  pass_hash TEXT NOT NULL, role TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS auth_sessions(
  token_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS hosts(
  id INTEGER PRIMARY KEY, owner_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL, folder TEXT NOT NULL DEFAULT '', host TEXT NOT NULL, port INTEGER NOT NULL,
  username TEXT NOT NULL, auth_kind TEXT NOT NULL, profile_id TEXT NOT NULL DEFAULT 'generic',
  legacy INTEGER NOT NULL DEFAULT 0, notes TEXT NOT NULL DEFAULT '',
  secret_enc TEXT NOT NULL DEFAULT '', passphrase_enc TEXT NOT NULL DEFAULT '',
  enable_enc TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS known_hosts(
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  host TEXT NOT NULL, port INTEGER NOT NULL, key_type TEXT NOT NULL, fingerprint TEXT NOT NULL,
  PRIMARY KEY(user_id, host, port));
";

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- users -------------------------------------------------------------
    pub fn user_count(&self) -> Result<i64> {
        Ok(self.c().query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
    }

    /// Creates the first admin atomically; fails if any user already exists.
    pub fn create_first_admin(&self, username: &str, pass_hash: &str) -> Result<i64> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n: i64 = tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        if n > 0 {
            bail!("setup already completed");
        }
        tx.execute(
            "INSERT INTO users(username,pass_hash,role,created_at) VALUES(?,?, 'admin', ?)",
            params![username, pass_hash, now()],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn create_user(&self, username: &str, pass_hash: &str, role: &str) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO users(username,pass_hash,role,created_at) VALUES(?,?,?,?)",
            params![username, pass_hash, role, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    fn user_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
        Ok(User { id: r.get(0)?, username: r.get(1)?, role: r.get(2)?, created_at: r.get(3)? })
    }

    pub fn user_by_name(&self, username: &str) -> Result<Option<(User, String)>> {
        Ok(self
            .c()
            .query_row(
                "SELECT id,username,role,created_at,pass_hash FROM users WHERE username=?",
                [username],
                |r| Ok((Self::user_from(r)?, r.get::<_, String>(4)?)),
            )
            .optional()?)
    }

    pub fn list_users(&self) -> Result<Vec<User>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id,username,role,created_at FROM users ORDER BY username")?;
        let rows = st.query_map([], Self::user_from)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_user(&self, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM users WHERE id=?", [id])?;
        Ok(())
    }

    pub fn admin_count(&self) -> Result<i64> {
        Ok(self.c().query_row("SELECT COUNT(*) FROM users WHERE role='admin'", [], |r| r.get(0))?)
    }

    pub fn set_password(&self, id: i64, pass_hash: &str) -> Result<()> {
        let c = self.c();
        c.execute("UPDATE users SET pass_hash=? WHERE id=?", [pass_hash, &id.to_string()])?;
        c.execute("DELETE FROM auth_sessions WHERE user_id=?", [id])?;
        Ok(())
    }

    pub fn get_user(&self, id: i64) -> Result<Option<User>> {
        Ok(self
            .c()
            .query_row("SELECT id,username,role,created_at FROM users WHERE id=?", [id], Self::user_from)
            .optional()?)
    }

    // ---- login sessions ----------------------------------------------------
    pub fn create_session(&self, token_hash: &str, user_id: i64, expires_at: i64) -> Result<()> {
        let c = self.c();
        c.execute("DELETE FROM auth_sessions WHERE expires_at < ?", [now()])?;
        c.execute(
            "INSERT INTO auth_sessions(token_hash,user_id,expires_at) VALUES(?,?,?)",
            params![token_hash, user_id, expires_at],
        )?;
        Ok(())
    }

    pub fn user_by_session(&self, token_hash: &str) -> Result<Option<User>> {
        Ok(self
            .c()
            .query_row(
                "SELECT u.id,u.username,u.role,u.created_at FROM auth_sessions s
                 JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.expires_at>?",
                params![token_hash, now()],
                Self::user_from,
            )
            .optional()?)
    }

    pub fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.c().execute("DELETE FROM auth_sessions WHERE token_hash=?", [token_hash])?;
        Ok(())
    }

    // ---- hosts -------------------------------------------------------------
    fn host_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<HostRow> {
        let secret_enc: String = r.get(12)?;
        let passphrase_enc: String = r.get(13)?;
        let enable_enc: String = r.get(14)?;
        Ok(HostRow {
            view: HostView {
                id: r.get(0)?,
                name: r.get(1)?,
                folder: r.get(2)?,
                host: r.get(3)?,
                port: r.get::<_, i64>(4)? as u16,
                username: r.get(5)?,
                auth_kind: r.get(6)?,
                profile_id: r.get(7)?,
                legacy: r.get::<_, i64>(8)? != 0,
                notes: r.get(9)?,
                has_secret: !secret_enc.is_empty(),
                has_passphrase: !passphrase_enc.is_empty(),
                has_enable: !enable_enc.is_empty(),
            },
            secret_enc,
            passphrase_enc,
            enable_enc,
        })
    }

    const HOST_COLS: &'static str = "id,name,folder,host,port,username,auth_kind,profile_id,legacy,notes,owner_id,0,secret_enc,passphrase_enc,enable_enc";

    pub fn list_hosts(&self, owner: i64) -> Result<Vec<HostView>> {
        let c = self.c();
        let sql = format!("SELECT {} FROM hosts WHERE owner_id=? ORDER BY folder, name", Self::HOST_COLS);
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([owner], Self::host_from)?;
        Ok(rows.map(|r| r.map(|h| h.view)).collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_host(&self, owner: i64, id: i64) -> Result<Option<HostRow>> {
        let sql = format!("SELECT {} FROM hosts WHERE owner_id=? AND id=?", Self::HOST_COLS);
        Ok(self.c().query_row(&sql, params![owner, id], Self::host_from).optional()?)
    }

    pub fn insert_host(&self, owner: i64, h: &HostInput, secret: &str, pass: &str, enable: &str) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO hosts(owner_id,name,folder,host,port,username,auth_kind,profile_id,legacy,notes,secret_enc,passphrase_enc,enable_enc)
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![owner, h.name, h.folder, h.host, h.port, h.username, h.auth_kind, h.profile_id, h.legacy as i64, h.notes, secret, pass, enable],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// `None` for a secret keeps the stored value.
    pub fn update_host(&self, owner: i64, id: i64, h: &HostInput, secret: Option<&str>, pass: Option<&str>, enable: Option<&str>) -> Result<bool> {
        let c = self.c();
        let n = c.execute(
            "UPDATE hosts SET name=?,folder=?,host=?,port=?,username=?,auth_kind=?,profile_id=?,legacy=?,notes=?,
               secret_enc=COALESCE(?,secret_enc),passphrase_enc=COALESCE(?,passphrase_enc),enable_enc=COALESCE(?,enable_enc)
             WHERE owner_id=? AND id=?",
            params![h.name, h.folder, h.host, h.port, h.username, h.auth_kind, h.profile_id, h.legacy as i64, h.notes, secret, pass, enable, owner, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_host(&self, owner: i64, id: i64) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM hosts WHERE owner_id=? AND id=?", [owner, id])? > 0)
    }

    // ---- known hosts -------------------------------------------------------
    pub fn known_fingerprint(&self, user: i64, host: &str, port: u16) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT fingerprint FROM known_hosts WHERE user_id=? AND host=? AND port=?",
                params![user, host, port],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn trust_host(&self, user: i64, host: &str, port: u16, key_type: &str, fp: &str) -> Result<()> {
        self.c().execute(
            "INSERT OR REPLACE INTO known_hosts(user_id,host,port,key_type,fingerprint) VALUES(?,?,?,?,?)",
            params![user, host, port, key_type, fp],
        )?;
        Ok(())
    }

    pub fn forget_host(&self, user: i64, host: &str, port: u16) -> Result<()> {
        self.c().execute(
            "DELETE FROM known_hosts WHERE user_id=? AND host=? AND port=?",
            params![user, host, port],
        )?;
        Ok(())
    }
}
