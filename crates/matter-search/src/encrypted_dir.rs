//! Encrypted Tantivy [`Directory`] for encrypted matters (track 0057).
//!
//! Stores index files as chunked AEAD blobs under `index/` using the matter DEK.
//! **No mmap** — plaintext lives only in process memory (`FileSlice` from `Vec`).
//!
//! Performance: every open decrypts the full file; suitable for e-discovery
//! matters, not high-QPS search services.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use matter_core::crypto::{
    decrypt_bytes_domain, encrypt_bytes_domain, is_encrypted_blob, Dek, DEFAULT_CHUNK_BYTES,
};
use tantivy::directory::error::{DeleteError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, FileHandle, FileSlice, TerminatingWrite, WatchCallback, WatchHandle,
    WritePtr,
};

const FTS_DOMAIN: &[u8] = b"fts";

/// On-disk encrypted Tantivy directory (no mmap).
#[derive(Clone)]
pub struct EncryptedDirectory {
    root: PathBuf,
    dek: Arc<Dek>,
    chunk_bytes: u32,
}

impl fmt::Debug for EncryptedDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedDirectory")
            .field("root", &self.root)
            .field("chunk_bytes", &self.chunk_bytes)
            .finish_non_exhaustive()
    }
}

impl EncryptedDirectory {
    /// Create a directory handle rooted at `index/` under the matter.
    pub fn open(root: impl AsRef<Path>, dek: Arc<Dek>, chunk_bytes: u32) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            dek,
            chunk_bytes: chunk_bytes
                .max(1)
                .max(DEFAULT_CHUNK_BYTES.min(chunk_bytes.max(1))),
        })
    }

    /// Build with explicit chunk size (caller may pass header `cas_chunk_bytes`).
    pub fn with_dek(root: impl AsRef<Path>, dek: Arc<Dek>, chunk_bytes: u32) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            dek,
            chunk_bytes: chunk_bytes.max(1),
        })
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        self.root.join(path)
    }

    fn path_extra(path: &Path) -> Vec<u8> {
        path.to_string_lossy().as_bytes().to_vec()
    }

    fn encrypt_to_disk(&self, path: &Path, plain: &[u8]) -> io::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        let enc = encrypt_bytes_domain(
            self.dek.as_ref(),
            FTS_DOMAIN,
            &Self::path_extra(path),
            plain,
            self.chunk_bytes,
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        // Unique temp name (avoid with_extension collisions on multi-dot paths).
        let tmp = full.with_file_name(format!(
            ".{}.{}.enc.tmp",
            full.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "blob".into()),
            std::process::id()
        ));
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&enc)?;
            f.sync_all()?;
        }
        // Atomic replace: on Windows, rename over existing can fail — remove first.
        if full.exists() {
            fs::remove_file(&full)?;
        }
        fs::rename(&tmp, &full)?;
        Ok(())
    }

    fn decrypt_from_disk(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let full = self.resolve(path);
        if !full.exists() {
            return Err(OpenReadError::FileDoesNotExist(PathBuf::from(path)));
        }
        let data = fs::read(&full).map_err(|io_error| OpenReadError::IoError {
            io_error: Arc::new(io_error),
            filepath: PathBuf::from(path),
        })?;
        if !is_encrypted_blob(&data) {
            return Err(OpenReadError::IoError {
                io_error: Arc::new(io::Error::other(
                    "FTS file is not an encrypted blob (rebuild index required)",
                )),
                filepath: PathBuf::from(path),
            });
        }
        decrypt_bytes_domain(
            self.dek.as_ref(),
            FTS_DOMAIN,
            &Self::path_extra(path),
            &data,
        )
        .map_err(|e| OpenReadError::IoError {
            io_error: Arc::new(io::Error::other(e.to_string())),
            filepath: PathBuf::from(path),
        })
    }
}

struct EncryptedVecWriter {
    dir: EncryptedDirectory,
    path: PathBuf,
    data: Vec<u8>,
    is_flushed: bool,
}

impl Write for EncryptedVecWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.is_flushed = false;
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.dir.encrypt_to_disk(&self.path, &self.data)?;
        self.is_flushed = true;
        Ok(())
    }
}

impl TerminatingWrite for EncryptedVecWriter {
    fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
        self.flush()
    }
}

impl Drop for EncryptedVecWriter {
    fn drop(&mut self) {
        if !self.is_flushed {
            // Best-effort seal; same honesty as RamDirectory.
            let _ = self.flush();
        }
    }
}

impl Directory for EncryptedDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let plain = self.decrypt_from_disk(path)?;
        let slice = FileSlice::from(plain);
        Ok(Arc::new(slice))
    }

    fn open_read(&self, path: &Path) -> Result<FileSlice, OpenReadError> {
        let plain = self.decrypt_from_disk(path)?;
        Ok(FileSlice::from(plain))
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        let full = self.resolve(path);
        if !full.exists() {
            return Err(DeleteError::FileDoesNotExist(PathBuf::from(path)));
        }
        fs::remove_file(&full).map_err(|io_error| DeleteError::IoError {
            io_error: Arc::new(io_error),
            filepath: PathBuf::from(path),
        })
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        Ok(self.resolve(path).exists())
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let full = self.resolve(path);
        if full.exists() {
            return Err(OpenWriteError::FileAlreadyExists(PathBuf::from(path)));
        }
        // Reserve empty ciphertext so exists() is true during write (mmap semantics).
        if let Err(e) = self.encrypt_to_disk(path, &[]) {
            return Err(OpenWriteError::IoError {
                io_error: Arc::new(e),
                filepath: PathBuf::from(path),
            });
        }
        let writer = EncryptedVecWriter {
            dir: self.clone(),
            path: PathBuf::from(path),
            data: Vec::new(),
            is_flushed: true,
        };
        Ok(BufWriter::new(Box::new(writer)))
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        self.decrypt_from_disk(path)
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.encrypt_to_disk(path, data)
    }

    fn sync_directory(&self) -> io::Result<()> {
        // Files are already `sync_all`'d in `encrypt_to_disk`. Opening a directory
        // handle and calling `sync_all` fails with Access Denied on Windows.
        let _ = &self.root;
        Ok(())
    }

    fn watch(&self, _watch_callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        // No filesystem watch on encrypted directory; Manual reload only.
        Ok(WatchHandle::empty())
    }
}
