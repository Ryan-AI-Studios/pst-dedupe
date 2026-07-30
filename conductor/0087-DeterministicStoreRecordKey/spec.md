# 0087 — Deterministic Store Record Key

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.

- **Track ID:** 0087-DeterministicStoreRecordKey
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after 0082–0086
- **Cross-repo contract:** n/a
- **Status:** Completed — Codex luna final PASS (DoD-3 path A; D-0079-deterministic-key closed)
- **Depends on:** 0068–0071 writer + unique-pst path **Completed**; 0079 **Completed** (structural equivalence oracle; residual **D-0079-deterministic-key**); 0080 volume digests **Completed**
- **Spec authored:** 2026-07-29
- **Series:** M (Unique export fidelity residuals)
>
> **Research (2026-07-29):** live crates.io pins + NIST/EDRM/ISO chain-of-custody framing + MS-PST
> `PidTagRecordKey` self-consistency already shipped. Deferred roll-in dispositions in §2.8.
>
> **Review fold-in (2026-07-29):** dual-AI review of Ready draft incorporated below.
> Disposition of each claim is in §2.13 (agree / partial / decline with reason).

---

## 1. Objective

Make unique-PST **`PidTagRecordKey` / EntryID ProviderUID** generation **deterministic from stable export inputs** so two runs over the same logical winners produce **byte-identical store identity fields**. When the writer’s layout of final PST bytes is also stable under those inputs, reported volume digests (`sha256_hex` / `md5_hex`) become re-run-stable chain-of-custody values.

**Outcome:** Store identity is no longer salted with wall-clock/PID entropy. **Logical store identity is guaranteed** under default deterministic mode. **Full volume-file byte-identity** is a best-effort claim subject to B-tree/layout stability (§2.7, DoD-3 fallback) — never greenwashed. Closes **D-0079-deterministic-key**.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0079-deterministic-key** | product | `generate_store_record_key` mixes **wall-clock nanos + process id + path + message count** via four salted CRC32s → every re-run differs in `PidTagRecordKey` **and** every folder EntryID ProviderUID → volume digests differ even when winners are identical |
| 0079 design note | — | Perf track correctly refused this as a product call and built a **structural equivalence oracle** instead of “byte-identical output” as the safety net |
| 0080 QC | — | Source-differential QC + optional scanpst prove *structure*; they do **not** make `sha256_hex` a content-stable custody seal across re-runs |
| Board after 0086 | — | Series M residuals list **D-0079-deterministic-key first**, then D-0073-eml, D-0084 named-prop write, D-0085 sovereign hosts, D-0086-* |

After 0082–0086 fidelity residuals (recipients, Mode A, cloud detect/body links, attach-content identity), the highest-value **remaining product-shaped** unique-export residual is store-key determinism: one function, one writer surface, outsized custody impact.

### 2.2 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `pst-writer/src/production.rs` `generate_store_record_key(path, message_count)` | Uses `SystemTime::now()` nanos + `process::id()` + path bytes + count; four salted `crc32fast::hash` → 16 bytes |
| Call site (~L1639) | One key per write; reused for store `PidTagRecordKey` (0x0FF9) **and** all folder EntryID ProviderUIDs |
| Self-consistency tests | `store_record_key_present_nonzero_and_matches_entry_id_provider_uid` **must stay** |
| Anti-constant test | `store_record_key_differs_across_separate_writes` asserts **different** keys across two writes (today relies on time/pid **or** different message counts) — **must be redesigned** for deterministic mode |
| Message `PidTagCreationTime` / `LastModificationTime` | Taken from **source submit time**, not wall clock — already stable |
| Temp staging name | `process_entropy_suffix` uses time+pid — **staging only**, not final PST bytes; leave alone |
| `WritePstOpts` | No store-key field today |
| unique-pst write order | Final write order is sourced from **`keep_set.winners` (Vec)**; `prepared_by_locus: HashMap` is lookup-only (not iterated for output order) — **safe today** |
| keepset grouping | `by_key: HashMap` drives union-find; group rebuild iterates **`groups` Vec** (first-seen seed order) — **safe today** |
| Deps | `pst-writer` already depends on workspace **`sha2`** + **`md-5`**; no new crate required |

### 2.3 Industry / standards anchors (researched 2026-07-29)

| Anchor | Relevance |
|---|---|
| **NIST CSRC — chain of custody** | Custody tracks integrity of evidence through handling; a production digest that **changes when content does not** weakens the seal between re-runs |
| **NIST SP 800-86** | Digital forensics guidance; commonly paired with dual-hash practice (SHA-256 + MD5) — aligns with 0080’s existing `sha256_hex`/`md5_hex` report pair |
| **ISO/IEC 27037** (industry floor with NIST in CoC literature) | Evidence-handling processes should be auditable, **repeatable**, and **reproducible** |
| **EDRM MIH / production hashes** | Industry custody fields are **content-derived and reproducible**; volume digests should not be salted with wall-clock entropy when content is identical |
| **MS-PST / MAPI `PidTagRecordKey`** ([Microsoft Learn canonical property](https://learn.microsoft.com/en-us/office/client-developer/outlook/mapi/pidtagrecordkey-canonical-property)) | Must be unique within the same message store/provider instance and binary-comparable (`memcmp`); **no requirement for cryptographic randomness or unpredictability** — backs path-independent deterministic construction |
| **0079 residual text** | Explicit: fixing this “would make the reported hash a meaningful chain-of-custody value” and “changes every produced PST’s record key” → product decision **this track locks** |

Not claiming EDRM MIH semantics for store keys — only that **store identity and (when layout-stable) volume digests should not be salted with wall-clock entropy**.

### 2.4 Dependency pins (researched 2026-07-29)

| Dep | Workspace / lock | crates.io max (access 2026-07-29) | 0087 |
|---|---|---|---|
| **sha2** | `0.11` workspace; lock **0.11.0** (+ dual **0.10.9** elsewhere) | **0.11.0** (released 2026-03-25) | **KEEP** — already used by writer hash path; use for store-key SHA-256 |
| **crc32fast** | used today for record key | n/a | **Drop from record-key path** (may remain elsewhere) |
| **rand** | workspace `0.8`; crates.io latest **0.10.2** | 0.10.2 | **Do not add** for store key |
| **uuid** | workspace `1` features v4; crates.io **1.24.0** | 1.24.0 | **Do not add** for store key |

No Cargo dep bumps required for P0. Dual sha2 0.10/0.11 and `crc32fast` elsewhere stay as-is. Unifying sha2 versions is a separate INFRA track — not 0087.

### 2.5 Product decision (LOCKED)

| Decision | Lock |
|---|---|
| **Default mode** | **Deterministic** for all production unique-PST / `write_unicode_pst*` paths |
| **Ephemeral mode** | Optional escape hatch only: `WritePstOpts.store_record_key_mode = Ephemeral` (time+pid style) for rare “force unique store identity” / debug — **off by default**; CLI surface only if cheap (`--store-record-key deterministic\|ephemeral`, default `deterministic`) |
| **Path independence** | **Exclude absolute output path** from the preimage — same winners written to different dest paths must share the same store key |
| **Multi-volume** | Include **`volume_index`** (0-based) so each volume is a distinct store with a distinct deterministic key |
| **Volume layout coupling** | **Documented product fact:** changing `--max-volume-bytes` (or any policy that changes volume membership) **breaks** per-volume key and volume-digest reproducibility even when the global winner set is identical. Re-run stability requires **identical chunking layout**, not only identical winners. |
| **Job-level seed (preferred when available)** | unique-pst **should** pass a **global export fingerprint** as `store_key_material` when cheap (e.g. digest of ordered keep-set winner loci / keep_set content), so each volume key is bound to the **whole job** and salted by `volume_index` — not only the volume’s message subset. Fallback derive (§2.6.2) remains for bare writer tests. |
| **All-zero ban** | If truncated digest is all zeros, force `key[0] = 0x5A` (or domain constant) so the non-zero test remains true |
| **Self-consistency** | ProviderUID **still equals** `PidTagRecordKey` byte-for-byte (unchanged invariant) |
| **Byte-identity vs logical identity** | **RecordKey determinism is the hard guarantee.** Full `sha256_hex` byte-identity is **best-effort**; if B-tree/layout entropy remains, DoD-3 falls back to **0079 structural equivalence oracle** and honest runbook wording — **track does not block** on full byte-identity |
| **No schema migration** | Writer-only; no matter schema |

### 2.6 Deterministic preimage (normative)

```
preimage =
  b"pst-dedup/store-record-key/v1\0"
  || algo_version_u32_le          // constant 1 for this track
  || volume_index_u32_le          // 0 for single-volume / default
  || message_count_u64_le
  || content_fingerprint          // 32 bytes, see below

record_key_16 = SHA-256(preimage)[0..16]
if record_key_16 is all 0x00: record_key_16[0] = 0x5A
```

**`content_fingerprint` (32 bytes) — construction order locked:**

1. If `WritePstOpts.store_key_material: Option<[u8; 32]>` is **Some**, use it as the primary fingerprint input:
   ```
   content_fingerprint = SHA-256(
     b"pst-dedup/store-key-material/v1\0"
     || store_key_material
     || volume_index_u32_le       // re-bind material to this volume (material may be job-global)
     || message_count_u64_le
     || volume_local_fingerprint  // §2.6.2 over messages actually on this volume
   )
   ```
   When material is absent, `content_fingerprint = volume_local_fingerprint` only.

2. **`volume_local_fingerprint`** — derive from the ordered list of messages **actually written to this volume** (after any volume split), for each message in **write order**, using **length-prefixed fields only** (no bare null terminators between variable fields):

   ```
   SHA-256 over concatenation of per-message records:
     for each msg:
       b"msg\0"                                    // fixed domain tag only
       || len_u32_le || utf8(internet_message_id)  // empty → len=0
       || len_u32_le || utf8(subject)
       || submit_time_filetime_i64_le              // fixed 8 bytes
       || len_u32_le || utf8(source_folder_path)
   ```

   - `len_u32_le` is the **byte length** of the following UTF-8 slice (not char count).
   - Empty MID/subject/path are allowed (`len = 0`, zero payload bytes).
   - **Forbidden:** `utf8(field) || b"\0"` variable-field framing — null-byte boundaries are ambiguous if a field ever contains `0x00` or if operators craft adversarial subjects/paths.
   - **Do not** include wall clock, PID, or dest path.

3. **Unit pure function:** expose `fn derive_store_record_key(volume_index, message_count, content_fingerprint) -> [u8; 16]` (and a pure fingerprint builder) so tests assert exact bytes without writing a full PST.

**Why SHA-256 not CRC32:** store key is now a custody-adjacent identity; domain-separated SHA-256 matches 0086 unread sentinels and volume digests; already a dep; 16-byte truncation is standard for fixed-width MAPI keys (not claiming full 256-bit strength for the truncated form).

**Why length-prefix:** cryptographic serialization standard; eliminates boundary confusion between adjacent variable fields.

### 2.7 Residual non-determinism inventory (Phase 0 must re-verify)

| Source | Final PST bytes? | 0087 action |
|---|---|---|
| `generate_store_record_key` time+pid | **Yes** | **Fix (core)** |
| Temp staging filename entropy | No (rename replaces) | Leave |
| Message creation/mod times | From source submit | Already stable |
| NID assignment | Write-order based (when order is Vec-stable) | Document; verify order sources |
| Multi-volume split points | Policy + size | Same inputs → same splits (assert); **changing max-volume-bytes is expected non-repro** |
| **B-tree / page / block allocation** | Possibly | **Do not block track**; measure in DoD-3; fallback to 0079 structural oracle + residual if needed |
| **HashMap/HashSet iteration for output order** | Possibly | Phase 0 **must** audit; live unique-pst path is Vec-ordered today (§2.2) — lock that invariant |
| Other wall-clock in production path | Phase 0 grep | Fix only if it lands in final bytes; else residual |

**Phase 0 grep checklist (normative — not only “random-looking” APIs):**

1. `SystemTime::now`, `process::id`, `rand`, `Uuid::new_v4` / `uuid::`
2. **`HashMap` / `HashSet` / `BTreeMap` used such that `.iter()` / `.values()` / `.keys()` determines write order, NID assignment, or folder order** — Rust `RandomState` is **per-thread cached**; iteration order is stable within one process/thread run but **can differ across separate process launches** (the real re-export scenario). Symbols like `rand` never appear — ordinary std iteration is enough to break cross-invocation byte-identity.
3. Any other entropy that lands in final PST bytes.

If Phase 0 finds **another** final-byte entropy source outside RecordKey:

- Fix in-scope when cheap, **or**
- Open **D-0087-*** residual, **and**
- DoD-3 uses **structural fallback** — do **not** claim full volume-file byte-identity.

### 2.8 Deferred roll-in (this track)

| ID | Disposition in 0087 | Why |
|---|---|---|
| **D-0079-deterministic-key** | **Ship / close** | Core deliverable |
| **D-0070-inline-hash-io** | **Decline** | Writer finalize seeks; separate format track |
| **D-0073-eml** | **Decline** | unique-eml attach ledger CSV parity — different surface |
| **D-0084-cloud-named-prop-write** | **Decline** | Writer NPMAP encyclopedia — different product |
| **D-0085-sovereign-cloud-hosts** | **Decline** (document next-candidate) | Body URL allowlist; **research now shippable** (see §2.9) but different crate — do not couple |
| **D-0086-embedded-email-hash** | **Decline** | Relativity recursive AttachmentHash for embedded msg |
| **D-0086-digest-probe-unify** | **Decline** | Perf double-I/O residual |
| **D-0076-default-v2** | **Decline** | Identity default product |
| **D-0079-stream-prepare** | **Decline** | RAM pipeline Phase C |

### 2.9 Research note — sovereign hosts (not in scope, for board)

Microsoft endpoint tables (access 2026-07-29):

| Cloud | SharePoint / OneDrive hosts (published) |
|---|---|
| **GCC High** | `*.sharepoint.us`; `admin.onedrive.us` (endpoints last updated **2026-07-01**) |
| **DoD** | `*.sharepoint-mil.us`; `*.dps.mil` (endpoints last updated **2026-06-30**) |
| Commercial SafeLinks | `*.safelinks.protection.outlook.com` (already 0085) |
| Sovereign SafeLinks | Public docs still emphasize commercial rewrite prefix; observed `gcc*.safelinks.protection.outlook.com` patterns exist; **`.us` SafeLinks host matrix remains thinner** than SharePoint — next track should allowlist SharePoint/OneDrive sovereign hosts with tests and treat SafeLinks sovereign as best-effort residual if still under-documented |

This **unlocks** a future thin track for **D-0085-sovereign-cloud-hosts**; it does **not** fold into 0087.

### 2.10 CLI / report honesty

| Surface | Behavior |
|---|---|
| unique-pst summary JSON | Optional field `store_record_key_mode: "deterministic" \| "ephemeral"` (default deterministic) |
| Report digests | Document: under default mode, **identical winners + identical tool version + identical volume layout** ⇒ **identical RecordKey**. Volume-file `sha256_hex` match is **best-effort** (layout-stable path); if not, structural equivalence still holds (0079). |
| Volume layout | **Mandatory runbook line:** changing `--max-volume-bytes` / volume membership **breaks** per-volume key and volume digest reproducibility even when the global winner set is unchanged. |
| unique-pst-export.md / runbook | Chain-of-custody section: re-run reproducibility; Mode A / identity levels still affect *which* winners are selected (hash stability is after keep-set resolve); B-tree honesty sentence from §2.5. |
| **0086 synergy (document, not mandate)** | Operators already paying for `--strong-content-hash body-recip-attach` may pass an **aggregate strong-content hash** (or keep-set fingerprint that embeds it) as `store_key_material` so the store key is attachment-byte-aware. Default derived fingerprint remains **metadata-only** (MID/subject/time/folder) so bare writer/unique-pst never forces attach I/O for store keys. |
| Desk | No new wizard control required (CLI default is enough) |

### 2.11 Tests (required)

1. **Pure unit:** fixed preimage → exact 16-byte key (golden bytes in test); length-prefix boundary case (subject/path that would confuse null-terminated framing).
2. **Same content same key (in-process writer):** same messages, different dest paths → identical RecordKey/ProviderUID.
3. **Same content same key (cross-process CLI — normative for DoD-2):** **two genuine separate process invocations** of the built CLI (`std::process::Command` / spawn `pst-dedup unique-pst` twice), different dest paths, identical inputs/flags → identical RecordKey read back from both PSTs.  
   - **Rationale:** same-process tests reuse thread-cached `HashMap` `RandomState` and can **false-pass** if a future refactor iterates a map for write order; real re-exports are new processes.
4. **Same content same volume digest (DoD-3):**  
   - **Try** fixture-scale cross-process proof that `sha256_hex` matches.  
   - **If fails:** record residual (if any), prove **0079 structural equivalence oracle** green for the pair, and pass DoD-3 via structural fallback — **do not fail the track solely for volume-file hash mismatch**.
5. **Different message sets → different keys.**
6. **volume_index 0 vs 1 → different keys** (same local messages + same job material if synthetic).
7. **Self-consistency:** ProviderUID == RecordKey; non-zero.
8. **Caller seed:** `store_key_material = Some(X)` changes fingerprint; different seeds → different keys; same seed + same volume_index → same key.
9. **Ephemeral (if shipped):** two ephemeral writes differ (time-dependent — may use distinct material if flaky otherwise).
10. **Regression:** existing writer v1 self-consistency tests still green; redesign `store_record_key_differs_across_separate_writes` so it no longer requires wall-clock entropy (use different content or different volume_index).

### 2.12 Risks

| Risk | Mitigation |
|---|---|
| Operators expected unique digests per re-run | Docs: RecordKey seals **logical store identity**; ephemeral escape if needed |
| Null-byte field boundary confusion | **Length-prefixed** fields only (§2.6) |
| Volume size change surprises | Mandatory runbook + DoD honesty; job-level `store_key_material` when available |
| B-tree / allocation layout shifts volume hash | DoD-3 structural fallback; runbook honesty; residual if needed |
| HashMap order across processes | Phase 0 map-iteration audit; cross-process CLI tests |
| Collision if preimage too weak | Domain sep + SHA-256 + length-prefix fingerprint; not CRC |
| Path exclusion surprises multi-machine | Document path independence as intentional |
| Multi-volume wrong index plumbing | Thread volume index from unique-pst; unit tests |
| Claim full byte-identity while other entropy remains | Phase 0 inventory; DoD-3 fallback; residual |
| Breaking “differs across writes” test | Redesign test; do not keep false requirement |

### 2.13 Review fold-in (2026-07-29) — dual-AI disposition

| ID | Claim | Disposition | Spec effect |
|---|---|---|---|
| **A1-1** | Null-terminated variable fields allow boundary confusion / ambiguous preimage | **Agree** | §2.6 uses **`len_u32_le \|\| utf8(field)`** only; unit test for adversarial-boundary case |
| **A1-2** | Volume layout change breaks digests even when global winners identical; prefer global winner-set binding | **Partial agree** | (1) **Mandatory** runbook/DoD honesty: layout coupling is expected. (2) unique-pst **should** pass **job-global** `store_key_material` when cheap, re-bound with `volume_index` + volume-local fingerprint. (3) Decline “only global set, ignore per-volume messages” — volume membership still belongs in the key so two volumes of the same job differ for content as well as index. |
| **A1-3** | B-tree allocation can shift volume `sha256_hex`; do not block track; fall back to 0079 structural oracle | **Agree** | Objective + §2.5 + §2.7 + **DoD-3** rewritten: RecordKey is hard guarantee; byte-identity best-effort with structural fallback |
| **A2-1** | Phase 0 must audit HashMap/HashSet output-order; DoD-2/3 need **cross-process** CLI tests (RandomState is per-thread) | **Agree** | §2.2 live note (path safe today); §2.7 grep checklist; §2.11 tests 3–4 require separate process invocations |
| **A2-2** | PidTagRecordKey uniqueness does not mandate randomness | **Agree (confirm)** | §2.3 Microsoft Learn citation; no code change |
| **A2-3** | ISO/IEC 27037 + NIST SP 800-86 align with CoC framing | **Agree** | §2.3 anchors expanded |
| **A2-4** | Document optional 0086 aggregate strong hash as `store_key_material` | **Agree (docs only)** | §2.10 synergy; **not** mandated (no forced attach I/O for store key) |
| **A2-5** | Path independence, all-zero guard, drop crc32 from this path, dual sha2 KEEP | **Agree (confirm)** | Unchanged |

---

## 3. In scope

1. Replace `generate_store_record_key` with deterministic domain-separated SHA-256 construction (§2.6), **length-prefixed** field encoding.
2. Plumb `volume_index`, optional `store_key_material`, and optional mode into `WritePstOpts` (or equivalent).
3. Thread volume index from unique-pst multi-volume writer path; prefer job-global seed when cheap.
4. Tests: pure unit (incl. length-prefix boundary) + in-process writer + **cross-process CLI** key proof; DoD-3 digest attempt with structural fallback.
5. Docs: unique-pst-export + eDiscovery runbook (CoC, volume-layout coupling, B-tree honesty, optional 0086 seed); CHANGELOG; close **D-0079-deterministic-key**.
6. Phase 0 residual non-determinism inventory including **HashMap/HashSet write-order** audit.

## 4. Out of scope (do NOT do here)

- unique-eml attach ledger CSV (**D-0073-eml**).
- Sovereign-cloud body URL hosts (**D-0085-sovereign-cloud-hosts**) — see §2.9 for research handoff only.
- Named-prop NPMAP write (**D-0084-cloud-named-prop-write**).
- Embedded-email recursive AttachmentHash (**D-0086-embedded-email-hash**).
- Digest/probe unify (**D-0086-digest-probe-unify**).
- Writer finalize forward-only / inline hash restructure (**D-0070-inline-hash-io**).
- Default identity v2 (**D-0076-default-v2**).
- Parallel `--jobs` / stream-prepare (**D-0079-stream-prepare**).
- Authenticode (**D-0062-codesign**).
- Changing temp-staging entropy (not final bytes).
- Matter schema / Desk wizard UI for store-key mode.
- Mandating attach-content I/O solely to feed store_key_material (0086 synergy is opt-in/docs).
- Guaranteeing full volume-file byte-identity on every platform if B-tree layout drifts (structural fallback is the honest stop).

## 5. Preconditions & dependencies

- **P1 (blocking):** `pst-writer` production write path + unique-pst multi-volume plumbing.
- **P2:** 0079 structural oracle remains available for DoD-3 fallback.
- **P3:** 0080 report digests present so DoD can attempt same `sha256_hex` and compare.
- *Verified to date (2026-07-29):* time+pid in `generate_store_record_key`; ProviderUID self-consistency tests; message times from submit; sha2 0.11.0 KEEP; dual 0.10.9/0.11.0 in lock; unique-pst write order Vec-based; D-0079-deterministic-key open in deferred; PidTagRecordKey MS Learn: uniqueness without randomness.

## 6. Risks

See §2.12. Summary: ambiguous preimage (mitigated by length-prefix); overclaiming volume-file byte-identity (mitigated by DoD-3 structural fallback); HashMap order across processes (Phase 0 + cross-process tests); volume-layout operator surprise (runbook).

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Deterministic default:** production `write_unicode_pst*` uses §2.6 preimage (length-prefixed fields); no wall-clock/PID/path in store key by default.
- [ ] **DoD-2 — Same content same key:** (a) in-process writer proof **and** (b) **two separate CLI process invocations**, identical messages/flags, **different dest paths** → identical 16-byte RecordKey and ProviderUID.
- [ ] **DoD-3 — Volume digest / structural fallback (not a hard byte-identity gate):** attempt fixture-scale cross-process same `sha256_hex`. **Pass if either:** (A) digests match, **or** (B) digests differ but **0079 structural equivalence oracle** reports equivalent and residual/runbook states: *“Store Record Key is deterministic (logical identity). Byte-for-byte volume hash reproducibility is subject to B-tree/layout stability.”* Track **must not** stay blocked solely for (B).
- [ ] **DoD-4 — Differentiation:** different message content and different `volume_index` produce different keys; non-zero; ProviderUID == RecordKey; length-prefix boundary unit test green.
- [ ] **DoD-5 — Plumbing:** unique-pst multi-volume passes volume index; job-global `store_key_material` when cheap; pure unit golden for preimage.
- [ ] **DoD-6 — Docs + deferred:** unique-pst-export + runbook CoC note including **volume-layout coupling**, **B-tree honesty**, optional **0086 seed synergy**; **D-0079-deterministic-key closed**; CHANGELOG `[Unreleased]`; §2.9 sovereign note does **not** close D-0085.
- [ ] **DoD-7 — Gates:** `cargo fmt --all --check`; `clippy -D warnings`; `cargo test --workspace` (or justified narrow + full before commit); ledger FEATURE committed.
- [ ] **DoD-8 — Recorded:** `review.md` with preimage formula (length-prefix), Phase 0 inventory (incl. HashMap order), fold-in §2.13, DoD-3 path taken (A or B), test evidence, residual list; board **Completed**.

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-writer -- record_key
cargo test -p pst-writer -- store_record
cargo test -p pst-dedup-cli --test unique_pst -- --nocapture
cargo test --workspace
ledgerful verify
```
