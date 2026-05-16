# Track 008 TDD

## Red

- Add targeted tests for bad CRC, malformed trailers, and missing subnodes.
- Add malformed heap/table/block cases before hardening parser behavior.

## Green

- Implement validation and error handling.

## Refactor

- Centralize low-level format validation helpers when duplication appears.
- Add compatibility tests before changing low-level dependency pins.
