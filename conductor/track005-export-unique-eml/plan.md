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

## Exit Criteria

- Export button writes EML files for unique messages.
- Export failures are visible and do not corrupt existing output.
