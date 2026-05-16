# Track 004 Plan: GUI Errors And Partial Results

## Objective

Make scan failures and partial results visible in the GUI so users can trust what happened.

## Scope

- Represent per-file scan status.
- Surface recoverable PST errors without losing successful results.
- Make fatal worker failures visible in the results or progress view.

## Steps

1. Audit worker messages and GUI state transitions.
2. Add explicit error and warning result types.
3. Show per-file success, skipped, failed, and partial states.
4. Handle cancellation, retry, unreadable files, invalid PSTs, and mixed success/failure scans.
5. Check `eframe` and `rfd` pins for API changes before touching GUI or dialog code.
6. Add tests around worker result aggregation where practical.

## Hardening Notes

- UI state must remain coherent if the worker fails after partial results are emitted.
- Dialog failures and permission errors need visible user-facing messages.
- Long scans must remain cancellable without leaving stale progress state.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Verification Notes

Verified on 2026-05-15:

- **`FileStats` expanded** with `error: Option<String>` and `skipped_messages: u64`.
- **`ScanResult` expanded** with `failed_files: u64`.
- **Worker now records** per-file errors (open failure, folder traversal failure) instead of just logging and continuing.
- **Message read failures** increment `skipped_messages` and continue scanning the rest of the PST.
- **Results view** shows:
  - Yellow warning banner: "X file(s) could not be scanned. Results are partial."
  - Per-file breakdown grid with "Skipped" and "Status" columns
  - Red error text for failed files, yellow for files with skipped messages, green "OK" for clean files
- **Progress view** already showed `progress.error`; unchanged.

## Exit Criteria

- A failed PST does not silently disappear.
- Partial results are labeled as partial.
- Export/report actions account for failed files.
