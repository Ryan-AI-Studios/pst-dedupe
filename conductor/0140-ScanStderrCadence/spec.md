# 0140 — Scan stderr cadence (volume + I/O tax)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--json` stays on stdout.** Progress stays on stderr.
> Distinct from **0132** (PowerShell NativeCommandError on unique-pst `stage=`).
> CRC first-N / aggregates stay **0077**. No BCC-default. Do not steal **0100–0104**. Not frontend.

- **Track ID:** 0140-ScanStderrCadence
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst / scan operator stderr. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-25).
- **Status:** Completed
- **Depends on:** **0065** scan progress · **0074** deep-attach writeln · **0077** CRC first-N · **0132** Completed (unique-pst PowerShell capture)
- **Spec authored:** 2026-09-24 placeholder
- **Ready:** 2026-09-25 (`/plan-track 0140` via coordinator advance after **0139**)
- **Fold-in:** 2026-09-25 `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`)
- **Series:** X (INC0102784 CLI operator friction, readonly eval 2026-09-24)
- **Ledger category:** `FEATURE`

> **Closes / absorbs:** `D-0140-scan-stderr-cadence`.
> **HITL (owner, not CI):** optional Desktop INC* `scan -v` (and `--deep-attach-preflight --crc-log-limit 0`) confirming tens of stderr lines, not thousands. Never commit INC* PSTs.

---

## 1. Objective

Operators who run `scan` / `dups` / `keep-set` on a split Purview store (INC*: ~2460 folders, 4099 messages) must get **periodic** stderr progress, not one INFO line per folder and not an unbounded attach-probe chatter stream.

2026-09-24 HITL: `scan -v` produced **~2506** folder `scan progress` lines; deep-attach reported **~2798** attempts; CRC WARN and probe lines still appeared with `--crc-log-limit 0`. Stderr volume is an I/O tax on the metadata path (~3 s wall) as well as an operator-friction problem.

Deliver: cadence for folder `tracing::info` under `-v`; keep `-vv` as folder-grain; honor `--crc-log-limit 0` on deep-attach `attempted=` writeln. Ranking, skip math, JSON envelopes, and unique-pst `stage=` lines stay as shipped.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0065 §3.11** allowed “every N messages **or per folder**” at `-v`. Live `scan.rs` emits `tracing::info!(… "scan progress")` at **every folder** plus every 500 messages. `init_tracing`: `0 => warn`, `1 => info`, `2+ => debug`. Default runs hide folder INFO; **`-v` is the firehose** operators use to “see progress.”

Deep-attach progress is **`writeln!(stderr)`**, not tracing, so it prints at default verbosity whenever `--deep-attach-preflight` is on. Live filter is `attempted == 1 || attempted.is_multiple_of(500)` in `scan.rs` and `unique_pst_cmd.rs`. That throttle is **not** wired to `--crc-log-limit`. `set_log_limit(0, …)` already means CRC totals-only (0077); it does not mute probe writeln.

This is operator stderr honesty + I/O tax, not a progress-protocol rewrite and not poly-CRC copy (**0143**).

### 2.2 Live facts (plan-time HEAD `f424c69`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Schema | Matter **41**. N/A this track. No JSON schema bump. |
| `init_tracing` | `crates/pst-dedup-cli/src/main.rs`: `ArgAction::Count` global `-v`/`-vv`; EnvFilter from `RUST_LOG` else `warn`/`info`/`debug`. Writer = stderr. |
| Folder progress | `scan.rs` folder loop: `tracing::info!(file, folder, recoverable, skipped, "scan progress")` **unconditional per folder**. Message cadence `PROGRESS_EVERY_MSGS = 500` already exists. Comment cites 0065 §3.11. |
| Probe progress | `ProbeEngine::emit_progress` every recorded attempt. Per-attempt CLI sinks: `scan: deep-attach-preflight: attempted={n} bytes={b} source={base}` and unique-pst `unique-pst: deep-attach-preflight: …` (also `on_log`). Unique-pst **also** emits one end-of-probe `emit_log` summary (`deep-attach-preflight: attempted={} failed={} truncated=…`) when `attempted > 0 \|\| truncated \|\| cancelled` (`unique_pst_cmd.rs` ~1973). Scan has no equivalent summary. |
| CRC | `pst_reader::integrity_telemetry::set_log_limit`; `first_n = 0` skips detail WARNs, still emits interval aggregate + `flush_summary` at **warn** (one line per source with mismatches — 0077, not this track). CLI `--crc-log-limit` default **10**, interval **30s**. `read_log_config` / `LOG_CONFIG` mutex is private; tests serialize with `TEST_LOCK` / `with_lock`. |
| `ScanOptions` | No `crc_log_limit` field. CLI calls `apply_crc_log_limits` **before** `run_scan`. Do **not** add a ScanOptions field (many test literals). |
| Hotspots | `scan.rs` rank **#2**; `unique_pst_cmd.rs` rank **#1**. Unique-pst authorized edits: probe `progress_cb` **and** the existing `UniquePstClapArgs::crc_log_limit` `///` (extend 0077 sentence). Not `stage=` / `emit_stage_progress` / end-of-probe summary. |
| CLI tests | No `tests/scan.rs`. Scan integration target is `tests/scan_integrity.rs`. Fixture `fixtures/aspose_outlook.pst` is already used (keep-set / 0077 / 0078); re-count folders at execute (review claimed **27** folders / **17** messages). |
| clap | Workspace `"4"`. Lock **4.6.4**. crates.io / docs.rs latest **4.6.7**. `ArgAction::Count` still increments `u8` from 0. Do **not** add `clap-verbosity-flag`. |
| tracing | Lock **tracing 0.1.44**, **tracing-subscriber 0.3.23** (= crates.io latest). `EnvFilter::try_from_default_env` + `EnvFilter::new(level)` still current. No pin bump. |
| serde_json / MS-PST | **N/A this track.** |
| `--json` | stdout envelope only; logs already on stderr (CLI `long_about`). |
| keep-set / dups | Share `run_scan` → folder cadence lands automatically. |
| unique-eml | Keep-set scan path; no separate probe writeln (unique-pst has its own). |
| 0132 | Closed: PowerShell capture docs for unique-pst `stage=`. Do not reopen `--progress-file`. |

### 2.3 Tools (plan-time 2026-09-25)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). 5015 pins. Last decision: **0139** Completed PR **#154** / `234f4fa`. |
| `ai-brains recall` / `sync query` | Series X minted Proposed; 0140 owns scan stderr I/O tax, distinct from 0132. 0077 CRC first-N stays. |
| `ledgerful doctor --json` | `readyForPublish: true`, 0 block. Unrelated warn: phantom verify rows, completion model cold, impact-stale (refreshed this pass). |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| `ledgerful scan --impact` | `riskLevel: low`. Dirty tree is coordinator/status only. Expected execute hotspots: `scan.rs` then unique-pst probe callback. |

### 2.4 Last-PR Cursor comments

Merged PRs **#155, #154, #153, #152**: inline comments empty; reviews empty; issue comments are Cursor Bugbot “usage limit reached” only. **Decline** — no 0140-owned finding.

### 2.5 Product locks

- Do not move progress onto `--json` stdout.
- Do not add `--progress-file`.
- Do not change CRC skip math, dual-rate poly classifier, `export_risk`, or `CRC_SUSPECT` taint (**0077** / **0099** / **0108** / **0143** / **0144**).
- Do not change unique-pst `stage=` / `emit_stage_progress` wording (**0132**).
- Do not default-on `--deep-attach-preflight`.
- Do not change `first_seen` / `--source-rank` (**0139**).
- Do not install a tracing Layer as the primary mute (**D-0077-tracing-layer** stays residual). Cadence is at the emit site.
- `RUST_LOG` still overrides the default EnvFilter when set.

### 2.6 Cadence contract (locked)

Helpers live in **`crates/pst-dedup-cli/src/scan_progress.rs`** (CLI concern). Do **not** put cadence in `pst-reader`; `integrity_telemetry` stays data-path counters + a thin `log_first_n()` getter.

**Folder `scan progress` (tracing):** keep the event message `"scan progress"`. Folder-only events have `folder=` and **omit** `msg_i`. Message-every-500 INFO keeps `msg_i=`. CLI tests count folder lines as `folder=` without `msg_i=`.

| Verbosity | Folder lines |
|---|---|
| default (`warn`) | None (stay at `info`/`debug`) |
| `-v` (`info`) | Periodic only, **per file**: first folder, last folder, every **250** folders, or every **2 s** since last **emit**. Message-every-500 INFO stays. |
| `-vv` (`debug`) | Exactly **one** line per folder: cadence folders `tracing::info!`, others `tracing::debug!` (`if cadence { info } else { debug }`). No INFO+DEBUG pair on the same folder. |

Per-file locals: `folder_i` (1-based), `folder_count`, `last_folder_emit: Instant`. Initialize `last_folder_emit` at the **start of each file’s** folder loop. **Re-arm** `last_folder_emit = Instant::now()` immediately whenever the helper returns true. Do not share the timer across input PSTs.

**Deep-attach per-attempt writeln** (`progress_cb` only):

| `--crc-log-limit` | Per-attempt `progress_cb` lines |
|---|---|
| `0` | **Zero** per-attempt lines (stderr **and** unique-pst `on_log`) |
| default `10` or any `> 0` | Keep live filter: `attempted == 1 \|\| multiple_of(500)` |

Unique-pst **retains** the single end-of-probe `emit_log` summary (`attempted={} failed={} truncated=…`). That line is not `progress_cb`. Scan has none. JSON `attach_probe` totals unchanged.

Capture `first_n: u64` **once** before building `progress_cb` (no `LOG_CONFIG` lock per attempt). `scan.rs`: `let first_n = log_first_n();`. unique-pst: `let first_n = args.crc_log_limit;` after `apply_crc_log_limits`. Closure calls `should_emit_probe_progress_line(attempted, first_n)` only.

CRC detail/aggregate/`flush_summary` is **unchanged** 0077. Do not claim “zero stderr” on CRC-mismatch sources.

Global `-v` help: **scan-family** folder progress (`scan` / `dups` / `keep-set` via `run_scan`). Unique-pst `stage=` / probe summary stay in `docs/unique-pst-export.md`.

---

## 3. In scope

1. `scan_progress.rs` helpers + folder loop: cadence INFO else DEBUG; per-file timer re-arm.
2. Probe `progress_cb` gated by captured `first_n` in `scan.rs` and unique-pst (stderr + `on_log`).
3. `integrity_telemetry::log_first_n()` getter — no ScanOptions blast. Getter tests under `TEST_LOCK` / `with_lock`.
4. Clap: extend `--crc-log-limit` `///` on scan / dups / keep-set / unique-eml (`main.rs`) **and** `UniquePstClapArgs::crc_log_limit` in `unique_pst_cmd.rs`. Global `-v` help is scan-family only.
5. Short docs sentence (CHANGELOG + unique-pst-export for probe summary vs per-attempt). `--json` contract unchanged.
6. Tests in §8 (`scan_integrity` + aspose CLI + helper 40-tick).

## 4. Out of scope (do NOT do here)

- unique-pst `stage=` progress rewrite or `--progress-file` (**0132**).
- Silencing unique-pst’s **one** end-of-probe attach summary (`emit_log` after probe). Per-attempt `progress_cb` only.
- Claiming “zero stderr” when 0077 `flush_summary` still WARNs on CRC-mismatch sources.
- Poly CRC vs preflight `ok` copy (**0143**).
- Integrity CSV empty-file honesty (**0144**).
- Deep-attach leftover coverage JSON (**0147**).
- Scan-once reuse (**0145**).
- Keep-set JSON envelope size (**0141**).
- `dups --limit` totals (**0142**).
- Unique-pst write wall (**0146**).
- In-tool ScanPST / CRC repair.
- Frontend / Desk tracing subscriber (**D-0077-desk-subscriber**).
- BCC-default. Stealing **0100–0104**.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0077 CRC limiter and 0074 probe callback exist (shipped).
- **P2:** 0132 Completed — do not reopen PowerShell capture.
- *Verified to date:* HEAD `f424c69`; per-folder INFO; probe writeln every 500; `crc-log-limit 0` is CRC-only.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Cadence hides hung scans | First + last folder of each file always emit at `-v`; 2 s floor |
| Muting probes looks like “probe off” | JSON `attach_probe.attempted` / `truncated` stay; clap help says limit 0 silences **lines** |
| unique_pst_cmd.rs hotspot edits regress export | Only `progress_cb` + `crc_log_limit` `///` |
| Mutex on probe hot path | Capture `first_n` once outside the closure |
| 2 s floor fires every remaining folder | Re-arm `last_folder_emit` on each true emit; reset per file |
| `-vv` duplicates cadence folders | `if cadence { info } else { debug }` |
| ScanOptions field explosion | Getter on existing LOG_CONFIG |
| `-v` tests depend on INC* folder counts | Helper 40-tick + CLI on `fixtures/aspose_outlook.pst`; count `folder=` without `msg_i=` |
| Progress on stdout | Forbidden; `--json` fixture still parses |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Default + `-v` volume:** Default (no `-v`) has **zero** folder-progress lines (`folder=` without `msg_i=`). **N ≥ 40 proof** is the `scan_progress` helper unit test (INFO emit count `< N`, includes first and last). **CLI proof** on `fixtures/aspose_outlook.pst`: `-v --json` folder-progress line count is **strictly less** than that file’s folder count (re-count at execute; with 27 folders and sub-2 s wall expect first+last only). INC*-class HITL target: **tens** of folder-progress lines, not thousands. Do not naively count every `"scan progress"` (message-every-500 uses the same message + `msg_i=`).
- [ ] **DoD-2 — `-vv` documented:** Global `-v` help states scan-family `-v` = periodic folder progress, `-vv` = one line per folder. Unique-pst probe/stage wording lives in unique-pst-export, not the global flag. `-vv` does not double-emit cadence folders.
- [ ] **DoD-3 — Probe cadence:** `--deep-attach-preflight` still logs every 500 attempts (plus attempt 1) when `--crc-log-limit` > 0 (`progress_cb` pattern `attempted=… bytes=… source=`).
- [ ] **DoD-4 — `--crc-log-limit 0`:** Zero **per-attempt** `progress_cb` lines on scan stderr and unique-pst stderr/`on_log`. Unique-pst **may** still print **one** end-of-probe summary. CRC first-N still totals-only (0077 `flush_summary` may still WARN). JSON `attach_probe` counts still populated when the probe ran. Clap `--crc-log-limit` help on all five surfaces includes the probe-line clause.
- [ ] **DoD-5 — Isolation:** `--json` stdout remains one parseable envelope (existing scan/keep-set fixture). No `--progress-file`. unique-pst `stage=` strings unchanged.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG user-facing CLI; ledger **FEATURE**. Optional owner HITL on Desktop INC* recorded or skipped.

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-reader --lib integrity_telemetry
cargo test -p pst-dedup-cli --lib
cargo test -p pst-dedup-cli --test scan_integrity
# plus scan_progress unit tests and aspose CLI cadence tests named in plan.md
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Owner HITL (optional): Desktop `INC0102784.pst` + `-2.pst`, `scan -v --json` and `scan --deep-attach-preflight --crc-log-limit 0 --json`. Count stderr. Never `git add` those PSTs.

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0140-scan-stderr-cadence** | **Absorb — this track.** |
| **D-0132-cli-progress-powershell** | **Closed / 0132** — do not reopen; this track is scan/dups/keep-set cadence + probe writeln. |
| `--progress-file` | **Decline** (0132). |
| **D-0077-tracing-layer** | **Decline** — emit-site cadence, not a subscriber Layer. |
| **D-0077-desk-subscriber** | **Decline** — CLI stderr. |
| CRC first-N math / poly classifier | **Decline** — 0077 / 0099 / **0143**. |
| Integrity CSV header-only | **Decline** — **0144**. |
| Probe leftover coverage JSON | **Decline** — **0147**. |
| **D-0074-gui** | **Decline** — wizard checkbox stays residual. Gating unique-pst `progress_cb` also silences GUI `on_log` probe ticks at limit 0; intended; summary `emit_log` still reaches `on_log`. |
| Bugbot #152–#155 | **Decline** — usage-limit only. |
| opencode-O1 sticky `set_log_limit` | **Already covered** — getter tests restore under `TEST_LOCK`. |
| opencode-O2 `flush_summary` | **Already covered** — 0077; DoD does not claim zero stderr. |

## 10. Unblocks

`-v` is usable on INC*-class folder trees. Probe silence is the same knob as CRC totals-only. Parallel with **0141–0144** copy/JSON tracks. Does not unblock unique-pst wall time (**0146**).
