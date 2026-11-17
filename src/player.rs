// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::analysis;
use anyhow::Result;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayOrder {
    Alphabetical,
    Random,
}

/// Lee las líneas reproducibles de un archivo Markdown.
pub fn read_lines(path: &Path, include_code: bool) -> Result<Vec<String>> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let cleaned = analysis::clean_markdown(&text, include_code);
    let mut lines = Vec::new();
    for raw in cleaned.lines() {
        let t = raw.trim();
        if !t.is_empty() {
            lines.push(t.to_string());
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod lines {
    use super::*;
    use std::io::Write;
    #[test]
    fn reads_and_cleans_lines() {
        let dir = std::env::temp_dir().join("tm_test_lines");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.md");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "# Título\n").unwrap();
        writeln!(f, "Primera línea **bold**").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "Segunda línea").unwrap();
        let lines = read_lines(&p, false).unwrap();
        assert_eq!(lines, vec!["Título", "Primera línea bold", "Segunda línea"]);
    }
}
