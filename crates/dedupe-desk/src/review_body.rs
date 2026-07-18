//! Off-UI-thread CAS body loader for the Review screen.
//!
//! # egui repaint requirement
//!
//! egui is immediate-mode and often **only repaints on input**. After a worker
//! sends a body result on the channel, it **must** call
//! [`egui::Context::request_repaint`] on a cloned context; otherwise the pane
//! can stay stuck on "Loading…" until the operator moves the mouse.
//!
//! Pattern:
//! 1. Clone `egui::Context` before `thread::spawn`.
//! 2. On completion: `tx.send(result)` **then** `ctx.request_repaint()`.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui;
use matter_core::Cas;

use crate::html_strip;

/// Display cap for body text in the UI (bytes of UTF-8 after decode).
pub const BODY_DISPLAY_CAP_BYTES: usize = 2 * 1024 * 1024; // 2 MiB

/// Result delivered from a background body load.
#[derive(Debug)]
pub struct BodyLoadResult {
    pub gen: u64,
    pub item_id: String,
    pub text: Result<String, String>,
    pub truncated: bool,
}

/// Current body pane state (selection-bound).
#[derive(Debug, Clone, Default)]
pub enum BodyPane {
    /// No item selected.
    #[default]
    Idle,
    /// In-flight load for `item_id` at generation `gen`.
    Loading { gen: u64, item_id: String },
    /// Load finished (success or error message in `text`).
    Ready {
        item_id: String,
        text: Result<String, String>,
        truncated: bool,
    },
}

/// Body loader with generation counter for ignoring stale loads.
#[derive(Default)]
pub struct BodyLoader {
    gen: u64,
    pane: BodyPane,
    rx: Option<Receiver<BodyLoadResult>>,
}

impl BodyLoader {
    pub fn pane(&self) -> &BodyPane {
        &self.pane
    }

    /// Clear state (e.g. matter closed / corpus empty).
    pub fn clear(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.pane = BodyPane::Idle;
        self.rx = None;
    }

    /// Start loading body for `item_id` from CAS under `matter_root`.
    ///
    /// Prefers `text_sha256`; falls back to `html_sha256` (block-aware strip).
    /// Cap: [`BODY_DISPLAY_CAP_BYTES`].
    ///
    /// `ctx` is cloned into the worker so it can call **`request_repaint()`**
    /// after the channel send (see module docs).
    pub fn spawn_load(
        &mut self,
        ctx: &egui::Context,
        matter_root: &Utf8Path,
        item_id: String,
        text_sha256: Option<String>,
        html_sha256: Option<String>,
    ) {
        self.gen = self.gen.wrapping_add(1);
        let gen = self.gen;
        self.pane = BodyPane::Loading {
            gen,
            item_id: item_id.clone(),
        };

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let root: Utf8PathBuf = matter_root.to_owned();
        let ctx = ctx.clone();
        let _ = thread::Builder::new()
            .name("desk-review-body".into())
            .spawn(move || {
                let result =
                    load_body_from_cas(&root, text_sha256.as_deref(), html_sha256.as_deref());
                let (text, truncated) = match result {
                    Ok((s, t)) => (Ok(s), t),
                    Err(e) => (Err(e), false),
                };
                let payload = BodyLoadResult {
                    gen,
                    item_id,
                    text,
                    truncated,
                };
                // Order: deliver payload first, then wake the UI.
                let _ = tx.send(payload);
                // REQUIRED: wake egui after async body load (0026 DoD-4).
                ctx.request_repaint();
            });
    }

    /// Poll for a completed load; apply only if generation still matches.
    pub fn try_take(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(r) => {
                self.rx = None;
                if r.gen != self.gen {
                    // Stale — a newer selection already started.
                    return;
                }
                self.pane = BodyPane::Ready {
                    item_id: r.item_id,
                    text: r.text,
                    truncated: r.truncated,
                };
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                if let BodyPane::Loading { gen, item_id } = &self.pane {
                    if *gen == self.gen {
                        let id = item_id.clone();
                        self.pane = BodyPane::Ready {
                            item_id: id,
                            text: Err("Body load thread ended unexpectedly.".into()),
                            truncated: false,
                        };
                    }
                }
            }
        }
    }
}

/// Load and decode body bytes from CAS. Pure enough for tests (no egui).
pub fn load_body_from_cas(
    matter_root: &Utf8Path,
    text_sha256: Option<&str>,
    html_sha256: Option<&str>,
) -> Result<(String, bool), String> {
    let (digest, is_html) = if let Some(t) = text_sha256.filter(|s| !s.is_empty()) {
        (t, false)
    } else if let Some(h) = html_sha256.filter(|s| !s.is_empty()) {
        (h, true)
    } else {
        return Err("No extracted text".into());
    };

    let cas = Cas::new(matter_root);
    let mut file = cas
        .open_read(digest)
        .map_err(|e| format!("CAS open failed: {e}"))?;

    // Read up to cap + 1 to detect truncation without loading multi-GB blobs fully
    // when we only display 2 MiB. We still stop at cap+1.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let mut truncated = false;
    loop {
        let n = file
            .read(&mut chunk)
            .map_err(|e| format!("CAS read failed: {e}"))?;
        if n == 0 {
            break;
        }
        let remaining = BODY_DISPLAY_CAP_BYTES.saturating_sub(buf.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        if n > remaining {
            buf.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let lossy = String::from_utf8_lossy(&buf);
    let text = if is_html {
        html_strip::html_to_review_text(&lossy)
    } else {
        lossy.into_owned()
    };
    Ok((text, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::{item_role, item_status, ItemInput, Matter};
    use tempfile::TempDir;

    fn utf8_temp() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        (tmp, p)
    }

    #[test]
    fn load_plain_text_from_cas() {
        let (_tmp, base) = utf8_temp();
        let root = base.join("matter-body");
        let matter = Matter::create(&root, "Body").expect("create");
        let digest = matter.cas().put_bytes(b"Hello review body").expect("put");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                text_sha256: Some(digest.clone()),
                subject: Some("s".into()),
                ..Default::default()
            })
            .expect("item");
        drop(matter);

        let (text, truncated) =
            load_body_from_cas(&root, Some(digest.as_str()), None).expect("load");
        assert_eq!(text, "Hello review body");
        assert!(!truncated);
        assert!(!item.id.is_empty());
    }

    #[test]
    fn load_html_strips_blocks() {
        let (_tmp, base) = utf8_temp();
        let root = base.join("matter-html");
        let matter = Matter::create(&root, "Html").expect("create");
        let digest = matter
            .cas()
            .put_bytes(b"<p>Hello</p><p>World</p>")
            .expect("put");
        drop(matter);

        let (text, _) = load_body_from_cas(&root, None, Some(digest.as_str())).expect("load");
        assert!(!text.contains("HelloWorld"), "{text:?}");
        assert!(text.contains("Hello") && text.contains("World"));
    }

    #[test]
    fn missing_hashes_honest_empty() {
        let (_tmp, base) = utf8_temp();
        let root = base.join("matter-empty");
        let _matter = Matter::create(&root, "Empty").expect("create");
        let err = load_body_from_cas(&root, None, None).expect_err("no hashes");
        assert!(err.contains("No extracted text"), "{err}");
    }

    #[test]
    fn generation_ignores_stale() {
        let mut loader = BodyLoader::default();
        assert!(matches!(loader.pane(), BodyPane::Idle));
        // Bump gen as if selection changed mid-flight.
        loader.gen = 1;
        loader.pane = BodyPane::Loading {
            gen: 1,
            item_id: "a".into(),
        };
        // Simulate stale result by manually setting higher gen then applying check.
        loader.gen = 2;
        // try_take with no channel is a no-op.
        loader.try_take();
        assert!(matches!(loader.pane(), BodyPane::Loading { gen: 1, .. }));
    }
}
