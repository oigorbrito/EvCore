use std::time::Duration;

use rusqlite::Connection;

use super::MemoryError;

pub(super) const SCHEMA_VERSION: i64 = 2;

pub(super) const RECORD_COLUMNS: &str = "
    m.public_id, m.content, m.kind, m.is_global, m.workspace_id,
    m.workspace_root, m.source_call_id, m.created_at, m.updated_at
";

pub(super) const MIGRATE_V1_TO_V2: &str = r#"
ALTER TABLE memories ADD COLUMN public_id TEXT;
UPDATE memories
SET public_id = 'mem_' || lower(hex(randomblob(16)))
WHERE public_id IS NULL;
CREATE UNIQUE INDEX memories_public_id ON memories(public_id);
PRAGMA user_version = 2;
"#;

pub(super) const SCHEMA: &str = r#"
CREATE TABLE memories (
    id INTEGER PRIMARY KEY,
    public_id TEXT NOT NULL UNIQUE
        CHECK (length(public_id) = 36 AND substr(public_id, 1, 4) = 'mem_'),
    content TEXT NOT NULL,
    kind TEXT NOT NULL,
    is_global INTEGER NOT NULL CHECK (is_global IN (0, 1)),
    workspace_id TEXT,
    workspace_root TEXT NOT NULL,
    source_call_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((is_global = 1 AND workspace_id IS NULL) OR
           (is_global = 0 AND workspace_id IS NOT NULL))
);
CREATE INDEX memories_scope ON memories(is_global, workspace_id, updated_at DESC);
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    kind,
    content='memories',
    content_rowid='id',
    tokenize='unicode61'
);
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content, kind)
    VALUES (new.id, new.content, new.kind);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, kind)
    VALUES ('delete', old.id, old.content, old.kind);
END;
CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, kind)
    VALUES ('delete', old.id, old.content, old.kind);
    INSERT INTO memories_fts(rowid, content, kind)
    VALUES (new.id, new.content, new.kind);
END;
PRAGMA user_version = 2;
"#;

/// Switch a fresh connection to WAL, tolerating a concurrent first open.
///
/// SQLite answers a journal-mode change with `SQLITE_BUSY` without consulting
/// the busy handler while another connection holds the database, so the
/// configured `busy_timeout` does not cover this one statement. Retry within
/// the same budget, and accept the mode another opener has already installed.
pub(super) fn set_wal(connection: &Connection) -> Result<(), MemoryError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match connection.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => return Ok(()),
            Err(error) => {
                let busy = matches!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                );
                let wal = connection
                    .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                    .is_ok_and(|mode| mode.eq_ignore_ascii_case("wal"));
                if wal {
                    return Ok(());
                }
                if !busy || std::time::Instant::now() >= deadline {
                    return Err(error.into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
