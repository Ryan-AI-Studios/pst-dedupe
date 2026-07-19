//! Off-UI-thread matter create/open (short DB work must not stall egui).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::Utf8PathBuf;

use crate::matter_ui;

/// Background matter open/create result.
#[derive(Debug)]
pub enum MatterOpResult {
    Created { root: Utf8PathBuf, name: String },
    Opened { root: Utf8PathBuf, name: String },
    Failed { message: String },
}

/// At most one in-flight create/open (shares debounce spirit with dialogs).
#[derive(Default)]
pub struct MatterOpState {
    busy: bool,
    rx: Option<Receiver<MatterOpResult>>,
}

impl MatterOpState {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn spawn_create(&mut self, parent: Utf8PathBuf, name: String) {
        if self.busy {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        let _ = thread::Builder::new()
            .name("desk-matter-create".into())
            .spawn(move || {
                let result = match matter_ui::create_matter(&parent, &name) {
                    Ok(root) => MatterOpResult::Created {
                        root,
                        name: name.trim().to_string(),
                    },
                    Err(message) => MatterOpResult::Failed { message },
                };
                let _ = tx.send(result);
            });
    }

    pub fn spawn_open(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        let root = match Utf8PathBuf::from_path_buf(path) {
            Ok(p) => p,
            Err(_) => {
                // Deliver failure without spawning.
                let (tx, rx) = mpsc::channel();
                self.rx = Some(rx);
                self.busy = true;
                let _ = tx.send(MatterOpResult::Failed {
                    message: "Matter path is not valid UTF-8.".into(),
                });
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        let _ = thread::Builder::new()
            .name("desk-matter-open".into())
            .spawn(move || {
                // Idle open: cleanup orphaned workspace/temp.
                let result = match matter_ui::open_matter(&root, true) {
                    Ok(name) => MatterOpResult::Opened { root, name },
                    Err(message) => MatterOpResult::Failed { message },
                };
                let _ = tx.send(result);
            });
    }

    pub fn try_take(&mut self) -> Option<MatterOpResult> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(r) => {
                self.busy = false;
                self.rx = None;
                Some(r)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                Some(MatterOpResult::Failed {
                    message: "Matter operation thread ended unexpectedly.".into(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_completes_off_thread() {
        let tmp = TempDir::new().unwrap();
        let parent = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let mut op = MatterOpState::default();
        op.spawn_create(parent, "OffThread".into());
        assert!(op.is_busy());
        let mut result = None;
        // CI cold-start (Windows runners) can exceed 2s for first Matter create;
        // allow ~30s before failing so this is not a load flake.
        for _ in 0..3_000 {
            if let Some(r) = op.try_take() {
                result = Some(r);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        match result.expect("op finished") {
            MatterOpResult::Created { name, .. } => assert_eq!(name, "OffThread"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!op.is_busy());
    }
}
