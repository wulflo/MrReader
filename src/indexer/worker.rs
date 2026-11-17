// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::analysis;
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub mtime: i64,
    pub size: i64,
    pub line_count: i64,
    pub counts: Vec<(String, String, u32)>,
}

pub fn pool_size(configured: Option<usize>) -> usize {
    let n = configured.unwrap_or_else(|| {
        let c = num_cpus::get();
        if c > 1 { c - 1 } else { 1 }
    });
    if n == 0 { 1 } else { n }
}

pub fn process_one(
    path: &Path,
    stopwords: &[String],
    min_len: usize,
    include_code: bool,
) -> Result<IndexedFile> {
    use crate::indexer::change;
    let (mtime, size) = change::file_meta(path).ok_or_else(|| anyhow::anyhow!("metadata"))?;
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let cleaned = analysis::clean_markdown(&text, include_code);
    let line_count = cleaned.lines().filter(|l| !l.trim().is_empty()).count() as i64;
    let counts = analysis::count_words(&cleaned, stopwords, min_len);
    Ok(IndexedFile {
        path: path.to_path_buf(),
        mtime,
        size,
        line_count,
        counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn process_one_counts() {
        let dir = std::env::temp_dir().join("tm_worker");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), "# T\n\nrust rust\n").unwrap();
        let sw = crate::analysis::default_stopwords();
        let r = process_one(&dir.join("a.md"), &sw, 3, false).unwrap();
        assert!(r.counts.iter().any(|x| x.0 == "rust" && x.2 == 2));
        assert_eq!(r.line_count, 2);
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn pool_size_min_one() {
        assert!(pool_size(Some(0)) >= 1);
        assert!(pool_size(Some(4)) == 4);
    }
}
