use std::path::Path;
use rusqlite::{params, Connection};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: i64,
    pub created_at: i64,
    pub target_lang: String,
    pub provider_key: String,
    pub source_markdown: String,
    pub translated_markdown: String,
    pub thumbnail_base64: String,
}
pub struct Cache {
    conn: Mutex<Connection>,
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        Self::init(&conn).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(&conn).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS translations (
                 source_text TEXT NOT NULL,
                 target_lang TEXT NOT NULL,
                 provider_key TEXT NOT NULL,
                 translated_text TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (source_text, target_lang, provider_key)
             );
             CREATE INDEX IF NOT EXISTS idx_trans_lookup ON translations (source_text, target_lang, provider_key);
             CREATE TABLE IF NOT EXISTS history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 created_at INTEGER NOT NULL,
                 target_lang TEXT NOT NULL,
                 provider_key TEXT NOT NULL,
                 source_markdown TEXT NOT NULL,
                 translated_markdown TEXT NOT NULL,
                 thumbnail_base64 TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_history_created ON history (created_at DESC);",
        )
    }

    pub fn get(&self, source_text: &str, target_lang: &str, provider_key: &str) -> Option<String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT translated_text FROM translations WHERE source_text = ?1 AND target_lang = ?2 AND provider_key = ?3 LIMIT 1",
            )
            .ok()?;
        stmt.query_row(params![source_text, target_lang, provider_key], |row| row.get(0))
            .ok()
    }

    pub fn set(&self, source_text: &str, target_lang: &str, provider_key: &str, translated_text: &str) {
        let conn = self.conn.lock();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let _ = conn.execute(
            "INSERT INTO translations (source_text, target_lang, provider_key, translated_text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_text, target_lang, provider_key) DO UPDATE SET
                 translated_text = excluded.translated_text,
                 created_at = excluded.created_at",
            params![source_text, target_lang, provider_key, translated_text, now],
        );
    }

    pub fn add_history(
        &self,
        target_lang: &str,
        provider_key: &str,
        source_markdown: &str,
        translated_markdown: &str,
        thumbnail_base64: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO history (created_at, target_lang, provider_key, source_markdown, translated_markdown, thumbnail_base64)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now, target_lang, provider_key, source_markdown, translated_markdown, thumbnail_base64],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_history(&self, limit: usize) -> Result<Vec<HistoryItem>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, target_lang, provider_key, source_markdown, translated_markdown, thumbnail_base64
                 FROM history ORDER BY created_at DESC, id DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(HistoryItem {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    target_lang: row.get(2)?,
                    provider_key: row.get(3)?,
                    source_markdown: row.get(4)?,
                    translated_markdown: row.get(5)?,
                    thumbnail_base64: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let items = rows.flatten().collect();
        Ok(items)
    }

    pub fn delete_history(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM history", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_set_and_get() {
        let cache = Cache::in_memory().unwrap();
        assert_eq!(cache.get("Hello", "vi", "free"), None);

        cache.set("Hello", "vi", "free", "Xin chào");
        assert_eq!(cache.get("Hello", "vi", "free").as_deref(), Some("Xin chào"));

        // Different target language
        assert_eq!(cache.get("Hello", "ja", "free"), None);

        // Different provider
        assert_eq!(cache.get("Hello", "vi", "openai:gpt-4o"), None);

        // Update existing entry
        cache.set("Hello", "vi", "free", "Chào bạn");
        assert_eq!(cache.get("Hello", "vi", "free").as_deref(), Some("Chào bạn"));
    }

    #[test]
    fn cache_persistence_across_connections() {
        let db_path = std::env::temp_dir().join(format!("test_cache_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);

        {
            let cache1 = Cache::open(&db_path).unwrap();
            cache1.set("Paragraph 1", "vi", "free", "Đoạn 1");
            cache1.set("Paragraph 2", "vi", "free", "Đoạn 2");
        }

        // Re-open from disk and verify entries exist
        {
            let cache2 = Cache::open(&db_path).unwrap();
            assert_eq!(cache2.get("Paragraph 1", "vi", "free").as_deref(), Some("Đoạn 1"));
            assert_eq!(cache2.get("Paragraph 2", "vi", "free").as_deref(), Some("Đoạn 2"));
            assert_eq!(cache2.get("Paragraph 3", "vi", "free"), None);
        }
    }

    #[test]
    fn history_crud_operations() {
        let cache = Cache::in_memory().unwrap();
        assert!(cache.get_history(10).unwrap().is_empty());

        let id1 = cache
            .add_history("vi", "free", "Source text 1", "Dịch 1", "data:image/png;base64,thumb1")
            .unwrap();
        let id2 = cache
            .add_history("vi", "openai:gpt", "Source text 2", "Dịch 2", "data:image/png;base64,thumb2")
            .unwrap();

        let list = cache.get_history(10).unwrap();
        assert_eq!(list.len(), 2);
        // Latest first
        assert_eq!(list[0].id, id2);
        assert_eq!(list[0].translated_markdown, "Dịch 2");
        assert_eq!(list[1].id, id1);

        // Delete one
        cache.delete_history(id2).unwrap();
        let list_after = cache.get_history(10).unwrap();
        assert_eq!(list_after.len(), 1);
        assert_eq!(list_after[0].id, id1);

        // Clear all
        cache.clear_history().unwrap();
        assert!(cache.get_history(10).unwrap().is_empty());
    }
}
