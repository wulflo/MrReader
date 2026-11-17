// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

pub mod scanner;
pub mod change;
pub mod worker;
pub mod runner;
pub mod watcher;
pub use scanner::scan_markdown;
