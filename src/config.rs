// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "mr-reader";
/// Nombres usados por versiones anteriores de la aplicación.
const LEGACY_APP_DIRS: &[&str] = &["marquesina-markdown", "text-motion"];

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "wulflo", APP_DIR)
}

/// Copia `src` a `dst` sólo si el destino no existe todavía.
fn copy_if_missing(src: &Path, dst: &Path) {
    if dst.exists() || !src.exists() {
        return;
    }
    if let Some(parent) = dst.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(?err, path = %parent.display(), "no se pudo crear el directorio de datos");
            return;
        }
    }
    match std::fs::copy(src, dst) {
        Ok(_) => tracing::info!(from = %src.display(), to = %dst.display(), "estado migrado"),
        Err(err) => tracing::warn!(?err, from = %src.display(), to = %dst.display(), "no se pudo migrar el estado"),
    }
}

/// Migra configuración y caché desde el nombre anterior de la aplicación la
/// primera vez que se ejecuta con el nombre nuevo. No sobrescribe nada.
fn migrate_legacy_state() {
    let Some(new_dirs) = project_dirs() else {
        return;
    };

    for legacy in LEGACY_APP_DIRS {
        let Some(old_dirs) = ProjectDirs::from("org", "wulflo", legacy) else {
            continue;
        };

        copy_if_missing(
            &old_dirs.config_dir().join("config.toml"),
            &new_dirs.config_dir().join("config.toml"),
        );

        let old_db = old_dirs.data_dir().join("cache.db");
        let new_db = new_dirs.data_dir().join("cache.db");
        if old_db.exists() && !new_db.exists() {
            copy_if_missing(&old_db, &new_db);
            // SQLite en modo WAL puede dejar estos archivos junto a la base.
            for suffix in ["-wal", "-shm"] {
                let src = old_dirs.data_dir().join(format!("cache.db{suffix}"));
                let dst = new_dirs.data_dir().join(format!("cache.db{suffix}"));
                copy_if_missing(&src, &dst);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub folder: Option<PathBuf>,
    pub top_n: usize,
    pub min_word_len: usize,
    /// Tamaño normal del texto (pt). Todas las palabras no destacadas usan
    /// exactamente este tamaño, sin importar la longitud de la línea.
    #[serde(default = "default_base_font_size")]
    pub base_font_size: i32,
    pub min_font_size: i32,
    pub max_font_size: i32,
    /// Familia tipográfica elegida por el usuario. Si no existe, GTK usa la
    /// siguiente alternativa de la lista.
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Color de acento (hexadecimal) del fondo, glow y controles.
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    pub speed: f64,
    /// Pausa en blanco al terminar cada línea, antes de la siguiente (ms).
    #[serde(default = "default_line_gap_ms")]
    pub line_gap_ms: u64,
    pub order: String, // "alphabetical" | "random"
    pub include_code_blocks: bool,
    pub max_file_size: u64,
    pub worker_threads: Option<usize>,
    pub max_ui_fps: u32,
    pub stopwords: Vec<String>,
    pub colors: Vec<String>,
    /// "auto" (sigue al sistema), "dark" o "light".
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    "dark".into()
}

fn default_base_font_size() -> i32 {
    22
}

fn default_font_family() -> String {
    "FantasqueSansM Nerd Font".into()
}

fn default_accent_color() -> String {
    "#e5304a".into()
}

fn default_line_gap_ms() -> u64 {
    700
}

impl Default for Config {
    fn default() -> Self {
        Self {
            folder: None,
            top_n: 40,
            min_word_len: 3,
            base_font_size: default_base_font_size(),
            min_font_size: 18,
            max_font_size: 72,
            font_family: default_font_family(),
            accent_color: default_accent_color(),
            speed: 1.0,
            line_gap_ms: default_line_gap_ms(),
            order: "alphabetical".into(),
            include_code_blocks: false,
            max_file_size: 5 * 1024 * 1024,
            worker_threads: None,
            max_ui_fps: 10,
            stopwords: Vec::new(),
            theme: default_theme(),
            colors: vec![
                "#ff5555".into(),
                "#50fa7b".into(),
                "#8be9fd".into(),
                "#bd93f9".into(),
                "#ffb86c".into(),
                "#f1fa8c".into(),
                "#ff79c6".into(),
            ],
        }
    }
}

impl Config {
    pub fn load() -> Self {
        migrate_legacy_state();
        let mut cfg = Config::default();
        if let Some(pd) = project_dirs() {
            let path = pd.config_dir().join("config.toml");
            if let Ok(s) = std::fs::read_to_string(&path) {
                if let Ok(parsed) = toml::from_str::<Config>(&s) {
                    cfg = parsed;
                }
            }
        }
        // merge default stopwords if empty
        if cfg.stopwords.is_empty() {
            cfg.stopwords = crate::analysis::default_stopwords();
        } else {
            // normalize
            cfg.stopwords = cfg.stopwords.iter().map(|w| crate::analysis::normalize_word(w)).collect();
        }
        cfg
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(pd) = project_dirs() {
            std::fs::create_dir_all(pd.config_dir())?;
            let s = toml::to_string_pretty(self)?;
            std::fs::write(pd.config_dir().join("config.toml"), s)?;
        }
        Ok(())
    }

    pub fn db_path() -> PathBuf {
        migrate_legacy_state();
        if let Some(pd) = project_dirs() {
            pd.data_dir().join("cache.db")
        } else {
            PathBuf::from("cache.db")
        }
    }
}

/// Expande `~` y `~/...` al directorio home sin invocar un shell.
/// También recorta espacios; el resto se devuelve tal cual.
pub fn expand_tilde(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

pub fn parse_args() -> (Config, Option<PathBuf>) {
    let mut cfg = Config::load();
    let mut folder_arg: Option<PathBuf> = None;
    for arg in std::env::args().skip(1) {
        if arg == "-h" || arg == "--help" {
            println!("Uso: mr-reader [CARPETA]");
            std::process::exit(0);
        } else if !arg.starts_with('-') {
            folder_arg = Some(expand_tilde(&arg));
        }
    }
    if let Some(f) = &folder_arg {
        cfg.folder = Some(f.clone());
    }
    (cfg, folder_arg)
}

#[cfg(test)]
mod paths {
    use super::expand_tilde;

    #[test]
    fn expands_home_prefix() {
        if std::env::var_os("HOME").is_some() {
            let p = expand_tilde("~/notas/md");
            assert!(p.is_absolute());
            assert!(p.ends_with("notas/md"));
        }
    }

    #[test]
    fn keeps_plain_paths_and_trims() {
        assert_eq!(expand_tilde("  notas  "), std::path::PathBuf::from("notas"));
        assert_eq!(expand_tilde("/tmp/x"), std::path::PathBuf::from("/tmp/x"));
        assert_eq!(expand_tilde(""), std::path::PathBuf::from(""));
    }
}

#[cfg(test)]
mod theme {
    use super::{
        Config, default_accent_color, default_base_font_size, default_font_family,
        default_line_gap_ms, default_theme,
    };

    #[test]
    fn defaults_to_dark() {
        assert_eq!(Config::default().theme, "dark");
        assert_eq!(default_theme(), "dark");
    }

    #[test]
    fn older_configs_without_theme_still_load() {
        let toml = r##"
            folder = "/tmp/notas"
            top_n = 40
            min_word_len = 3
            min_font_size = 18
            max_font_size = 72
            speed = 1.0
            order = "alphabetical"
            include_code_blocks = false
            max_file_size = 5242880
            max_ui_fps = 10
            stopwords = []
            colors = ["#ff5555"]
        "##;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.theme, "dark");
        assert_eq!(cfg.base_font_size, default_base_font_size());
        assert_eq!(cfg.font_family, default_font_family());
        assert_eq!(cfg.accent_color, default_accent_color());
        assert_eq!(cfg.line_gap_ms, default_line_gap_ms());
    }
}
