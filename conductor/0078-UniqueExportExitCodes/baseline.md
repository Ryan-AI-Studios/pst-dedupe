# 0078 Baseline — Exit codes at HEAD (`3d693e5`)

Captured **before** behavior change so rule 4 (refinement-only) can be proven after.
Evidence source: **code + unit tests** at HEAD (process runs not required for frozen 0–5).

## Shipped `CliExit` values (`crates/pst-dedup-cli/src/error.rs`)

| Class | Code | How produced | Evidence |
|---|---:|---|---|
| Success | **0** | `main`: `Ok(())` → `CliExit::Success` | `main.rs` match on `run` |
| Generic | **1** | Most hard errors; `AlreadyEmitted { exit: Generic }`; `Msg`; PST/IO/CSV | `CliError::exit_code` |
| Usage | **2** | Bad args, missing path, relative params, `CliError::Usage` | `error.rs` mapping + tests |
| Busy | **3** | Matter busy / runner `Busy` | `error.rs` + `RunnerError::Busy` |
| JobFailed | **4** | Job finished failed or cancelled | `CliError::JobFailed` |
| MatterIo | **5** | Matter open/create/IO/schema/crypto | `From<matter_core::Error>` + tests (`error.rs:198-222`) |

**Frozen:** values, mappings, and `error.rs` tests for 0–5 must remain unchanged by 0078.

## unique-pst outcomes at HEAD

| Scenario | Process exit | Summary / notes | Evidence |
|---|---:|---|---|
| Clean fixture (`--no-attachments` or zero attach fails) | **0** | `ok: true`, `compute_export_ok` all dimensions true | `tests/unique_pst.rs` `unique_pst_fixture_schema_and_counts` asserts `status.success()` |
| Attach soft-fail (`attachments_failed > 0`) | **1** | `ok: false`; artifact retained; report flushed | `unique_pst_attachment_failures_force_export_fail`; site `AlreadyEmitted { Generic }` / `run_unique_pst` → `Msg` → Generic |
| Cancel (cooperative) | **1** | `cancelled: true` in summary; incomplete PST left at `--out` if any bytes written | `cancelled` modelled fully; exit still Generic via `!ok` path (`run_unique_pst` / AlreadyEmitted) |
| Hard fail (verify/count/report/export_partial) | **1** | `ok: false` | `compute_export_ok` dimensions → false |
| Usage (existing `--out` without `--overwrite`) | **2** | No export | `unique_pst_cmd` guard |

### Notes for post-change refinement assertion

- After 0078, attach soft-fail becomes **64** (still non-zero).
- Cancel becomes **130** (still non-zero).
- Hard fail stays **1**.
- Clean complete stays **0**.
- Nothing that exits non-zero today may become 0 without an explicit flag (`--allow-partial-fidelity`).

## `compute_export_ok` dimensions (HEAD)

```
scan_ok && verify_ok && export_err_absent && !export_partial
  && messages_written_total == unique
  && attach_failed_total == 0
  && report_ok
```

Any false → process exit **1** today (via `ok=false`).

## `export_risk` (0077) at HEAD

Computed and written to JSON / stdout; **does not affect exit code**.
