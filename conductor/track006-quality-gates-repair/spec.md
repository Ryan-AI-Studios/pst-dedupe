# Track 006 Spec: Quality Gates Repair

## Expected Behavior

- Verification commands are Rust workspace commands.
- Failures point to real code or test issues.
- Hooks and docs agree.

## Edge Cases

- Missing local PST fixtures.
- Warnings emitted while tests pass.
- Dirty worktree with unrelated user edits.
- ChangeGuard semantic indexing unavailable while structural verification works.
- Dependency pin updates requiring `Cargo.lock` refresh.

## Verification

- `changeguard verify`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test --workspace`
