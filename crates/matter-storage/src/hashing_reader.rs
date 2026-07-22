//! Reader wrapper that computes SHA-256 while streaming.

use std::io::{self, Read};

use sha2::{Digest, Sha256};

use crate::digest::hex_encode;

/// Default read buffer when copying through a hashing reader (64 KiB).
pub const HASHING_READ_BUF: usize = 64 * 1024;

/// Wraps a [`Read`], updating a SHA-256 hasher on every successful read.
///
/// Call [`HashingReader::finalize`] after EOF to obtain the lowercase hex digest.
/// Used by cloud put paths so CAS identity is verified independently of
/// multipart ETags (which are **not** content SHA-256).
pub struct HashingReader<R: Read> {
    inner: R,
    hasher: Sha256,
    bytes_read: u64,
}

impl<R: Read> HashingReader<R> {
    /// Wrap `inner` and start hashing.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_read: 0,
        }
    }

    /// Total bytes successfully read so far.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    /// Consume the reader and return the lowercase hex SHA-256 of all bytes read.
    pub fn finalize(self) -> String {
        hex_encode(self.hasher.finalize().as_ref())
    }

    /// Copy all remaining bytes from `self` into `writer`, hashing as we go.
    ///
    /// Returns `(digest, total_bytes)`.
    pub fn copy_to<W: io::Write>(mut self, writer: &mut W) -> io::Result<(String, u64)> {
        let mut buf = vec![0u8; HASHING_READ_BUF];
        loop {
            let n = self.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
        }
        let total = self.bytes_read;
        let digest = self.finalize();
        Ok((digest, total))
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.hasher.update(&buf[..n]);
            self.bytes_read += n as u64;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use std::io::Cursor;

    #[test]
    fn finalize_matches_sha256_hex() {
        let data = b"hello hashing reader";
        let mut hr = HashingReader::new(Cursor::new(data.as_slice()));
        let mut out = Vec::new();
        hr.read_to_end(&mut out).expect("read");
        assert_eq!(out, data);
        assert_eq!(hr.finalize(), sha256_hex(data));
    }
}
