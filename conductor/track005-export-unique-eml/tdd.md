# Track 005 TDD

## Red

- Add tests for EML serialization and safe output naming.
- Add tests for Unicode, long names, duplicate names, and missing fields.

## Green

- Implement export path and GUI wiring.

## Refactor

- Separate serialization from filesystem writes so it remains testable.
- Add golden EML snippets before changing serialization dependencies or formatting behavior.
