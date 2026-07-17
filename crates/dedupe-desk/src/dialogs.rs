//! Off-thread native file/folder dialogs + `dialog_open` debounce.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

/// Kind of picker the UI requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    CreateParentFolder,
    OpenMatterFolder,
    AddSourceFolder,
    AddZipFile,
    AddPstFile,
}

/// Result delivered from a background dialog thread.
#[derive(Debug, Clone)]
pub struct DialogResult {
    pub kind: DialogKind,
    /// `None` means cancel / no selection.
    pub path: Option<PathBuf>,
}

/// Active off-thread dialog (at most one).
#[derive(Default)]
pub struct DialogState {
    open: bool,
    rx: Option<Receiver<DialogResult>>,
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Spawn a folder/file picker on a background thread. No-op if already open.
    ///
    /// `initial_dir` seeds the dialog starting directory when present (e.g. last
    /// parent used for Create matter).
    pub fn spawn(&mut self, kind: DialogKind, initial_dir: Option<PathBuf>) {
        if self.open {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.open = true;
        if thread::Builder::new()
            .name("desk-rfd".into())
            .spawn(move || {
                let mut dlg = rfd::FileDialog::new();
                if let Some(dir) = initial_dir {
                    dlg = dlg.set_directory(dir);
                }
                let path = match kind {
                    DialogKind::CreateParentFolder => dlg
                        .set_title("Choose parent folder for new matter")
                        .pick_folder(),
                    DialogKind::OpenMatterFolder => {
                        dlg.set_title("Open matter folder").pick_folder()
                    }
                    DialogKind::AddSourceFolder => dlg
                        .set_title("Add source folder (Purview export / dump)")
                        .pick_folder(),
                    DialogKind::AddZipFile => dlg
                        .set_title("Add ZIP package")
                        .add_filter("ZIP", &["zip"])
                        .pick_file(),
                    DialogKind::AddPstFile => dlg
                        .set_title("Add PST file")
                        .add_filter("PST", &["pst"])
                        .pick_file(),
                };
                let _ = tx.send(DialogResult { kind, path });
            })
            .is_err()
        {
            // Spawn failed: release gate so the UI is not stuck open forever.
            self.open = false;
            self.rx = None;
        }
    }

    /// Poll for a completed dialog (non-blocking). Clears `open` when ready.
    pub fn try_take(&mut self) -> Option<DialogResult> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.open = false;
                self.rx = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.open = false;
                self.rx = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_while_open_is_no_op() {
        let mut d = DialogState {
            open: true,
            ..Default::default()
        };
        d.spawn(DialogKind::OpenMatterFolder, None);
        // Still "open" with no new receiver if we forced open without rx.
        assert!(d.is_open());
    }
}
