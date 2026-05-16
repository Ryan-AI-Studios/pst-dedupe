# Track 002 Plan: Real PST Fixtures And Traversal

## Objective

Prove the reader can open real Unicode PST files and traverse folders/messages far enough to support deduplication.

## Scope

- Define a fixture strategy that is practical for local development and safe for repository history.
- Add ignored local fixture paths or documented fixture acquisition steps.
- Add integration tests that run when fixtures are present and skip clearly when absent.
- Prove `PstFile::open`, folder traversal, and message property extraction against at least one real PST.

## Steps

1. Decide fixture location, naming, and privacy rules.
2. Add fixture discovery helpers for integration tests.
3. Add a minimal smoke test for `PstFile::open` on a Unicode PST.
4. Add traversal assertions for root folder, folder count, and message count.
5. Add property extraction assertions for subject, sender, date, message ID, and body availability where the fixture supports them.
6. Add negative fixture cases for unsupported, missing, locked, or malformed files where practical.
7. Check whether reader dependency pins need updates for parsing, IO, CRC, or test harness support.
8. Document how to create or place fixtures locally.

## Hardening Notes

- Real PST fixtures must stay outside git unless explicitly synthetic and privacy-reviewed.
- Tests must skip clearly when local fixtures are absent.
- The reader must return typed errors for invalid PSTs, not panic.
- Large-file behavior must avoid reading whole PST files unless required by the format layer under test.
- See [Track Guardrails](../TRACK-GUARDRAILS.md).

## Exit Criteria

- Real fixture tests can be run locally without committing PST data.
- Missing fixtures produce skipped tests or clear ignored-test instructions, not failures.
- At least one real PST proves open plus folder/message traversal.
- Findings from fixture testing are converted into follow-up reader tracks if the parser fails on valid PSTs.
