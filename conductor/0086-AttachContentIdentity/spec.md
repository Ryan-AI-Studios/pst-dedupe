# 0086 — Attach-Content Strong Identity

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.

- **Track ID:** 0086-AttachContentIdentity
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after 0082–0085
- **Cross-repo contract:** n/a
- **Status:** Completed (2026-07-29; Codex luna PASS WITH DEFERRED P3)
- **Depends on:** 0074 · 0076 · 0082 · 0083 · 0084 (all **Completed** on board); 0085 Completed for Mode A / cloud honesty cross-links
- **Spec authored:** 2026-07-29
- **Series:** M (Unique export fidelity residuals)
>
> **Review fold-in (2026-07-29):** dual-AI review of Ready draft incorporated below.
> Disposition of each claim is in §2.12 (agree / partial / decline with reason).

---

## 1. Objective

Ship the deferred **Tier-2.5 attach-content identity level** — enable `--strong-content-hash body-recip-attach` so keep-set / scan / unique export can bind (and **split**) on **per-attachment native SHA-256 digests**, not only name+size metadata — without inventing digests for unread/cloud attaches, without multi-GB `Vec`s, and without changing the default identity (still `off` / v1).

**Outcome:** Operators can opt into a Relativity-class **AttachmentHash** component for unique export identity. Two messages that share subject/sender/time/preview/body/recipients but carry **different file bytes** under the same attach names no longer collapse into one winner under this level. Unreadable or cloud-link attaches never fabricate equality.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0076-attach-content** | P2 | CLI **rejects** `body-recip-attach` with an explicit pointer here; enum + hasher preimage already exist; digests never populated |
| 0076 honesty | — | Relativity four-component model is documented; our attach component is still **name:size only** at live levels |
| 0083/0084 Mode A | — | Promote prefers physical over MAPI cloud-link **within a group**; attach-byte identity can **fracture** incomplete vs complete into different groups — Mode A cannot cross that split |
| 0084/0085 cloud | — | Cloud attaches have **no offline payload**; body-inline links never become attaches — attach-content level must treat missing bytes as **unread**, not empty hash |

Board line after 0085 Completed: *Later Series M residuals: **D-0076-attach-content**, D-0079-deterministic-key (product), D-0073-eml full ledger, D-0084-cloud-named-prop-write, D-0085-sovereign-cloud-hosts.* This track is the highest-value **implementable** residual (not product-policy alone).

### 2.2 Industry anchors (researched 2026-07-29)

**RelativityOne — Deduplication considerations** ([help.relativity.com](https://help.relativity.com/RelativityOne/Content/Relativity/Processing/Deduplication_considerations.htm), last modified **2026-Jul-02**, access 2026-07-29):

Email dedup uses **four component SHA-256 hashes** (not whole-item MD5/SHA alone):

| Component | Relativity construction |
|---|---|
| MessageBodyHash | `PR_BODY` (or RTF/HTML→Unicode) with CR/LF/space/tab stripped |
| HeaderHash | Subject + SenderName + SenderEMail + ClientSubmitTime |
| RecipientHash | Loop **all** recipients including **BCC** |
| **AttachmentHash** | SHA-256 **per attachment native**; non-email = “normal standard SHA256 file hash” (Relativity wording — **no published block size**; our stream chunk size is an implementation detail and does not change the digest); embedded email = recursive four-component then outer compose |
| Processing duplicate hash | SHA-256 over the four component hashes composed |

Explicit: *“If two emails have an identical body, attachment, recipient, and header hash, they are duplicates.”* Explicit: whole-file MD5/SHA1/SHA256 **are not** the email dedup key.

**Product mapping (locked, not byte-identical Relativity):**

| Relativity | Our live path | 0086 |
|---|---|---|
| Header + body preview | Tier-2 v1 | unchanged |
| Full body | `--strong-content-hash body` | already shipped |
| Recipients (display / TC) | `body-recip` (+ 0082 TC keys) | already shipped |
| Attachment **content** digests | **rejected CLI** | **this track** |

We stay on the 0076 **refinement** model: v2 preimage = **byte-identical v1 preimage** ∥ extras. That differs from Relativity’s “four separate hashes then outer hash,” but preserves split-only safety and existing v1 report keys. **Do not** switch the default identity to v2 (**D-0076-default-v2** stays open).

**Microsoft Purview** (dedup on `InternetMessageId` + topic + BodyTagInfo; cloud attach collection is service-side): does **not** replace attach-content identity. Offline PST unique-export still needs byte-level attach binding when operators opt in.

### 2.3 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `IdentityLevel::BodyRecipAttach` | Enum + `includes_attach_content()` + `as_str` = `body-recip-attach` |
| `hasher::AttachmentInfo.content_sha256` | `Option<[u8; 32]>` — always `None` on scan path today |
| `compute_strong_content_hash` | If `includes_attach_content()`, sorts present digests and updates hasher; **today missing digests simply omit** — **0086 must replace with real\|sentinel per slot (Choice B)** |
| `GroupingStats.strong_hash_attach_unread` | Counter + human summary line exist; never incremented in production path |
| CLI `parse_identity_level` | **Hard-rejects** `body-recip-attach` with D-0076-attach-content message |
| `scan.rs` | Builds `AttachmentInfo::new(filename, size)` + `is_inline`; never opens attach streams for hash |
| 0074 `attach_probe` | L0–L3 integrity probe; **discards** stream bytes; `ProbeOutcome` has **no digest field** |
| `pst_reader::open_attachment_data` | Streaming `Read` over attach binary; production path must not materialize multi-GB `Vec` |
| 0084 `is_cloud_link` | Cloud attach → incomplete; **no offline bytes** for content hash |
| Mode A (0083) | Pre-write promote within keep-set **group** only |

### 2.4 Dependency currency (re-queried crates.io 2026-07-29)

No new crates. Use workspace **`sha2`** already on the strong-hash path. No network client.

| Dep | Workspace / lock | crates.io max | 0086 |
|---|---|---|---|
| sha2 | 0.10.9 + 0.11.0 both present | **0.11.0** (released 2026-03-25) | **KEEP** dual (dedup-engine already uses workspace pin; no forced 0.10→0.11 churn). **Past** RUSTSEC-2021-0100 (sha2 0.9.7 AVX2 multi-block miscompute; fixed 0.9.8) — still mandate NIST KATs (§2.11) so a future backend cannot silently false-merge |
| clap | 4.6.4 | 4.6.4 | KEEP |
| camino | 1.2.5 | 1.2.5 | KEEP |
| serde_json | 1.0.151 | 1.0.151 | KEEP |
| uuid | 1.24.0 | 1.24.0 | KEEP |
| thiserror | 2.0.19 (ws 2) | 2.0.19 | KEEP |
| regex | 1.13.1 | 1.x | unused here |
| eframe | 0.34.2 | 0.35.0 | DECLINE_MAJOR |
| reqwest | 0.12.x (+0.13 present) | 0.13.x | DECLINE_MAJOR |

**0081 override:** only bump on High/Critical advisory. This track is **feature wiring**, not a dep-refresh track.

### 2.5 Locked product rules

1. **Sources read-only.** Never mutate source PSTs.
2. **Default identity unchanged.** `--strong-content-hash` default remains `off`. `body-recip-attach` is **opt-in split-only**.
3. **Never invent digests from failure.** Open fail, CRC/stream fail, timeout, budget truncate mid-stream, cloud-link / no binary, length mismatch, empty method without data → **unread sentinel** under §2.6 (Choice **B**, locked) — **not** a silent omit and **not** a tier downgrade to `body-recip`. Count `strong_hash_attach_unread`. Do not claim payload-read success for unread slots.
4. **No multi-GB attach `Vec`.** Stream via `open_attachment_data` + fixed chunk (64 KiB class, same family as 0074 `DISCARD_CHUNK`); feed `sha2` incrementally. Chunk size is **not** a Relativity-cited parameter.
5. **0074 integrity probe ≠ identity digest by default.** L2 head-read is **insufficient** for AttachmentHash (partial bytes would false-merge). Identity level requires a **full-stream digest pass** (or L3 full with hashing), separately gated and budgeted. Optional reuse of open handles / sticky cache is fine; **do not** treat head-probe success as digest equality.
6. **Cloud / modern attaches (0084):** `is_cloud_link` or method/path-only web-ref without binary → **unread sentinel** for attach-content (no offline payload). Ledger cloud URL remains the operator path; do not invent file digests from URL strings.
7. **Body-inline links (0085):** never attachment-table rows → not in attach-content preimage. Mode A known gap (physical vs HTML-inline) **unchanged**.
8. **Inline ignore (soft warning, not hard reject):** `--identity-ignore-inline-attachments` continues to omit inline attaches from **both** name:size and content components when set. Combined with `body-recip-attach`, this **softens** the byte-strict promise (logo/signature variance still filtered). CLI **must** emit a **one-line stderr warning** when both flags are set; **do not** hard-reject — operators still need logo-induced false-split control on mass-mail. Docs state the trade-off explicitly.
9. **Embedded message attaches (`ATTACH_EMBEDDED_MSG`):** P0 hashes the **raw attach data stream** as a binary blob (standard SHA-256). Recursive Relativity-style “four-component email inside email” is **out** unless Phase 0 proves it is cheap on existing fixtures — residual **D-0086-embedded-email-hash** if deferred.
10. **Order / determinism:** digests sorted (existing hasher) so attach-table order does not change the strong hash.
11. **Surfaces:** enable on `scan`, `dups`, `keep-set`, `unique-eml`, `unique-pst` the same way other identity levels work. Desk remains checkbox→`body` only (**D-0076-gui** residual).
12. **Split-only:** equal-`body-recip-attach` ⇒ equal-`body-recip` ⇒ equal-v1 by construction (v1 prefix unchanged). Fixture refinement tests must include this level.
13. **Performance honesty:** this level is **expensive** (full attach I/O). Document budgets, cancel, and multi-GB operator residual. Never imply it is free like `body`.
14. **Mode A interaction (mandatory docs):** attach-content identity can place a complete physical copy and an incomplete/cloud copy into **different keep-set groups**. Mode A only promotes **within** a group. Runbook must state: *enable `body-recip-attach` when attach-byte fidelity matters for grouping; Mode A is not a substitute for attach-content identity.*

### 2.6 Digest acquisition policy (normative)

When `identity == BodyRecipAttach` and attachments are included:

For each non-ignored attachment (respect inline filter), produce **exactly one** attach-content slot (real digest **or** unread sentinel — never omit the slot):

| Condition | Slot value | Stat / behavior |
|---|---|---|
| By-value binary stream fully read; **bytes_read matches declared size** (or declared size unknown / 0 with pure empty success — see empty row) | `Some(real_sha256)` | success |
| Stream open fails / CRC fail / IO error | **Unread sentinel** | `strong_hash_attach_unread++` |
| Timeout / cancel mid-hash | **Unread sentinel** | unread + cancel semantics |
| Budget: max attaches / max digest bytes / per-attach cap exceeded | **Unread sentinel** for remaining / truncated | unread + truncation flag |
| Cloud-link / no binary payload | **Unread sentinel** | expected, not a bug |
| **Length mismatch:** declared `size > 0` but stream EOF at 0 (or `bytes_read != size` when size is authoritative) | **Unread sentinel** | **not** empty-file success — corrupt / truncated payload |
| Zero-size by-value: declared size **0**, open succeeds, immediate EOF | `Some(SHA-256(""))` = `e3b0c442…` | legitimate empty file; **v1 name:size already separates** `Financials.xlsx:0` from `Contract.pdf:0` |

**Empty-file note (AI2 fold-in, partial):** Blanketing all 0-byte streams as unread would false-split real identical empty placeholders. Different corrupt empty **names** already split on the v1 `name:size` component. The spoliation trap is **length mismatch** (metadata claims content, stream is empty) and **silent omit** of failed digests — both forbidden above.

**Incomplete attach-content item binding rule (LOCKED — Choice B only):**

- **Choice A (tier downgrade to `body-recip`) is declined.** Downgrading a message whose attach stream failed would let it false-merge with a **no-attachment** peer that shares body/recipients, while fracturing away from the peer that **successfully** hashed the same PDF — group hijacking / keep-set instability.
- **Choice B (required):** every identity-relevant attach contributes a 32-byte slot:
  - Real content: `SHA-256(stream_bytes)`.
  - Unread: **domain-separated sentinel digest** that incorporates at least **normalized filename + declared size** (and a fixed domain tag), e.g. conceptually  
    `SHA-256( b"pst-dedup/attach-unread/v1\0" || name_lower || b"\0" || size_le_u32 )`  
    so unread `Contract.pdf` ≠ unread `Financials.xlsx` ≠ `SHA-256("")` ≠ any honest file digest (domain tag + structural inputs; collision with real content is computationally infeasible for honest SHA-256).
- Slots are still **sorted** before folding into the strong preimage (order-independent).
- **Forbidden:**
  - Omitting missing digests so two messages with different failed attaches hash equal via empty attach tails.
  - Binding an incomplete item at a lower strong level while peers remain at `body-recip-attach`.
  - Using a **static** single `UNREAD` constant for every failure (would false-merge unread PDF with unread Excel when names/sizes differ in intent but were collapsed).

**Hasher change implication:** today’s `filter_map(|a| a.content_sha256)` that **drops** `None` is incorrect for 0086 — replace with “real or sentinel per slot” (may store sentinel in `content_sha256` as `Some(sentinel)` with a parallel `attach_digest_kind` / unread flag for stats, or feed sentinels only inside strong-hash construction). Phase 1 unit-tests must prove unread-PDF ≠ unread-XLSX ≠ empty-file ≠ successful content.

### 2.7 Budgets (defaults — Phase 0 may tighten)

Reuse 0074 budget *shape* with identity-specific names (do not silently share L2 head caps for full digests):

| Budget | Suggested default | Notes |
|---|---|---|
| Max attaches digested per run | 50_000 | align 0074 |
| Max digest bytes per run | **1 GiB** (or 256 MiB if Phase 0 measures fixture risk) | full-stream; document |
| Per-attach max bytes | **unlimited under global** *or* very high (e.g. 512 MiB) | head 1 MiB is **wrong** for identity |
| Max open PSTs | 32 | sticky LRU (0079 cache if available) |
| Per-attach time | optional; cancel respects global | |

CLI: either inherit `--deep-attach-*` budget flags where they already exist, or add `--strong-hash-attach-*` only if reuse would mislead operators (prefer reuse with clear help text: *“body-recip-attach always full-stream digests; head probe is unrelated”*).

### 2.8 Deferred roll-in

| ID | Disposition in 0086 | Why |
|---|---|---|
| **D-0076-attach-content** | **Ship / close** | Core deliverable |
| **D-0076-default-v2** | **Decline** | Product: would rehash every report key |
| **D-0076-operator-perf** | **Narrow / document** | Record fixture timings for attach level; multi-GB remains operator residual |
| **D-0076-gui** | **Decline** | Enum surface stays CLI; Desk checkbox remains `body` |
| **D-0073-eml** | **Partial optional** | If unique-eml already runs strong-hash path, enabling the level is free; full attach-ledger CSV parity remains residual |
| **D-0079-deterministic-key** | **Decline** | Writer record-key product decision; unrelated to identity digests |
| **D-0084-cloud-named-prop-write** | **Decline** | Writer NPMAP; not hash identity |
| **D-0085-sovereign-cloud-hosts** | **Decline** | Body URL allowlist residual |
| **D-0067-cloud-attaches** / hydration | **Decline** | Never download cloud payloads offline |

**New residual (open if needed):**

| ID | Item |
|---|---|
| **D-0086-embedded-email-hash** | Recursive Relativity-style hash for embedded-message attaches (P0 = raw stream SHA-256) |
| **D-0086-digest-probe-unify** | Unify 0074 Full probe + identity digest into one streaming pass (perf polish) |

### 2.9 Architecture sketch

```text
scan / keep-set / unique-*
  | IdentityLevel::BodyRecipAttach
  v
for each msg with attaches:
  for each non-ignored attach:
    if cloud/no-binary/fail/length-mismatch → unread sentinel (name+size domain tag)
    else open_attachment_data → Read → sha2 chunked → real digest
      (bytes_read must match declared size when size authoritative)
  compute_dedup_keys_ex(... slots real|sentinel ..., StrongHashInput { identity: BodyRecipAttach, ... })
  group / index as today (always body-recip-attach strong hash when level set — no tier downgrade)
```

**Placement options (Phase 0 chooses; prefer least churn):**

1. **Helper in `pst-dedup-cli`** (e.g. `attach_content_hash.rs`) called from `scan.rs` when identity includes attach content — mirrors 0074 probe ownership.
2. Thin pure function in `dedup-engine` for “digest list → preimage” (already exists); I/O stays CLI/reader-side.
3. Optional cache: `(source_path, mtime/size, msg_nid, attach_nid, size) → sha256` for scan→unique reuse within process.

Do **not** put PST open loops inside `dedup-engine` pure hasher tests beyond unit fixtures with pre-supplied digests.

### 2.10 Surfaces & reporting

| Surface | Change |
|---|---|
| CLI help (`main`, `unique_pst_cmd`) | Live levels: `off\|body\|body-recip\|body-recip-attach` |
| `parse_identity_level` | Accept `body-recip-attach`; update reject tests → accept tests |
| Combined flags | When `body-recip-attach` **and** `--identity-ignore-inline-attachments`: **one-line stderr warning** (not hard reject) |
| `GroupingStats` | Ensure `strong_hash_attach_unread` increments; add `strong_hash_attach_digested` / `strong_hash_attach_bytes` if cheap |
| Optional attribution | `tier2_5_splits_attach_only` when fingerprints already support attach component |
| `keep_set_v1` / summary | Echo `identity_level`; human lines already partially exist |
| Decision CSV | `identity_version=v2`, `bound_by=content_hash_strong` when strong binds |
| Docs | unique-pst-export identity table; eDiscovery runbook when-to-use; Mode A × attach-content; sentinel + empty/length-mismatch; ignore-inline trade-off |

### 2.11 Tests (minimum)

1. **Unit (hasher):** two msgs identical body+recip, different real digests → different strong hash at `BodyRecipAttach`; equal digests → equal; sorted order independent of attach order.
2. **Unit (Choice B):** unread sentinel for `Contract.pdf` ≠ unread for `Financials.xlsx` ≠ `SHA-256("")` ≠ successful content digest; two identical name+size unreads **do** match (structural incomplete peers).
3. **Unit (no tier hijack):** message with unread attach at `body-recip-attach` does **not** share strong hash with a no-attach message that matches body+recip only (Choice A regression guard).
4. **Unit (empty vs length-mismatch):** declared size 0 + empty EOF → real empty digest; declared size > 0 + empty EOF → unread sentinel.
5. **Unit (NIST KAT — mandatory):** at least one fixed multi-block SHA-256 known-answer vector on the **same `sha2` path** used for attach streaming (e.g. NIST/FIPS “abc” **and** a multi-block vector such as 1 000 000 × `'a'` or any published ≥2-block vector) asserting **exact** expected digest bytes. Internal consistency alone would miss a RUSTSEC-2021-0100-class SIMD miscompute.
6. **Unit:** inline ignored attach does not participate when flag set.
7. **CLI parse:** `body-recip-attach` accepted; unknown still rejected; combined ignore-inline emits warning (test via stderr capture or warn counter if pure).
8. **Integration (synthetic PST / writer fixture):** same meta name:size, different file bytes → **two** winners under `body-recip-attach`; **one** under `body-recip` / `off` as appropriate.
9. **Cloud-link attach:** no binary → unread sentinel + stat; does not invent real digest; does not panic.
10. **Refinement:** `body-recip-attach` groups ⊆ `body-recip` groups ⊆ v1 groups on fixture matrix (**note:** unread sentinels still refine — they only subdivide further; never merge across v1).
11. **Cancel / budget:** mid-run cancel does not leave inconsistent partial claims; unread/truncation stats honest.
12. **Source PST unchanged:** full-file hash before/after optional integration (0076 pattern).
13. **No multi-GB Vec:** code review + existing stream API only (assert no `read_to_end` on attach path for this feature).

### 2.12 Dual-AI review fold-in (2026-07-29)

| # | Claim | Disposition | Spec impact |
|---|---|---|---|
| A1-1 | NIST multi-block KAT for attach SHA-256 path (RUSTSEC-2021-0100 class) | **Agree** | §2.11 item 5; Phase 1; dep note on sha2 past 0.9.7 |
| A1-2 | Drop “8 KiB blocks” as Relativity-attributed detail | **Agree** | §2.2 AttachmentHash row; chunk size is ours only |
| A2-1 | Never use `SHA-256("")` for 0-byte — always unread | **Partial** | Length mismatch → unread; **legitimate size-0 empty stream** keeps empty digest; v1 name:size already separates different empty filenames |
| A2-2 | Lock Choice B domain-separated name+size sentinel; decline Choice A tier downgrade | **Agree** | §2.6 locked B only; §2.11 hijack guard |
| A2-3 | Hard-reject `body-recip-attach` + `--identity-ignore-inline-attachments` | **Partial / decline hard reject** | Soft **stderr warning** + docs trade-off; logo false-split control remains valid |

---

## 3. In scope

1. Enable `--strong-content-hash body-recip-attach` on all identity-bearing CLI surfaces.
2. Full-stream per-attachment SHA-256 digests via `open_attachment_data` (+ budgets, cancel, sticky opens).
3. Wire **real digests or Choice B unread sentinels** into strong preimage (no omit, no tier downgrade).
4. Length-mismatch vs legitimate empty-file policy; stats; combined-flag warning.
5. NIST multi-block KAT on the attach hash path.
6. Docs + deferred close for D-0076-attach-content; Mode A / cloud / ignore-inline notes.
7. Tests in §2.11.

## 4. Out of scope (do NOT do here)

- Changing default identity to v2 (**D-0076-default-v2**).
- Network hydration of cloud/SharePoint payloads.
- Writer NPMAP named-prop re-emit (**D-0084-cloud-named-prop-write**).
- Deterministic store record key (**D-0079-deterministic-key**).
- Sovereign-cloud body URL hosts (**D-0085-sovereign-cloud-hosts**).
- Full unique-eml attach ledger CSV parity (**D-0073-eml**) beyond enabling the identity level if already shared.
- Desk enum UI for all identity levels (**D-0076-gui**).
- Recursive Relativity embedded-email AttachmentHash (unless Phase 0 proves free).
- BLAKE3 / alternate digests; stay SHA-256.
- Parallel `--jobs` attach digest redesign (0079 residual).

## 5. Preconditions & dependencies

- **P1 (blocking):** 0076 Completed (enum, preimage, reject path); 0074 Completed (`open` patterns, budgets); `open_attachment_data` streaming API.
- **P2:** 0082 recipient TC identity (body-recip base is correct).
- **P3:** 0083/0084/0085 Completed for Mode A + cloud honesty cross-links in docs.
- *Verified to date (2026-07-29):* CLI rejects `body-recip-attach`; hasher attach branch present; scan never fills `content_sha256`; Relativity AttachmentHash docs current 2026-07-02; crates.io sha2 max 0.11.0 KEEP dual.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Full-stream digests blow wall time on multi-GB | Opt-in only; budgets; progress/cancel; document; operator residual |
| Partial digests false-merge | Forbid head-as-identity; incomplete policy §2.6 |
| Silent SIMD/hash miscompute | NIST multi-block KAT (§2.11); pins past RUSTSEC-2021-0100 |
| Choice A group hijack | Locked Choice B only |
| Empty corrupt false-merge | Length-mismatch → unread; v1 name:size for different empty names |
| Cloud attaches “all unread” surprise | Docs + stats; still valuable for physical-attach families |
| Double I/O with 0074 Full probe | Residual unify D-0086-digest-probe-unify; cache by locus if same run |
| Silent enable without digest wire | Forbidden — tests must prove digests change grouping |
| Multi-GB `Vec` regression | Stream only; clippy/review; no `read_to_end` |
| Mode A over-claim after splits | Runbook: Mode A ≠ attach-content identity |
| Ignore-inline softens byte promise | Stderr warning + runbook; not hard reject |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — CLI level live:** `body-recip-attach` accepted on scan/dups/keep-set/unique-eml/unique-pst; help text updated; parse unit tests inverted from reject→accept; combined ignore-inline **warns** (not rejects).
- [ ] **DoD-2 — Digests computed:** identity-relevant by-value attaches get streaming SHA-256 under budgets; length-mismatch → unread; no multi-GB Vec.
- [ ] **DoD-3 — Grouping effect:** synthetic fixture proves different attach **bytes** (same name:size) split under `body-recip-attach` and not under `body-recip`.
- [ ] **DoD-4 — Choice B unread honesty:** cloud-link / open-fail / timeout use **name+size domain-separated sentinels**; no tier downgrade; `strong_hash_attach_unread` increments; hijack + multi-name unread unit tests green.
- [ ] **DoD-4b — NIST KAT:** multi-block known-answer test on attach SHA-256 path green.
- [ ] **DoD-5 — Refinement:** attach level only subdivides lower levels (automated assertion on fixture set).
- [ ] **DoD-6 — Default safe:** default `off` still passes prior goldens; no default-path new attach I/O.
- [ ] **DoD-7 — Docs + deferred:** unique-pst-export + eDiscovery runbook (when to use; budgets; Mode A; Choice B; empty vs length-mismatch; ignore-inline trade-off); **D-0076-attach-content closed**; optional D-0086-* residuals opened; CHANGELOG `[Unreleased]`.
- [ ] **DoD-8 — Gates:** `cargo fmt --all --check`; `clippy -D warnings`; `cargo test --workspace` (or justified narrow + full before commit); `cargo deny check` if deps touched; ledger FEATURE committed.
- [ ] **DoD-9 — Recorded:** `review.md` with fold-in table §2.12, sentinel formula, NIST vectors used, dep table, residuals; board **Completed**.

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dedup-engine -- hasher
cargo test -p pst-dedup-cli -- grouping_cli
cargo test -p pst-dedup-cli --test unique_pst -- attach_content
cargo test --workspace
cargo deny check
ledgerful verify
```
