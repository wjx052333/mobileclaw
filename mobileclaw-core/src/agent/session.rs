//! Session persistence: JSONL transcript save/load/list/delete.
//!
//! Sessions are written as one JSON object per line (JSONL format), matching
//! the claude-code transcript format. Writes are atomic: data goes to a `.tmp`
//! file first, then `fsync` + `rename()` to the final path.
//!
//! Path safety: all file operations are scoped under the caller-specified
//! `session_dir` — no writes outside this directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::ClawResult;
use crate::llm::types::Message;

// ─── DTOs ────────────────────────────────────────────────────────────────────

/// Summary of a saved session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    /// Unique session ID derived from filename (without extension).
    pub id: String,
    /// File modification time (Unix epoch seconds).
    pub modified: u64,
    /// Number of messages in the transcript.
    pub message_count: usize,
    /// Absolute path to the `.jsonl` file.
    pub file_path: String,
}

// ─── Timestamp helper ────────────────────────────────────────────────────────

fn timestamp_hex() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}{:09x}", now.as_secs(), now.subsec_nanos())
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Save messages to a JSONL session file in the given directory.
///
/// **Atomic**: writes to `{id}.tmp`, syncs to disk, then renames to `{id}.jsonl`.
/// This guarantees no corrupted file even if the process crashes mid-write.
///
/// Returns the absolute path of the created session file.
pub async fn save_session(dir: &Path, messages: &[Message]) -> ClawResult<PathBuf> {
    // SAFETY: dir must be absolute — relative paths could escape sandbox
    if !dir.is_absolute() {
        return Err(crate::ClawError::Session("session_dir must be an absolute path".into()));
    }

    // Create directory if missing (idempotent)
    if !dir.exists() {
        tokio::fs::create_dir_all(dir).await
            .map_err(|e| crate::ClawError::Session(format!("create_dir_all: {}", e)))?;
    }

    let id = format!("session_{}", timestamp_hex());
    let tmp_path = dir.join(format!("{}.tmp", id));
    let final_path = dir.join(format!("{}.jsonl", id));

    let serialized_count = messages.len();
    {
        let mut f = tokio::fs::File::create(&tmp_path).await
            .map_err(|e| crate::ClawError::Session(format!("create tmp: {}", e)))?;

        for msg in messages {
            let line = serde_json::to_string(msg)?;
            f.write_all(line.as_bytes()).await
                .map_err(|e| crate::ClawError::Session(format!("write line: {}", e)))?;
            f.write_all(b"\n").await
                .map_err(|e| crate::ClawError::Session(format!("write newline: {}", e)))?;
        }

        // fsync before rename — ensures data is on disk
        f.sync_all().await
            .map_err(|e| crate::ClawError::Session(format!("sync: {}", e)))?;
    }

    // Atomic rename
    tokio::fs::rename(&tmp_path, &final_path).await
        .map_err(|e| crate::ClawError::Session(format!("rename: {}", e)))?;

    // Verify: count should match (corruption guard)
    let content = tokio::fs::read_to_string(&final_path).await.unwrap_or_default();
    let lines = content.lines().filter(|l| !l.is_empty()).count();
    if lines != serialized_count {
        return Err(crate::ClawError::Session(format!(
            "written {serialized_count} messages but file has {lines} lines"
        )));
    }

    Ok(final_path)
}

/// Load a session from a JSONL transcript file.
///
/// Returns the parsed message vector. Empty lines are silently skipped.
pub async fn load_session(file_path: &Path) -> ClawResult<Vec<Message>> {
    if !file_path.exists() {
        return Err(crate::ClawError::Session(format!(
            "session file not found: {}", file_path.display()
        )));
    }

    // Validate: must be a .jsonl file under some directory (basic guard)
    if file_path.extension().is_none_or(|e| e != "jsonl") {
        return Err(crate::ClawError::Session(
            "session file must have .jsonl extension".into(),
        ));
    }

    let content = tokio::fs::read_to_string(file_path).await
        .map_err(|e| crate::ClawError::Session(format!("read: {}", e)))?;

    let mut messages = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let msg: Message = serde_json::from_str(line)
            .map_err(|e| crate::ClawError::Session(format!(
                "parse error at line {}: {}", line_num + 1, e
            )))?;
        messages.push(msg);
    }

    Ok(messages)
}

/// List all saved sessions in the given directory.
///
/// Returns sessions sorted by modification time (newest first).
/// Empty `.tmp` files are excluded.
pub async fn list_sessions(dir: &Path) -> ClawResult<Vec<SessionEntry>> {
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut entries = Vec::new();
    let mut dir_stream = tokio::fs::read_dir(dir).await
        .map_err(|e| crate::ClawError::Session(format!("read_dir: {}", e)))?;

    while let Some(dentry) = dir_stream.next_entry().await
        .map_err(|e| crate::ClawError::Session(format!("next_entry: {}", e)))?
    {
        let file_name = dentry.file_name();
        let name_str = file_name.to_string_lossy();

        // Only .jsonl files
        if !name_str.ends_with(".jsonl") {
            continue;
        }

        let metadata = dentry.metadata().await
            .map_err(|e| crate::ClawError::Session(format!("metadata: {}", e)))?;

        let modified = metadata.modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0);

        let file_path = dentry.path();
        let id = name_str.trim_end_matches(".jsonl").to_string();

        // Count messages (fast: just count newlines, JSONL has one object per line)
        let content = tokio::fs::read_to_string(&file_path).await.unwrap_or_default();
        let message_count = content.lines().filter(|l| !l.is_empty()).count();

        entries.push(SessionEntry {
            id,
            modified,
            message_count,
            file_path: file_path.to_string_lossy().to_string(),
        });
    }

    // Sort: newest first
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));

    Ok(entries)
}

/// Delete a session file.
///
/// Path safety: the file must be under `session_dir` to prevent traversal attacks.
/// Returns true if the file existed and was deleted.
pub async fn delete_session(session_dir: &Path, file_path: &Path) -> ClawResult<bool> {
    // Validate: file_path must be under session_dir
    let abs = file_path.canonicalize().ok();
    if let Some(abs_path) = abs {
        let dir_abs = session_dir.canonicalize().ok();
        if let Some(dir_p) = dir_abs {
            if !abs_path.starts_with(&dir_p) {
                return Err(crate::ClawError::Session(
                    "session file must be under session_dir".into(),
                ));
            }
        }
    }

    if !file_path.exists() {
        return Ok(false);
    }

    tokio::fs::remove_file(file_path).await
        .map_err(|e| crate::ClawError::Session(format!("remove: {}", e)))?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // save_session + load_session round-trip
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_and_load_session_round_trip() {
        let dir = TempDir::new().unwrap();
        let msgs = vec![
            Message::user("hello"),
            Message::assistant("hi!"),
        ];
        let path = save_session(dir.path(), &msgs).await.unwrap();
        let loaded = load_session(&path).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, crate::llm::types::Role::User);
        assert_eq!(loaded[1].text_content(), "hi!");
    }

    #[tokio::test]
    async fn save_session_atomic_rename_no_tmp_leftover() {
        let dir = TempDir::new().unwrap();
        save_session(dir.path(), &[Message::user("x")]).await.unwrap();

        let mut entries = std::fs::read_dir(&dir).unwrap();
        let mut files = Vec::new();
        while let Some(e) = entries.next().transpose().unwrap() {
            files.push(e.file_name().to_string_lossy().to_string());
        }
        assert_eq!(files.len(), 1, "only final .jsonl file, no .tmp");
        assert!(files[0].ends_with(".jsonl"), "file must be .jsonl, got {}", files[0]);
    }

    #[tokio::test]
    async fn save_session_creates_directory() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("sessions");
        save_session(&subdir, &[Message::user("x")]).await.unwrap();
        assert!(subdir.exists(), "session dir must be auto-created");
    }

    #[tokio::test]
    async fn save_session_refuses_relative_path() {
        let result = save_session(Path::new("sessions"), &[Message::user("x")]).await;
        assert!(result.is_err(), "relative session_dir must be rejected");
    }

    // -----------------------------------------------------------------------
    // load_session error cases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn load_session_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.jsonl");
        let result = load_session(&path).await;
        assert!(result.is_err(), "missing file must return error");
    }

    #[tokio::test]
    async fn load_session_wrong_extension() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.txt");
        std::fs::write(&path, "").unwrap();
        let result = load_session(&path).await;
        assert!(result.is_err(), "non-jsonl extension must be rejected");
    }

    #[tokio::test]
    async fn load_skips_empty_lines() {
        let dir = TempDir::new().unwrap();
        let path = save_session(dir.path(), &[Message::user("a")]).await.unwrap();
        // Append an empty line
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(&mut f).unwrap();
        let loaded = load_session(&path).await.unwrap();
        assert_eq!(loaded.len(), 1, "empty lines must be skipped");
    }

    // -----------------------------------------------------------------------
    // list_sessions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_sessions_empty_dir() {
        let dir = TempDir::new().unwrap();
        let entries = list_sessions(dir.path()).await.unwrap();
        assert!(entries.is_empty(), "empty dir must return empty list");
    }

    #[tokio::test]
    async fn list_sessions_returns_sorted_newest_first() {
        let dir = TempDir::new().unwrap();
        save_session(dir.path(), &[Message::user("first")]).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        save_session(dir.path(), &[Message::user("second")]).await.unwrap();

        let entries = list_sessions(dir.path()).await.unwrap();
        assert_eq!(entries.len(), 2);
        // "second" was saved later → should appear first
        assert!(entries[0].modified >= entries[1].modified, "newest first");
    }

    #[tokio::test]
    async fn list_sessions_correct_message_count() {
        let dir = TempDir::new().unwrap();
        save_session(dir.path(), &[Message::user("a"), Message::assistant("b")]).await.unwrap();
        let entries = list_sessions(dir.path()).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_count, 2);
    }

    // -----------------------------------------------------------------------
    // delete_session
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn delete_session_existing() {
        let dir = TempDir::new().unwrap();
        let path = save_session(dir.path(), &[Message::user("a")]).await.unwrap();
        let deleted = delete_session(dir.path(), &path).await.unwrap();
        assert!(deleted, "existing file must be deleted");
        assert!(!path.exists(), "file must not exist after delete");
    }

    #[tokio::test]
    async fn delete_session_nonexistent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ghost.jsonl");
        let deleted = delete_session(dir.path(), &path).await.unwrap();
        assert!(!deleted, "nonexistent file must return false");
    }

    #[tokio::test]
    async fn delete_session_refuses_outside_dir() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let path = save_session(dir2.path(), &[Message::user("x")]).await.unwrap();
        // Try to delete using dir1 as the session_dir — should fail
        let result = delete_session(dir1.path(), &path).await;
        assert!(result.is_err(), "must reject file outside session_dir");
    }
}
