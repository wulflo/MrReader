// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let s = Self {
            conn: Mutex::new(conn),
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let s = Self {
            conn: Mutex::new(conn),
        };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL,
                line_count INTEGER NOT NULL DEFAULT 0,
                indexed_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS word_counts (
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                word TEXT NOT NULL,
                display_form TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (file_id, word)
            );
            CREATE INDEX IF NOT EXISTS idx_word_counts_word ON word_counts(word);
            CREATE TABLE IF NOT EXISTS top_words (
                word TEXT PRIMARY KEY,
                display_form TEXT NOT NULL,
                count INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn file_state(&self, path: &str) -> Result<Option<(i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT mtime, size FROM files WHERE path = ?1")?;
        let mut rows = stmt.query(params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    /// Reindexa un archivo: resta conteos anteriores, actualiza metadatos, inserta nuevos conteos.
    pub fn index_file(
        &self,
        path: &str,
        mtime: i64,
        size: i64,
        line_count: i64,
        counts: &[(String, String, u32)],
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        // buscar id existente
        let existing_id: Option<i64> = tx
            .query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| r.get(0))
            .ok();
        if let Some(fid) = existing_id {
            // restar conteos anteriores de top_words
            let mut stmt = tx.prepare("SELECT word, count FROM word_counts WHERE file_id = ?1")?;
            let rows: Vec<(String, i64)> = stmt
                .query_map(params![fid], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            for (word, c) in rows {
                tx.execute(
                    "UPDATE top_words SET count = count - ?1 WHERE word = ?2",
                    params![c, word],
                )?;
                tx.execute("DELETE FROM top_words WHERE word = ?1 AND count <= 0", params![word])?;
            }
            tx.execute("DELETE FROM word_counts WHERE file_id = ?1", params![fid])?;
            tx.execute(
                "UPDATE files SET mtime=?1, size=?2, line_count=?3, indexed_at=?4 WHERE id=?5",
                params![mtime, size, line_count, now_ts(), fid],
            )?;
            // insertar nuevos
            for (word, display, cnt) in counts {
                tx.execute(
                    "INSERT INTO word_counts (file_id, word, display_form, count) VALUES (?1,?2,?3,?4)",
                    params![fid, word, display, *cnt as i64],
                )?;
                // upsert top_words
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM top_words WHERE word = ?1",
                    params![word],
                    |r| r.get(0),
                )?;
                if exists > 0 {
                    tx.execute(
                        "UPDATE top_words SET count = count + ?1, display_form = ?2 WHERE word = ?3",
                        params![*cnt as i64, display, word],
                    )?;
                } else {
                    tx.execute(
                        "INSERT INTO top_words (word, display_form, count) VALUES (?1,?2,?3)",
                        params![word, display, *cnt as i64],
                    )?;
                }
            }
        } else {
            tx.execute(
                "INSERT INTO files (path, mtime, size, line_count, indexed_at) VALUES (?1,?2,?3,?4,?5)",
                params![path, mtime, size, line_count, now_ts()],
            )?;
            let fid = tx.last_insert_rowid();
            for (word, display, cnt) in counts {
                tx.execute(
                    "INSERT INTO word_counts (file_id, word, display_form, count) VALUES (?1,?2,?3,?4)",
                    params![fid, word, display, *cnt as i64],
                )?;
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM top_words WHERE word = ?1",
                    params![word],
                    |r| r.get(0),
                )?;
                if exists > 0 {
                    tx.execute(
                        "UPDATE top_words SET count = count + ?1, display_form = ?2 WHERE word = ?3",
                        params![*cnt as i64, display, word],
                    )?;
                } else {
                    tx.execute(
                        "INSERT INTO top_words (word, display_form, count) VALUES (?1,?2,?3)",
                        params![word, display, *cnt as i64],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_file(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let fid: Option<i64> = tx
            .query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| r.get(0))
            .ok();
        if let Some(fid) = fid {
            let rows: Vec<(String, i64)> = {
                let mut stmt = tx.prepare("SELECT word, count FROM word_counts WHERE file_id = ?1")?;
                let v = stmt
                    .query_map(params![fid], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .filter_map(|r| r.ok())
                    .collect();
                v
            };
            for (word, c) in rows {
                tx.execute(
                    "UPDATE top_words SET count = count - ?1 WHERE word = ?2",
                    params![c, word],
                )?;
                tx.execute("DELETE FROM top_words WHERE word = ?1 AND count <= 0", params![word])?;
            }
            tx.execute("DELETE FROM files WHERE id = ?1", params![fid])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn top_words(&self, n: usize) -> Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT word, display_form, count FROM top_words ORDER BY count DESC, word ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![n as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn file_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let c: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(c)
    }

    pub fn all_paths_sorted(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path FROM files ORDER BY path ASC")?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Vacía la caché por completo. Se usa al cambiar de carpeta para no
    /// mezclar notas de la carpeta anterior con las de la nueva.
    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM word_counts; DELETE FROM files; DELETE FROM top_words;",
        )?;
        Ok(())
    }

    pub fn random_path(&self, exclude: Option<&str>) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        // intenta hasta 5 veces evitar el actual
        for _ in 0..5 {
            let p: Option<String> = conn
                .query_row(
                    "SELECT path FROM files ORDER BY RANDOM() LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok();
            if let Some(ref pth) = p {
                if Some(pth.as_str()) != exclude {
                    return Ok(p);
                }
            } else {
                return Ok(None);
            }
        }
        // fallback
        let p: Option<String> = conn
            .query_row("SELECT path FROM files ORDER BY RANDOM() LIMIT 1", [], |r| r.get(0))
            .ok();
        Ok(p)
    }

    pub fn path_at_offset(&self, offset: usize) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let p: Option<String> = conn
            .query_row(
                "SELECT path FROM files ORDER BY path ASC LIMIT 1 OFFSET ?1",
                params![offset as i64],
                |r| r.get(0),
            )
            .ok();
        Ok(p)
    }
}

fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod schema {
    use super::*;
    #[test]
    fn creates_tables() {
        let s = Store::open_in_memory().unwrap();
        // smoke: counts
        assert_eq!(s.file_count().unwrap(), 0);
        assert!(s.top_words(5).unwrap().is_empty());
    }
    #[test]
    fn idempotent_migrate() {
        let s = Store::open_in_memory().unwrap();
        s.migrate().unwrap();
        assert_eq!(s.file_count().unwrap(), 0);
    }
}

#[cfg(test)]
mod counts {
    use super::*;
    #[test]
    fn incremental_add_and_remove() {
        let s = Store::open_in_memory().unwrap();
        s.index_file("/a.md", 1, 10, 2, &[("rust".into(), "Rust".into(), 2)]).unwrap();
        assert_eq!(s.file_count().unwrap(), 1);
        let top = s.top_words(10).unwrap();
        assert_eq!(top[0].0, "rust");
        assert_eq!(top[0].2, 2);
        // reindex with different counts
        s.index_file("/a.md", 2, 10, 2, &[("rust".into(), "Rust".into(), 1), ("go".into(), "Go".into(), 3)]).unwrap();
        let top = s.top_words(10).unwrap();
        let rust = top.iter().find(|x| x.0 == "rust").unwrap();
        assert_eq!(rust.2, 1);
        let go = top.iter().find(|x| x.0 == "go").unwrap();
        assert_eq!(go.2, 3);
        // remove
        s.remove_file("/a.md").unwrap();
        assert_eq!(s.file_count().unwrap(), 0);
        assert!(s.top_words(10).unwrap().is_empty());
    }

    #[test]
    fn clear_removes_everything() {
        let s = Store::open_in_memory().unwrap();
        s.index_file("/a.md", 1, 10, 1, &[("rust".into(), "Rust".into(), 2)]).unwrap();
        s.index_file("/b.md", 1, 10, 1, &[("gtk".into(), "GTK".into(), 3)]).unwrap();
        assert_eq!(s.file_count().unwrap(), 2);
        s.clear().unwrap();
        assert_eq!(s.file_count().unwrap(), 0);
        assert!(s.top_words(10).unwrap().is_empty());
        assert!(s.all_paths_sorted().unwrap().is_empty());
    }
}

#[cfg(test)]
mod top {
    use super::*;
    #[test]
    fn top_n_ordering() {
        let s = Store::open_in_memory().unwrap();
        s.index_file("/a.md", 1, 10, 1, &[("a".into(), "a".into(), 5), ("b".into(), "b".into(), 2)]).unwrap();
        s.index_file("/b.md", 1, 10, 1, &[("b".into(), "b".into(), 4)]).unwrap();
        let top = s.top_words(2).unwrap();
        assert_eq!(top[0].0, "b");
        assert_eq!(top[0].2, 6);
        assert_eq!(top[1].0, "a");
        assert_eq!(top[1].2, 5);
    }
}
