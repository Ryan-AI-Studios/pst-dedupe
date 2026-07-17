# Track Guardrails

Apply these guardrails to every conductor track before marking it complete.

## Conductor template (required for new tracks)

- New tracks use the coordinated-style layout: `####-PascalDescription/` with `spec.md` + `plan.md`.
- Copy from `templates/0000-Description/`; do not hand-roll structure.
- Register every track in `conductor.md`; sequence in `sequencing.md`.
- Plan-of-record for Desk product work: `C:\dev\Dedupe-plan.md`.
- On completion: write `review.md`, set registry status to **Completed**, commit a ledger transaction.

## Desktop / product invariants (Dedupe Desk)

- Single-exe launch path for Desk edition — no user-managed servers or daemons.
- AI / OCR / transcription plugins are **opt-in** and off by default.
- Never mutate source Purview export or source PST files.
- Prefer honest partial results + item-level errors over silent drops.
- CPU-heavy parse/hash/OCR runs on a **blocking pool**, not the UI/async executor (`Dedupe-plan.md` §4.6).
- Store both **native** and **logical** hashes; dedupe decisions use logical/Message-ID identity (`Dedupe-plan.md` §2.3).
- Ingest/process jobs must be **checkpointed/resumable** for multi-GB packages.
- Full-text search uses **Tantivy**; SQLite is for structured metadata (not primary FTS).

## Supply chain / hostile inputs

- Keep `cargo audit` (or `cargo deny` advisories) in the verification gate for production deps.
- When adding a parser for untrusted formats (ZIP, PDF, Office, MSG, PST slices), add fuzz or strong property tests for that surface before calling the track complete.

## Compatibility And Pin Updates

- Check `Cargo.toml` and `Cargo.lock` before and after the track.
- If a dependency pin changes, review upstream release notes for breaking APIs, MSRV changes, feature defaults, license changes, and platform support changes.
- Prefer narrow dependency updates that serve the track. Do not bundle unrelated major upgrades with feature work.
- When a pin update changes syntax or public APIs, add a compatibility note to the track plan and include the migration in TDD coverage.
- Re-run workspace checks after dependency updates even if the code change is small.

## Resilience

- Handle invalid inputs with structured errors, not panics.
- Treat missing, corrupt, locked, unsupported, or partially readable PST files as expected user scenarios.
- Keep partial results available when they are trustworthy, and label them clearly.
- Preserve user data: no destructive writes to source PST files, no overwrite of export output without an explicit policy.

## Edge Cases

- Empty inputs and empty PSTs.
- Duplicate paths, missing paths, unreadable paths, and long Windows paths.
- Unicode file names and message content.
- Very large PST files and message counts.
- Corrupt PST pages, blocks, heaps, tables, and properties.
- Missing Message-ID, malformed Message-ID, missing sender/date/body, and attachment metadata gaps.
- Cancellation and retry during long-running scans or exports.

## Verification

- Run the smallest useful test first, then the workspace gate before completion.
- If `ledgerful verify` passes but warnings remain, document whether the warnings are accepted debt or part of the track.
- Do not mark a track complete until the final notes include commands run, fixture assumptions, and any intentionally deferred risk.
