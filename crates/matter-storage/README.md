# matter-storage

Opt-in **BlobStore** backends for matter CAS (track **0061**).

## Offline default

Default features are **local only**. Desk and default workspace builds do not pull
`object_store` or cloud SDKs.

## Features

| Feature | Effect |
|---|---|
| *(none)* | `LocalFsBlobStore`, `InMemoryBlobStore`, `CachedBlobStore`, config types |
| `cloud-s3` | `S3BlobStore` via `object_store` 0.14.x (`aws`) |
| `cloud-azure` | Residual flag for Azure (not opened in P0 factory) |

## Layout

Local parity with matter-core CAS:

```text
blobs/sha256/<aa>/<64-hex>
```

Cloud object keys:

```text
{prefix?}/{tenant_id?}/{matter_id?}/cas/sha256/<aa>/<64-hex>
```

## Integrity

- **`put_stream`** (plaintext / content-addressed): stream through **HashingReader** (SHA-256).
  Digest mismatch → **abort multipart** + best-effort **delete** + fail closed.
- **`put_at_digest`** (encryption path): store stream bytes under a **precomputed** digest
  key without requiring SHA-256(stream) == digest (ciphertext under plaintext identity).
- Never trust S3 multipart ETags as content hashes.
- **All** multipart failure paths (read, capacity, finish, mismatch) abort + best-effort delete.
- **Get**: stream object_store payload chunks to a wipe-on-drop temp file (no full-object RAM).

## Multipart RAM caps

- Part size default **10 MiB** (hard max **16 MiB**)
- Max **2** concurrent part uploads
- Peak buffers ≈ part_size × concurrent ≲ **~20 MiB**

## Cache

`CachedBlobStore` stores cloud gets under matter-local **`.cache/blobs/`** (LRU by max bytes).

## Secrets

`StorageBackendConfig` holds **no credentials**. Use `AWS_*` env, IAM roles, or keyring.
Never write secrets into `matters.storage_backend_json`. Endpoints with URL userinfo
(`user:pass@`) or values containing `AKIA` / `password=` / `secret=` are **rejected**.
Audit redaction never emits raw userinfo.

## SQLite

`matter.db` stays on the **service host local disk**. Only CAS blob bytes may live in object storage.

## Activation

Config may be **stored** without the `cloud-s3` feature. **Opening** a matter with
`kind=s3` requires a binary built with `cloud-s3` and succeeds only if `open_blob_store`
works — **fail closed**, no silent local CAS.

## Remote workers

Remote job workers (residual) must talk **HTTP to matter-service only** — never open remote SQLite/NFS.
Local `JobBackend::complete`/`fail` return errors; host-local terminal updates use
`set_job_terminal_local` on the service host.

## Tests

```powershell
cargo test -p matter-storage
cargo test -p matter-storage --features cloud-s3
```
