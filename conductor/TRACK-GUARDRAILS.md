# Track Guardrails

Apply these guardrails to every conductor track before marking it complete.

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
- If `changeguard verify` passes but warnings remain, document whether the warnings are accepted debt or part of the track.
- Do not mark a track complete until the final notes include commands run, fixture assumptions, and any intentionally deferred risk.
