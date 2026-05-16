# Track 005 Plan: Unique EML Export

## Objective

Wire unique-message export from GUI selection through PST re-read and EML file creation.

## Scope

- Use dedup results to identify unique messages.
- Re-open source PSTs as needed.
- Serialize unique messages as EML.
- Report export progress and failures.

## Steps

1. Audit current exporter API.
2. Define stable output naming for exported EML files.
3. Connect GUI export action to worker/exporter flow.
4. Add tests for EML serialization.
5. Add overwrite, path-length, Unicode filename, duplicate filename, and partial export handling.
6. Review dependencies involved in CSV/reporting, file dialogs, and encoding before pin updates.
7. Add manual verification with fixture-backed unique messages.

## Hardening Notes

- Never modify source PST files during export.
- Avoid overwriting existing EML files unless the policy is explicit and tested.
- Preserve enough context in export errors for the user to retry safely.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Verification Notes

Verified on 2026-05-15:

- Added `export_unique_eml()` in `worker.rs` that re-opens source PSTs and exports unique messages as EML files.
- Groups messages by source file for efficient re-opening (one open per file).
- Added `source_files: Vec<PathBuf>` to `ScanResult` to preserve paths for re-export.
- GUI results view now calls `export_unique_eml` when the EML button is clicked.
- Export feedback shown in results view: green "X files written" or yellow "X written, Y failed" with last error.
- Added `export_result` state to `PstDedupApp` for UI feedback.
- Added simple RFC 2822 date formatter in worker.rs to avoid adding chrono dependency to GUI crate.

## Exit Criteria

- Export button writes EML files for unique messages.
- Export failures are visible and do not corrupt existing output.
