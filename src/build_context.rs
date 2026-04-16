use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::errors;

/// Owns the per-build runtime state that was previously stored in process
/// globals: the tokio runtime, the interrupt flag, and the interrupt broadcast
/// channel. Creating a fresh `BuildContext` gives an isolated build
/// environment — the prerequisite for daemon mode, LSP integration, and
/// parallel test harnesses.
#[allow(dead_code)]
pub(crate) struct BuildContext {
    runtime: Runtime,
    interrupted: AtomicBool,
    interrupt_tx: watch::Sender<bool>,
    interrupt_rx: watch::Receiver<bool>,
}

#[allow(dead_code)]
impl BuildContext {
    pub(crate) fn new() -> Self {
        let runtime = Runtime::new().expect(errors::TOKIO_RUNTIME);
        let (interrupt_tx, interrupt_rx) = watch::channel(false);
        Self {
            runtime,
            interrupted: AtomicBool::new(false),
            interrupt_tx,
            interrupt_rx,
        }
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub(crate) fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub(crate) fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        let _ = self.interrupt_tx.send(true);
    }

    pub(crate) fn interrupt_receiver(&self) -> watch::Receiver<bool> {
        self.interrupt_rx.clone()
    }
}
