# Core Mandates - pst-dedupe

1. **Correctness before completeness**: A partial PST reader with honest errors is better than silently wrong deduplication. Never treat parser failure as an empty PST unless the caller explicitly requested best-effort behavior.
2. **Spec-grounded PST parsing**: Header, NDB, LTP, messaging, encryption, BTree, and subnode behavior must be backed by `ARCHITECTURE.md`, MS-PST references, or fixture tests.
3. **No destructive PST writes**: This project is read-only against PST inputs. Do not mutate PST files.
4. **Explicit error handling**: Use Rust `Result` and typed errors. Avoid `unwrap`, `expect`, and panic paths in production parsing and worker code.
5. **Real fixture coverage**: PST-reader milestones require integration tests or byte-level fixtures. Unit tests for helper functions are not enough.
6. **Dedup semantics must be conservative**: Message-ID matches are definitive. Content hash is a fallback for missing Message-ID unless a track explicitly changes that policy and tests prove the behavior.
7. **Large-file awareness**: Avoid loading whole PST files into memory. Loading individual node data is acceptable only when bounded or justified by the PST layer.
8. **Cargo gate**: Before commit, run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`, unless a blocker is explicitly recorded.
9. **Provenance via ChangeGuard**: Major changes and architectural decisions should be recorded in `changeguard ledger`.
10. **Project memory via ai-brains**: Durable decisions, user corrections, and important constraints should be pinned with `ai-brains`.
