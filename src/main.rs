// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use mr_reader::config;
use mr_reader::ui;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let (mut cfg, _) = config::parse_args();
    if cfg.folder.is_none() {
        cfg.folder = Some(std::env::current_dir().unwrap_or_default());
    }
    ui::run_app(cfg);
}
