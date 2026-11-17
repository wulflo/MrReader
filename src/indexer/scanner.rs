// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn scan_markdown(root: &Path, max_size: u64) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    // El usuario puede elegir una carpeta o un único archivo Markdown.
    if root.is_file() {
        if is_markdown(root)
            && std::fs::metadata(root)
                .map(|m| m.len() <= max_size)
                .unwrap_or(false)
        {
            out.push(root.to_path_buf());
        }
        return out;
    }
    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if !is_markdown(p) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.len() > max_size {
                continue;
            }
        }
        out.push(p.to_path_buf());
    }
    out
}

fn is_markdown(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
}

#[cfg(test)]
mod scan {
    use super::*;
    use std::fs;
    #[test]
    fn finds_md_files() {
        let dir = std::env::temp_dir().join("tm_scan_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), "x").unwrap();
        fs::write(dir.join("b.txt"), "x").unwrap();
        fs::write(dir.join("c.markdown"), "x").unwrap();
        let files = scan_markdown(&dir, 1024 * 1024);
        assert_eq!(files.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn acepta_un_archivo_individual() {
        let dir = std::env::temp_dir().join("tm_scan_single");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("nota.md");
        fs::write(&file, "hola").unwrap();
        let files = scan_markdown(&file, 1024 * 1024);
        assert_eq!(files, vec![file.clone()]);
        assert!(scan_markdown(&dir.join("nada.txt"), 1024 * 1024).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn ignores_large_files() {
        let dir = std::env::temp_dir().join("tm_scan_large");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("big.md"), vec![b'x'; 100]).unwrap();
        let files = scan_markdown(&dir, 10);
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
