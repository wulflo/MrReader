// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use async_channel::{Receiver, Sender, bounded};

#[derive(Debug, Clone)]
pub enum IndexEvent {
    FileIndexed {
        path: String,
    },
    TopWordsUpdated {
        words: Vec<(String, String, i64)>,
    },
    IndexProgress {
        done: usize,
        total: usize,
    },
    Error {
        path: String,
        message: String,
    },
    Finished,
}

pub type UiSender = Sender<IndexEvent>;
pub type UiReceiver = Receiver<IndexEvent>;

pub fn ui_channel(capacity: usize) -> (UiSender, UiReceiver) {
    bounded(capacity.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_channel_backpressure() {
        let (tx, rx) = ui_channel(1);
        tx.send_blocking(IndexEvent::Finished).unwrap();
        // second send blocks; test by try_send failing
        assert!(tx.try_send(IndexEvent::Finished).is_err());
        let _ = rx.recv_blocking().unwrap();
        assert!(tx.try_send(IndexEvent::Finished).is_ok());
    }
}
