// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::bridge::{IndexEvent, UiSender};
use crate::indexer::worker;
use crate::store::Store;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone)]
pub struct WatchConfig {
    pub root: PathBuf,
    pub max_size: u64,
    pub stopwords: Vec<String>,
    pub min_len: usize,
    pub include_code: bool,
    pub top_n: usize,
}

pub fn start_watcher(
    store: Arc<Store>,
    config: WatchConfig,
    tx: UiSender,
    cancel: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (ntx, nrx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(ev) = res {
                let _ = ntx.send(ev);
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };
        let single_file = if config.root.is_file() {
            Some(
                config
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| config.root.clone()),
            )
        } else {
            None
        };
        // Con un único archivo se vigila su carpeta (sin recursión) y se
        // descartan los eventos de los demás archivos.
        let watch_target = match &single_file {
            Some(file) => file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
            None => config.root.clone(),
        };
        let mode = if single_file.is_some() {
            RecursiveMode::NonRecursive
        } else {
            RecursiveMode::Recursive
        };
        if watcher.watch(&watch_target, mode).is_err() {
            return;
        }

        while !cancel.load(Ordering::Relaxed) {
            // collect events with debounce
            let mut events = Vec::new();
            match nrx.recv_timeout(Duration::from_millis(500)) {
                Ok(ev) => events.push(ev),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(ev) = nrx.try_recv() {
                events.push(ev);
            }

            // gather affected paths
            let relevant = |p: &Path| match &single_file {
                Some(target) => {
                    p == target.as_path() || p == config.root.as_path() || same_file(p, target)
                }
                None => true,
            };
            let mut to_reindex: Vec<PathBuf> = Vec::new();
            let mut to_remove: Vec<String> = Vec::new();
            for ev in events {
                match ev.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        for p in ev.paths {
                            if relevant(&p) && is_md(&p) {
                                to_reindex.push(p);
                            }
                        }
                    }
                    EventKind::Remove(_) => {
                        for p in ev.paths {
                            if relevant(&p) {
                                to_remove.push(p.to_string_lossy().to_string());
                            }
                        }
                    }
                    EventKind::Other => {}
                    _ => {
                        // rename events may appear as remove+create; handle paths
                        for p in ev.paths {
                            if relevant(&p) && is_md(&p) {
                                to_reindex.push(p);
                            }
                        }
                    }
                }
            }

            for p in &to_remove {
                let _ = store.remove_file(p);
                let _ = tx.send_blocking(IndexEvent::FileIndexed {
                    path: p.clone(),
                });
            }

            for p in to_reindex {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let ps = p.to_string_lossy().to_string();
                // if file no longer exists -> remove
                if !p.exists() {
                    let _ = store.remove_file(&ps);
                    continue;
                }
                if let Ok(meta) = std::fs::metadata(&p) {
                    if meta.len() > config.max_size {
                        continue;
                    }
                }
                match worker::process_one(&p, &config.stopwords, config.min_len, config.include_code) {
                    Ok(ifile) => {
                        let _ = store.index_file(
                            &ps,
                            ifile.mtime,
                            ifile.size,
                            ifile.line_count,
                            &ifile.counts,
                        );
                        let _ = tx.send_blocking(IndexEvent::FileIndexed { path: ps });
                        if let Ok(top) = store.top_words(config.top_n) {
                            let _ = tx.send_blocking(IndexEvent::TopWordsUpdated { words: top });
                        }
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(IndexEvent::Error {
                            path: ps,
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
        let _ = watcher; // keep alive
    })
}

fn is_md(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
}

/// Compara dos rutas teniendo en cuenta enlaces simbólicos y rutas relativas.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.file_name().is_some() && a.file_name() == b.file_name(),
    }
}
