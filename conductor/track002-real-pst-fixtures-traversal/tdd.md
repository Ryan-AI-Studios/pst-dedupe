# Track 002 TDD

## Red

- Add an integration test that expects a real Unicode PST fixture and currently fails or skips because the fixture harness is absent.
- Add assertions for open, folder count, message count, and representative properties.

## Green

- Implement fixture discovery and reader fixes needed to satisfy the smallest real PST smoke case.
- Keep fixture paths local and ignored.

## Refactor

- Extract reusable test helpers for fixture loading and traversal summaries.
- Convert any parser discoveries into focused unit tests where possible.

## Test Cases

- Unicode PST opens successfully.
- ANSI or unsupported PST produces a clear unsupported-format error.
- Root folder can be loaded.
- At least one folder hierarchy can be traversed.
- At least one message can be enumerated and mapped into dedup-relevant fields.
- Negative fixture path returns a structured error.
- Dependency pin update does not change test harness behavior without explicit migration notes.
