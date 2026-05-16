# Track 001 TDD

## Red

- Run baseline checks and record failing commands.
- Treat any failing gate as the test case for this track.

## Green

- Make the smallest formatting, config, or code cleanup needed for the gates to pass.
- Prefer deleting unused code over suppressing warnings when code is not part of the public design.

## Refactor

- Consolidate duplicated gate commands into documented project workflow.
- Keep conductor notes current with the exact commands that passed.

## Test Cases

- Workspace formatting check.
- Workspace compile check.
- Full workspace test suite.
- ChangeGuard verification command.
- Pin update check: dependency change includes release-note review and migration coverage.
- Regression check: no new warnings introduced without conductor notes.
