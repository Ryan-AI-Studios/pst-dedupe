# 0087 — Deterministic Store RecordKey — Completion review

**Track:** 0087-DeterministicStoreRecordKey  
**Status:** Completed (engineering + governance)  
**Branch:** `feat/0087-deterministic-store-record-key`  
**Closes:** D-0079-deterministic-key

## Reviewers / rounds

| Round | Reviewer | Verdict | Notes |
|---|---|---|---|
| Internal #1 | explore/general | PASS WITH DEFERRED P3 | Ephemeral test missing; serde default hygiene; multi-volume E2E optional |
| Fix | orchestrator | — | Ephemeral test + serde default; multi-volume E2E added after Codex |
| Internal #2 | explore | PASS WITH DEFERRED P3 | Only multi-vol E2E left; then strengthened after Codex |
| Codex #1 | gpt-5.6-luna high | **FAIL** | P1 governance pending; P2 cross-process test incomplete (ProviderUID, report digest, multi-volume) |
| Fix | orchestrator | — | Strengthened DoD-2/3 test + multi-volume distinct keys test; board Completed |
| Codex #2–N | gpt-5.6-luna high | FAIL→fix | Governance Ready leftovers; fidelity doc; spec status |
| Codex **final** | gpt-5.6-luna high | **PASS** | No findings; DoD 1–8 met; D-0079 closed |

## Preimage formula (length-prefix, normative)

```
preimage =
  b"pst-dedup/store-record-key/v1\0"
  || algo_version_u32_le          // 1
  || volume_index_u32_le
  || message_count_u64_le
  || content_fingerprint          // 32 bytes

record_key_16 = SHA-256(preimage)[0..16]
if all zero: key[0] = 0x5A
```

`content_fingerprint`:
- If `store_key_material: Some(m)`:
  `SHA-256(b"pst-dedup/store-key-material/v1\0" || m || volume_index_u32_le || message_count_u64_le || volume_local)`
- Else: `volume_local` only

`volume_local` = SHA-256 over write-order messages:
```
for each msg:
  b"msg\0"
  || len_u32_le || utf8(internet_message_id)
  || len_u32_le || utf8(subject)
  || submit_time_filetime_i64_le   // None → 0
  || len_u32_le || utf8(source_folder_path)
```

Job-global seed (unique-pst):
```
SHA-256(b"pst-dedup/job-key-material/v1\0" || for each winner in order:
  b"win\0" || len||source_path || len||folder_path || nid_u64_le)
```

Golden empty-volume key (algo v1, volume_index 0, count 0): `b0a1ca291fa355c1ebb3f9ec13a7b879`.

## Phase 0 non-determinism inventory

| Source | Final PST bytes? | Action |
|---|---|---|
| Old `generate_store_record_key` time+pid+path | **Yes** | **Fixed** → deterministic SHA-256 |
| Temp staging `process_entropy_suffix` | No (rename) | Left alone |
| Message creation/mod times | From submit | Already stable |
| `unique_pst_cmd` quarantine stamp / cancelled path PID | No | Leave |
| `HashMap`/`HashSet` write-order | unique-pst `prepared`/`winners` **Vec**-ordered; maps lookup-only | Confirmed safe |
| B-tree / page allocation | Possibly | DoD-3 structural fallback (path A observed) |

## Fold-in §2.13

All agree/partial dispositions from dual-AI Ready review incorporated: length-prefix, job-global material, B-tree honesty, cross-process tests, 0086 docs synergy only.

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| **1** Deterministic default | **Met** | SHA-256 preimage; no clock/PID/path in default mode |
| **2** Same content same key | **Met** | In-process writer + cross-process CLI: RecordKey **and** ProviderUID match |
| **3** Volume digest / structural | **Met — path A** | Reported `sha256_hex` match across processes on fixture |
| **4** Differentiation | **Met** | Content, volume_index, seed, length-prefix boundary, non-zero, ProviderUID==RecordKey |
| **5** Plumbing | **Met** | unique-pst job material + 0-based volume_index; multi-volume distinct keys test |
| **6** Docs + deferred | **Met** | export + runbook + fidelity doc + CHANGELOG; D-0079 closed |
| **7** Gates | **Met** | fmt, clippy -D warnings, targeted + workspace tests (pre-commit / CI) |
| **8** Recorded | **Met** | this file + board Completed |

## DoD-3 path A evidence

```
unique_pst_cross_process_deterministic_record_key ... ok
0087 DoD-3 path A: cross-process reported volume sha256_hex match (5ae868a6…)
unique_pst_multi_volume_distinct_record_keys ... ok
```

Two separate `Command::new(bin())` spawns → identical RecordKey + ProviderUID + reported volume digests. Multi-volume with `--max-volume-bytes 4096` → distinct per-volume RecordKeys.

## Residuals

- No new **D-0087-*** opened (path A achieved).
- Out of scope remain open: D-0073-eml, D-0084 named-prop write, D-0085 sovereign hosts, D-0086-*.

## Deps

- sha2 **KEEP 0.11.0**; no new crates; crc32fast retained for ephemeral + temp staging only.
