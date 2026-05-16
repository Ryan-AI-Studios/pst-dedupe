# Track 005 Spec: Unique EML Export

## Expected Behavior

- Export includes only unique messages unless a later option explicitly requests duplicates.
- EML output is RFC 5322-like enough for common mail clients to inspect.
- File names are deterministic and filesystem-safe.
- Existing files are not overwritten accidentally without an explicit strategy.

## Edge Cases

- Duplicate subjects producing the same filename.
- Very long subjects or paths on Windows.
- Unicode subject, sender, and attachment names.
- Missing body, sender, date, or recipients.
- Disk full, permission denied, partial export, and retry.
- Dependency update changes encoding, path handling, or dialog behavior.

## Verification

- `cargo test -p dedup-engine`
- Manual GUI export with fixture data.
