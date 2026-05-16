# Track 009 Plan: Windows Release Packaging

## Objective

Prepare the application for Windows executable distribution after core functionality is proven.

## Scope

- Release profile validation.
- Windows icon and metadata.
- Packaging instructions.
- Smoke test for the built executable.

## Steps

1. Confirm release build works.
2. Add Windows metadata and icon if desired.
3. Document build artifacts and deployment steps.
4. Review GUI and packaging dependency pins for release-impacting changes.
5. Smoke test the release executable on Windows.
6. Record binary size, runtime assumptions, and known limitations.

## Hardening Notes

- Release packaging must not require Outlook, libpff, or external native DLLs.
- Verify release behavior after GUI framework or dialog pin updates.
- Keep debug-only assumptions out of release code paths.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Verification Notes

Verified 2026-05-15:

- **`cargo build --release -p pst-dedup-gui`** succeeds and produces `target\release\pst-dedup-gui.exe` (~13 MB).
- **Self-contained**: No external DLLs required; only standard Windows system libraries.
- **Smoke test**: Process starts and runs for multiple seconds without immediate crash or missing-dependency error.
- **README updated**: Added release executable path, size estimate, and run command.

## Exit Criteria

- `cargo build --release -p pst-dedup-gui` produces the intended executable.
- Release instructions are documented.
