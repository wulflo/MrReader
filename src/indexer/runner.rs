// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::bridge::{IndexEvent, UiSender};
use crate::indexer::watcher::{WatchConfig, start_watcher};
use crate::indexer::{change, scan_markdown, worker};
use crate::store::Store;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub struct IndexConfig {
    pub root: PathBuf,
    pub max_size: u64,
    pub stopwords: Vec<String>,
    pub min_len: usize,
    pub include_code: bool,
    pub top_n: usize,
    pub pool: usize,
}

pub fn run_initial(
    store: Arc<Store>,
    config: IndexConfig,
    tx: UiSender,
    cancel: Arc<AtomicBool>,
) {
    let files = scan_markdown(&config.root, config.max_size);
    let total = files.len();
    let scanned_set: HashSet<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();

    // borrados
    if let Ok(paths) = store.all_paths_sorted() {
        for p in paths {
            if !scanned_set.contains(&p) {
                let _ = store.remove_file(&p);
            }
        }
    }

    // filtrar los que necesitan reindex
    let mut to_process: Vec<PathBuf> = Vec::new();
    for p in &files {
        let ps = p.to_string_lossy().to_string();
        if let Some((m, s)) = change::file_meta(p) {
            if change::needs_reindex(&store, &ps, m, s) {
                to_process.push(p.clone());
            }
        }
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.pool.max(1))
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    let done_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let to_total = to_process.len();

    pool.install(|| {
        to_process.par_iter().for_each(|p| {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let ps = p.to_string_lossy().to_string();
            match worker::process_one(p, &config.stopwords, config.min_len, config.include_code) {
                Ok(ifile) => {
                    let counts = ifile.counts.clone();
                    if let Err(e) = store.index_file(
                        &ps,
                        ifile.mtime,
                        ifile.size,
                        ifile.line_count,
                        &counts,
                    ) {
                        let _ = tx.send_blocking(IndexEvent::Error {
                            path: ps.clone(),
                            message: e.to_string(),
                        });
                        return;
                    }
                    let _ = tx.send_blocking(IndexEvent::FileIndexed { path: ps });
                    // top update cada 50
                    let d = done_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = tx.send_blocking(IndexEvent::IndexProgress {
                        done: d,
                        total: to_total.max(1),
                    });
                    if d.is_multiple_of(50) || d == to_total {
                        if let Ok(top) = store.top_words(config.top_n) {
                            let _ = tx.send_blocking(IndexEvent::TopWordsUpdated { words: top });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send_blocking(IndexEvent::Error {
                        path: ps,
                        message: e.to_string(),
                    });
                }
            }
            let _ = total;
        });
    });

    if !cancel.load(Ordering::Relaxed) {
        if let Ok(top) = store.top_words(config.top_n) {
            let _ = tx.send_blocking(IndexEvent::TopWordsUpdated { words: top });
        }
        let _ = tx.send_blocking(IndexEvent::Finished);
    }
}

/// Ejecuta el indexado inicial y el watcher para una carpeta, y los reinicia
/// cuando llega una carpeta nueva por `roots`. Cada ejecución usa su propio
/// flag de cancelación, de modo que el watcher anterior termina antes de
/// empezar el siguiente (así no se mezclan notas de dos carpetas).
pub fn run_supervisor(
    store: Arc<Store>,
    config: IndexConfig,
    watch: WatchConfig,
    tx: UiSender,
    cancel: Arc<AtomicBool>,
    roots: std::sync::mpsc::Receiver<PathBuf>,
) {
    let mut root = config.root.clone();
    loop {
        let run_cancel = Arc::new(AtomicBool::new(false));
        let store_run = store.clone();
        let tx_run = tx.clone();
        let cancel_run = run_cancel.clone();
        let icfg = IndexConfig {
            root: root.clone(),
            ..config.clone()
        };
        let wcfg = WatchConfig {
            root: root.clone(),
            ..watch.clone()
        };

        let worker = std::thread::spawn(move || {
            run_initial(store_run.clone(), icfg, tx_run.clone(), cancel_run.clone());
            let handle = start_watcher(store_run, wcfg, tx_run, cancel_run.clone());
            while !cancel_run.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let _ = handle;
        });

        // Esperar una carpeta nueva o el cierre de la aplicación.
        let mut next_root = None;
        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            match roots.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(r) => {
                    next_root = Some(r);
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    cancel.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }

        run_cancel.store(true, Ordering::Relaxed);
        let _ = worker.join();

        match next_root {
            Some(r) => root = r,
            None => break,
        }
    }
}
