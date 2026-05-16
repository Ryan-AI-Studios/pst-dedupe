# Track 006 TDD

## Red

- Run `changeguard verify` and capture stale command failures.
- Add failure examples for missing commands, fmt drift, test failure, and dependency lock drift where practical.

## Green

- Update verification config until failures represent real repo issues.

## Refactor

- Remove duplicated or contradictory gate docs.
- Keep slow fixture tests opt-in so the default gate remains reliable.
