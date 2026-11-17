// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::store::Store;
use std::path::Path;

/// Determina si un archivo debe reindexarse comparando mtime y tamaño con la caché.
pub fn needs_reindex(store: &Store, path: &str, mtime: i64, size: i64) -> bool {
    match store.file_state(path) {
        Ok(Some((om, os))) => om != mtime || os != size,
        Ok(None) => true,
        Err(_) => true,
    }
}

/// Metadatos básicos de un archivo.
pub fn file_meta(path: &Path) -> Option<(i64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((mtime, meta.len() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::fs;
    #[test]
    fn detects_new_and_changed() {
        let s = Store::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join("tm_change");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.md");
        fs::write(&p, "hola").unwrap();
        let (m, sz) = file_meta(&p).unwrap();
        assert!(needs_reindex(&s, p.to_str().unwrap(), m, sz));
        s.index_file(p.to_str().unwrap(), m, sz, 1, &[]).unwrap();
        assert!(!needs_reindex(&s, p.to_str().unwrap(), m, sz));
        // changed size
        assert!(needs_reindex(&s, p.to_str().unwrap(), m, sz + 1));
        let _ = fs::remove_dir_all(&dir);
    }
}
