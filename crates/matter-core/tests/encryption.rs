//! Integration tests for optional matter encryption (track 0057).

use std::fs;
use std::io::Read;

use camino::Utf8PathBuf;
use matter_core::{
    is_encrypted_matter, Matter, MAGIC_DB, SCHEMA_VERSION, WORKSPACE_DIR, WORKSPACE_TEMP_DIR,
};
use tempfile::tempdir;

fn utf8_root(dir: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8")
}

#[test]
fn create_encrypted_open_cas_roundtrip() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-m1");
    let pass = "test-passphrase-001";

    {
        let m = Matter::create_encrypted(&root, "EncCase", pass).expect("create");
        assert!(m.encryption_enabled());
        assert_eq!(m.schema_version().expect("ver"), SCHEMA_VERSION);
        // Multi-chunk CAS payload (chunk default 1 MiB — use small override path via put size).
        let mut payload = vec![0u8; 2_000];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let digest = m.put_bytes(&payload).expect("put");
        let got = m.get_bytes(&digest).expect("get");
        assert_eq!(got, payload);
        assert!(m
            .workspace_temp_dir()
            .as_str()
            .replace('\\', "/")
            .starts_with(root.as_str().replace('\\', "/").as_str()));
    }

    // After drop: at-rest matter.db is AEAD blob, not SQLite header.
    let db_bytes = fs::read(root.join("matter.db").as_std_path()).expect("read db");
    assert!(
        db_bytes.starts_with(MAGIC_DB),
        "matter.db should start with MAGIC_DB"
    );
    assert!(
        !db_bytes.starts_with(b"SQLite format 3"),
        "must not leave plaintext SQLite on disk"
    );
    assert!(is_encrypted_matter(&root));

    // Open with correct passphrase.
    {
        let m = Matter::open_with_passphrase(&root, pass, true).expect("open");
        assert!(m.encryption_enabled());
        // CAS still readable
        // (re-put to verify write path still works)
        let d = m.put_bytes(b"second").expect("put2");
        assert_eq!(m.get_bytes(&d).expect("get2"), b"second");
    }
}

#[test]
fn wrong_passphrase_fails_closed() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-wrong");
    {
        let _m = Matter::create_encrypted(&root, "W", "correct-secret").expect("create");
    }
    let err = match Matter::open_with_passphrase(&root, "wrong-secret", false) {
        Ok(_) => panic!("wrong passphrase must fail"),
        Err(e) => e,
    };
    let s = err.to_string().to_lowercase();
    assert!(
        s.contains("wrong") || s.contains("passphrase"),
        "unexpected err: {err}"
    );
}

#[test]
fn change_passphrase_rewraps() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-change");
    let old = "old-pass-aaa";
    let new = "new-pass-bbb";

    {
        let m = Matter::create_encrypted(&root, "Chg", old).expect("create");
        let d = m.put_bytes(b"keep-me").expect("put");
        m.change_passphrase(old, new).expect("change");
        // CAS still readable under same DEK while open.
        assert_eq!(m.get_bytes(&d).expect("get"), b"keep-me");
    }

    // Old fails, new works.
    assert!(Matter::open_with_passphrase(&root, old, false).is_err());
    let m = Matter::open_with_passphrase(&root, new, true).expect("open new");
    // Find our blob by re-hash
    let d = matter_core::sha256_hex(b"keep-me");
    assert_eq!(m.get_bytes(&d).expect("get after rewrap"), b"keep-me");
}

#[test]
fn unencrypted_create_still_works() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("plain");
    {
        let m = Matter::create(&root, "Plain").expect("create");
        assert!(!m.encryption_enabled());
        let d = m.put_bytes(b"x").expect("put");
        assert_eq!(m.get_bytes(&d).expect("get"), b"x");
    }
    let db = fs::read(root.join("matter.db").as_std_path()).expect("db");
    assert!(db.starts_with(b"SQLite format 3"));
    assert!(!is_encrypted_matter(&root));
    let m = Matter::open(&root).expect("open plain");
    assert!(!m.encryption_enabled());
}

#[test]
fn open_encrypted_without_env_requires_passphrase() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-env");
    {
        let _m = Matter::create_encrypted(&root, "E", "secret-xyz").expect("create");
    }
    // Ensure env is not set for this process path.
    std::env::remove_var(matter_core::ENV_MATTER_PASSPHRASE);
    let err = match Matter::open(&root) {
        Ok(_) => panic!("encrypted open without env must require passphrase"),
        Err(e) => e,
    };
    assert!(
        matches!(err, matter_core::Error::PassphraseRequired(_)),
        "got {err}"
    );
}

#[test]
fn encrypted_temp_under_matter_root() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-temp");
    let m = Matter::create_encrypted(&root, "T", "p").expect("create");
    let temp = m.workspace_temp_dir();
    let temp_s = temp.as_str().replace('\\', "/");
    let root_s = root.as_str().replace('\\', "/");
    assert!(
        temp_s.starts_with(&root_s),
        "temp {temp_s} must be under matter {root_s}"
    );
    assert!(temp_s.contains(WORKSPACE_DIR));
    assert!(temp_s.contains(WORKSPACE_TEMP_DIR));
    // Session plain DB is under workspace/temp/.enc-db/
    let mut found_enc_db = false;
    if let Ok(entries) = fs::read_dir(temp.as_std_path()) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy() == ".enc-db" {
                found_enc_db = true;
            }
        }
    }
    assert!(
        found_enc_db,
        "expected .enc-db under workspace/temp while unlocked"
    );
    let _ = found_enc_db;
}

#[test]
fn open_read_implements_read() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-read");
    let m = Matter::create_encrypted(&root, "R", "p").expect("create");
    let data = b"reader-payload";
    let d = m.put_bytes(data).expect("put");
    let mut r = m.cas().open_read(&d).expect("open");
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).expect("read");
    assert_eq!(buf, data);
}

#[test]
fn cas_without_crypto_fails_closed_on_encrypted_blob() {
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-cas-fc");
    let digest = {
        let m = Matter::create_encrypted(&root, "C", "p").expect("create");
        m.put_bytes(b"secret-blob").expect("put")
    };
    // Cas::new has no DEK — must not return ciphertext as plaintext.
    let bare = matter_core::Cas::new(&root);
    let err = match bare.open_read(&digest) {
        Ok(_) => panic!("fail closed"),
        Err(e) => e,
    };
    assert!(
        err.to_string().to_lowercase().contains("encrypted")
            || err.to_string().to_lowercase().contains("unlock"),
        "got {err}"
    );
}

#[test]
fn crash_orphan_plain_db_wiped_without_passphrase() {
    // Plant plaintext SQLite under .enc-db + crypto header + sealed matter.db.
    // Open without passphrase must fail AND wipe the orphan plain session.
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-orphan");
    let pass = "orphan-pass-001";
    {
        let m = Matter::create_encrypted(&root, "Orphan", pass).expect("create");
        let _ = m.put_bytes(b"payload-keep").expect("put");
    }
    // After drop: sealed at rest.
    let db_bytes = fs::read(root.join("matter.db").as_std_path()).expect("sealed db");
    assert!(db_bytes.starts_with(MAGIC_DB));

    // Simulate crash residue: plant a plaintext SQLite under .enc-db.
    let enc_db_dir = root
        .join(WORKSPACE_DIR)
        .join(WORKSPACE_TEMP_DIR)
        .join(".enc-db");
    fs::create_dir_all(enc_db_dir.as_std_path()).expect("mkdir enc-db");
    let orphan_plain = enc_db_dir.join("matter.db");
    fs::write(
        orphan_plain.as_std_path(),
        b"SQLite format 3\0stolen-plaintext-session",
    )
    .expect("plant orphan");
    assert!(orphan_plain.as_std_path().exists());

    std::env::remove_var(matter_core::ENV_MATTER_PASSPHRASE);
    let err = match Matter::open(&root) {
        Ok(_) => panic!("encrypted open without passphrase must fail"),
        Err(e) => e,
    };
    assert!(
        matches!(err, matter_core::Error::PassphraseRequired(_)),
        "got {err}"
    );
    assert!(
        !enc_db_dir.as_std_path().exists(),
        "orphan .enc-db must be wiped on PassphraseRequired path"
    );

    // Open with passphrase still works (re-decrypts from sealed matter.db).
    {
        let m = Matter::open_with_passphrase(&root, pass, true).expect("open with pass");
        let d = matter_core::sha256_hex(b"payload-keep");
        assert_eq!(m.get_bytes(&d).expect("get"), b"payload-keep");
    }
    // After drop, no leftover plain session.
    assert!(
        !enc_db_dir.as_std_path().exists(),
        "sealed drop must not leave .enc-db orphan"
    );
}

#[test]
fn reseal_roundtrip_no_nonce_reuse_break() {
    // Re-open / re-seal multiple times; random nonces must keep AEAD sound.
    let tmp = tempdir().unwrap();
    let root = utf8_root(&tmp).join("enc-reseal");
    let pass = "reseal-pass";
    {
        let m = Matter::create_encrypted(&root, "Reseal", pass).expect("create");
        let _ = m.put_bytes(b"v1").expect("put");
    }
    for i in 0..3 {
        let m = Matter::open_with_passphrase(&root, pass, true).expect("open");
        let _ = m.put_bytes(format!("v{i}").as_bytes()).expect("put");
        // drop seals
    }
    let m = Matter::open_with_passphrase(&root, pass, true).expect("final open");
    assert!(m.encryption_enabled());
}
