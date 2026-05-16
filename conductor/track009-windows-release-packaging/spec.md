# Track 009 Spec: Windows Release Packaging

## Expected Behavior

- The release artifact is a Windows executable suitable for local deployment.
- Runtime dependencies are documented.
- Packaging does not depend on Outlook, libpff, or C toolchains.

## Edge Cases

- Fresh Windows machine without Rust installed.
- Long install path or output path.
- Missing permissions to selected PST or export directory.
- GUI framework update changes native options, icon handling, or subsystem behavior.

## Verification

- `cargo build --release -p pst-dedup-gui`
- Manual launch smoke test.
