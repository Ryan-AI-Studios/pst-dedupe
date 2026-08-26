# 0094 — Embedded Message Nested Export

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0094-EmbeddedMsgNestedExport
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N
- **Cross-repo contract:** n/a
- **Status:** Completed (Codex luna r5 PASS WITH DEFERRED P3; 2026-08-25)
- **Depends on:** 0069 · 0080 · 0090 · **0093** (all **Completed**)
- **Spec authored:** 2026-08-25
- **Series:** N (Operator fidelity — INC0102784 post-0092)
>
> **Review fold-in (2026-08-25):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.10. **PtypObject `0x3701` is in scope**; unique-eml nested MIME is not.

---

## 1. Objective

Wire unique-pst / production write so method-5 (`ATTACH_EMBEDDED_MSG`) attachments carry a
**nested `WriteMessage`** extracted from the source PST (bounded depth), instead of always
emitting `ATTACH_EMBEDDED_UNPARSED` soft-fails when the writer already knows how to build
embedded subnode objects — and so a spec-conformant client can **find** that nested message
via `PidTagAttachDataObject` (PtypObject), not only via our reader’s subnode scan.

**Closes / narrows:** `D-0067-embedded-depth` (unique-pst nested MAPI **export**).  
**Closes:** `D-0069-embed-object` (write `PidTagAttachDataObject` PtypObject on method-5 attaches).  
**Does not reopen:** `D-0086-embedded-email-hash` (closed in **0090** — identity hash ≠ export).

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784 unique-pst)

| Metric | Value |
|---|---|
| Attach fail reason | **100%** `ATTACH_EMBEDDED_UNPARSED` (374 / 374) |
| Attach method | all **5** |
| Unique parent msgs | **220** |
| Bytes represented (size sum) | ~**241 MB** |
| Folders | inbox 138, recoverable 98, sent 46, other/deleted/sync rest |
| Exit | `ATTACH_SOFT_FAIL` (+ QC verify) |

DoD-5 expects a **large drop** in unparsed, **not** a guaranteed `374 written / 0 failed / SUCCESS`. Depth/byte budget, unreadable nests, and child-attach stream fails can leave residuals.

### 2.2 Live code snapshot (verified 2026-08-25)

| Layer | State |
|---|---|
| `CanonicalAttachment` | No nested-message field (`keepset.rs` ~674–700). **`Serialize`/`Deserialize`**. |
| `CanonicalMessage` | Full winner DTO: `locus`, `content_hash: [u8;32]`, `fidelity`, `message_id_norm`, `edrm_mih_hex`, HTML body, `message_class`, flags. **`Clone`+`Debug` only — not serde**. |
| `from_canonical_message` / `_owned` | Hardcodes `embedded_message: None` (`production.rs` ~1028, ~1127) |
| Writer | `build_embedded_message_object` **exists**; method=5 + size; nested PC under attach **subnode leaf**; **no `0x3701` property**. `PcValue` has no `Object` variant. Comment: PtypObject residual. Test 12 (`writer_fidelity.rs` ~584–602) asserts **absence** of `PidTagAttachDataBinary` and documents PtypObject as residual. |
| Tag overlap | `PID_TAG_ATTACH_DATA_BINARY = 0x3701`. `PidTagAttachDataObject` is the **same property id** with **PtypObject `0x000D`**, not PtypBinary `0x0102`. |
| Reader identity | `messaging/embedded.rs` (0090): `MessageNodeRef`, `resolve_embedded_root(_nbt)`, `read_identity_from_message_node`, `list_attachments_from_message_node`, `list_recipients_from_message_node`, `open_attach_data_from_message_node`. `EmbeddedIdentityFields` has **no HTML, no `message_class`, no `message_id`, no flags**. Module docs: full extract is **out** (`D-0067-embedded-depth`). |
| NBT stream | `open_attachment_data` does `nbt.get(message_nid)` — **nested NIDs are not in the NBT**. Child binary attaches under a nest cannot use this API. |
| Depth constants | Writer `max_embedded_depth` default 3, clamp `[1, 8]`; reader `MAX_EMBEDDED_IDENTITY_DEPTH = 3`; engine `MAX_EMBEDDED_MSG_DEPTH = 3`. |
| QC sample | `select_sample_indices` already has a `has_embedded` stratum (0080). |
| Hygiene | Untracked `fixtures/keep_set_summary.json` (test-output-shaped) — Phase 0 inventory. |

### 2.3 Product locks

1. **Never invent** nested content — fail closed with `ATTACH_EMBEDDED_UNPARSED` when extract fails.
2. **Bounded depth + byte/count budgets.** Export depth owner is the writer’s `max_embedded_depth` (default **3**, clamp `[1, 8]`). Extract **must receive the same budget**. Hits are `ATTACH_DEPTH_LIMIT`, **not** generic unparsed. See §2.6.
3. Nested export is **subnode method=5**. Do **not** invent by-value `.msg` / PtypBinary `0x3701` payloads. **Do** write `PidTagAttachDataObject` PtypObject (`0x3701` / `0x000D`) pointing at the nested subnode. See §2.4.
4. **Parent identity stability:** parent `content_hash` and `strong_content_hash` must be **byte-identical** with nested extraction on vs off (filename/size stay source-derived; adding an in-memory nested DTO must not perturb the preimage). 0090 `embedded-msg-hash/v1` unchanged unless a bug is found. Regression test required.
5. **Anti-ghost:** do not write empty placeholder attaches for failed nested extracts.
6. **Method-5 only.** Method-1 / by-value `message/rfc822` stays a binary attach. Do not rewire it as nested `WriteMessage`.
7. **unique-eml ignores** any nested DTO this track (field may exist; EML pack path unchanged). Nested MIME `message/rfc822` in unique-eml stays a `D-0067-embedded-depth` residual.
8. **Lazy extract only for unique-pst winners** (and writer-fidelity fixtures that already hold a nested `WriteMessage`). Do not materialize ~241 MB of nests during scan of every parent.
9. Nested payload on a serde `CanonicalAttachment` must be **`#[serde(skip)]`** (CanonicalMessage is not Serialize; keep-set JSON must not balloon).
10. Fixtures in CI; INC* operator evidence in `review.md` only. No production `unwrap`/`expect`.

### 2.4 `PidTagAttachDataObject` (locked **in** this track)

Microsoft Learn [MS-PST] (accessed **2026-08-25**):

| Cite | Rule |
|---|---|
| §2.4.6.2.2 | If the attachment is itself a message, data is stored in **PidTagAttachDataObject**. The nid of the PtypObject structure (§2.3.3.5) is a subnode that is a fully formed message — **not in the NBT**, no parent folder. |
| §2.3.3.5 | A PtypObject in a PC stores `dwValueHnid` → an 8-byte heap allocation `{Nid, ulSize}` pointing at that subnode. |
| §2.5.2.4 | Embedded message objects; nested PC must include `PidTagMessageClass`. |

Today the writer places the nested message under the attach subnode leaf and writes **no `0x3701` property**. `pst-reader::resolve_embedded_from_attach_entry` finds it by scanning the attach subnode tree for `NID_TYPE_MESSAGE`. That is **lenient, not spec-conformant**. DoD-1 “reopen shows nested message” would pass against our reader while **Outlook has no documented discovery path**.

**Locked work (cheap, same machinery as `SubnodeString`):**

1. Add `PcValue::Object { nid, size }` writing PtypObject `0x000D` + 8-byte heap `{Nid, ulSize}`.
2. On method-5 attach PC, emit `0x3701` as **PtypObject**, never as non-empty PtypBinary.
3. Reader: **property-based resolve is primary**; keep subnode-scan as **fallback** for 0069-era output (no `0x3701` object).
4. DoD-1 fixture resolves **via the `0x3701` property**. Update test 12 so it still forbids PtypBinary payload and now **requires** PtypObject.
5. Close `D-0069-embed-object`. Optional operator Outlook smoke on INC* output is DoD-5 evidence, not a CI gate.

### 2.5 Nested extract is full export, not 0090 identity

`EmbeddedIdentityFields` is **not** enough for `WriteMessage`. Phase 1 is a **bids-based nested extract** (refactor main extract to accept `MessageNodeRef` / bids, or extend `embedded.rs`) including:

| Field | Nested export expectation |
|---|---|
| subject, sender, DisplayTo/Cc/(Bcc per 0082 flag) | **Preserved** when present |
| recipients (structured TC) | **Preserved** when table present; never invent from Display* |
| `message_class` | **Preserved** (MS-PST nested PC MUST); default `IPM.Note` only if source missing |
| `message_id` / submit time | **Preserved** when present |
| `body_plain` | **Preserved** under byte budget |
| `body_html` | **Preserved** under byte budget (this is the bulk of ~645 KB average nests). Identity path currently has **no HTML**. |
| `message_flags` | **BestEffort** — copy when readable; do not invent UNSENT |
| folder / `locus` | Nests have **no folder** (§2.4.6.2.2). Do not invent IPM paths. |
| child attaches | Recurse method-5 under depth; **stream** by-value children via `MessageNodeRef` (§2.8) |

Unreadable nested object → `embedded = None` + `ATTACH_EMBEDDED_UNPARSED` (lock 1). Partial child-attach failure → honesty flag on that child, never invent bytes.

### 2.6 Depth / byte budget owner (locked)

Three constants today; export must have **one owner**.

| Surface | Role |
|---|---|
| Writer `max_embedded_depth` (default 3, clamp 1–8) | **Export budget owner** — pass into materialize/extract |
| Reader `MAX_EMBEDDED_IDENTITY_DEPTH` | Identity (0090) — do not silently diverge; reuse 3 as the default |
| Engine `MAX_EMBEDDED_MSG_DEPTH` | Hash (0090) — unchanged |

**Misclassification trap:** if extract enforces its own depth/byte cap and returns `embedded: None` with no distinct reason, the writer counts **`ATTACH_EMBEDDED_UNPARSED`**, and DoD-3’s depth counter undercounts the case it exists to measure.

**Lock:** budget exhaustion at extract **must** surface as a distinct materialize-side signal mapped to `ATTACH_DEPTH_LIMIT` (writer `AttachmentFidelityKind::DepthLimit`). Generic unreadable stays `ATTACH_EMBEDDED_UNPARSED`.

Nested body/HTML + child-attach bytes charge existing per-attach / run caps. Additional **per-nest payload ceiling: 32 MiB** (agy); tighter existing caps still win. Well-formed PST subnode containment is a tree; depth + byte budgets suffice — do not add a cycle-walker for well-formed input (malformed cycles fail closed as unparsed or depth).

### 2.7 DTO / serde / lazy materialize (locked constraints; shape is Phase 0)

`CanonicalMessage` requires `content_hash`, `locus`, `fidelity`, `message_id_norm`, `edrm_mih_hex` — none exist naturally for nests.

Phase 0 picks **one**:

- **A.** Dedicated `NestedCanonicalMessage` (or equivalent) mapped to `WriteMessage` in the adapter, **or**
- **B.** Synthesize `CanonicalMessage` (nested `content_hash` via 0090 `embedded-msg-hash/v1` is the natural fit; `locus` parent-relative / attach-keyed, **not** a fake folder path).

Either way:

- Nested payload on `CanonicalAttachment` is `#[serde(skip)]` (agy compile constraint + keep-set size).
- unique-eml / GUI / CSV consumers must **ignore** the field this track (lock 7).
- Extract nested bodies **only for winners** headed to unique-pst (lock 8). Writer already drops `att.embedded_message = None` after write (`production.rs` ~1679) — keep that RAM relief.

### 2.8 Nested child-attach streaming (locked)

Child by-value attaches under a nest **cannot** use `open_attachment_data(parent_nid, attach_nid)` — that NBT-looks-up the message NID (agy). Nested NIDs are not in the NBT.

**Helper already exists:** `PstFile::open_attach_data_from_message_node(&MessageNodeRef, attach_nid)` (`embedded.rs`). Do **not** invent a second public API. Wire `PstAttachStreamSource` / materialize to pass a `MessageNodeRef` (or bids) for nested parents.

Prefer streaming child bytes into the writer’s `AttachStreamSource` (0069) over buffering whole nests.

### 2.9 Affected crates

| Path | Change |
|---|---|
| `pst-reader` | Full nested extract from `MessageNodeRef`; PtypObject primary resolve + scan fallback; reuse `open_attach_data_from_message_node` |
| `dedup-engine` | Nested DTO on `CanonicalAttachment` (`serde(skip)`); identity hashes unchanged |
| `pst-writer` | Map nested through `from_canonical_message*`; `PcValue::Object`; method-5 attach PC writes PtypObject `0x3701` |
| `pst-dedup-cli` | Materialize nested extract for winners; stream child attaches; depth reason mapping; unique-pst tests |
| docs | PtypObject discovery; unique-eml still honesty-only for nests |

### 2.10 Dual-AI review disposition (2026-08-25)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | `D-0069-embed-object` is the documented Outlook/MAPI discovery path; fold into 0094; DoD-1 must resolve via `0x3701` | opencode | **Agree — pull in.** Cheap (`PcValue::Object`); lock 3 updated | §2.4; DoD-1; closes D-0069-embed-object |
| O2 | Reader API is identity-shaped; Phase 1 is full nested extract (HTML, class, MID, flags) + nested fidelity contract | opencode | **Agree** | §2.5 |
| O3 | Three depth constants; extract budget fail currently becomes UNPARSED not DEPTH_LIMIT | opencode | **Agree** | §2.6; DoD-3 |
| O4 | Parent `content_hash` / `strong_content_hash` must be identical with extract on vs off | opencode | **Agree** | lock 4; DoD-1 regression |
| O5 | CanonicalMessage required fields; serde balloon; lazy-winners; EML ignores field | opencode | **Agree** (shape is Phase 0 A vs B) | §2.7; locks 7–9 |
| O6 | DoD-1 must reopen+resolve nested fields; test 12 is too weak | opencode | **Agree** | DoD-1 |
| O7 | Disambiguate rfc822-as-embedded (method-5 vs method-1); inventory `keep_set_summary.json` | opencode | **Agree** | lock 6; Phase 0 hygiene |
| A1 | `open_attachment_data` NBT-misses nested child attaches; need subnode stream helper | agy | **Agree hazard.** Helper **already exists** (`open_attach_data_from_message_node`) — **wire it**, do not add a twin API | §2.8 |
| A2 | `#[serde(skip)]` on nested field or CanonicalAttachment serde fails (`CanonicalMessage` is not Serialize) | agy | **Agree** | lock 9 |
| A3 | `MAX_EMBEDDED_DEPTH = 3` + 32 MiB nested body budget; depth → `ATTACH_DEPTH_LIMIT` | agy | **Agree** (writer clamp remains `[1,8]`; default 3). Cycle-walker not required for well-formed trees | §2.6 |
| A4 | unique-eml MIME `message/rfc822` nested parity | agy | **Decline this track** | lock 7; residual under D-0067 |
| A5 | Post-0094 INC* will be `embedded_messages_written: 374`, `attachments_failed: 0`, exit SUCCESS | agy | **Decline guarantee.** Large drop, not necessarily zero | §2.1; DoD-5 |

**Declined / not in this track**

- unique-eml nested RFC822 packaging (A4).
- Guaranteed zero unparsed on INC* (A5).
- Inventing by-value `.msg` bytes / PtypBinary `0x3701` (still forbidden).
- Relativity/matter child-document extraction.
- A second reader stream API when `open_attach_data_from_message_node` already works.

---

## 3. In scope

1. Full bounded nested extract for **method-5** into a nested DTO; map through `from_canonical_message*` into `WriteAttachment.embedded_message`.
2. `PcValue::Object` + method-5 attach PC `PidTagAttachDataObject`; reader property-based resolve + scan fallback.
3. Wire nested child-attach streaming via `open_attach_data_from_message_node`.
4. Depth/byte budgets with **correct reason codes**; parent-hash regression; winner-only lazy extract; `serde(skip)`.
5. Tests: nested write + **0x3701 resolve** + nested subject/sender/recipients/body; nest-with-child-binary stream; unparsed path; depth-limit path; identity-on-vs-off hash.
6. Close `D-0069-embed-object`. Close or narrow `D-0067-embedded-depth` (unique-eml nested MIME + matter child docs remain residual).

## 4. Out of scope

- Relativity child-document extraction / review corpus children (matter path).
- Cloud hydrate.
- unique-eml nested `message/rfc822` packaging (lock 7).
- Heap/recipient (`0093` Completed), folder-tree (`0095`), PermissionType (`0096`), body-cloud (`0097`).
- Method-1 by-value rfc822 rewired as nested `WriteMessage`.
- Cycle detection beyond fail-closed budgets.

## 5. Preconditions & dependencies

- **P1:** Writer nested builder + reader `MessageNodeRef` / `open_attach_data_from_message_node` exist.
- **P2:** 0093 Completed (heap diversion) so large parents with Display* still write.
- *Verified:* INC0102784 fails are exclusively method-5 unparsed — highest attach soft-fail ROI.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Outlook cannot find nest without PtypObject | §2.4 in scope; DoD-1 via `0x3701` |
| Deep nests / RAM | Depth + 32 MiB per-nest + existing caps; lazy winners only; stream child attaches |
| Materialize cost regresses unique-pst | Lock 8 — no scan-time nest extract |
| Extract budget counted as unparsed | Distinct `ATTACH_DEPTH_LIMIT` signal (§2.6) |
| Keep-set / parent hash churn | serde skip; lock 4 regression |
| Partial nested (missing child attaches) | Honesty flags; never invent |
| Serde compile break | `#[serde(skip)]` (CanonicalMessage is not Serialize) |
| `open_attachment_data` on nested NID | Use `open_attach_data_from_message_node` only |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 —** Method-5 fixture unique-pst (or writer fidelity) writes nested subnode **and** `PidTagAttachDataObject` (PtypObject `0x3701` / `0x000D`, no PtypBinary payload). Reopen **resolves via `0x3701`** (scan fallback still works). Nested subject/sender/recipients/body match source. `embedded_messages_written >= 1` and `embedded_unparsed == 0` on that fully-parseable fixture. Parent `content_hash` / `strong_content_hash` identical with extract on vs off.
- [ ] **DoD-2 —** Missing/unreadable nested still yields `ATTACH_EMBEDDED_UNPARSED` (no invent, no ghost attach). Fixture with a nested message that itself has a **by-value child attach** streams that child (not `NodeNotFound` on NBT).
- [ ] **DoD-3 —** Depth budget enforced at extract **and** write; exhaustion maps to `ATTACH_DEPTH_LIMIT` (not UNPARSED). Default 3.
- [ ] **DoD-4 —** `D-0069-embed-object` **closed**. `D-0067-embedded-depth` closed or narrowed with explicit residual (unique-eml nested MIME / matter child docs).
- [ ] **DoD-5 —** Operator note: re-smoke INC0102784 expect **large drop** in `ATTACH_EMBEDDED_UNPARSED` (not necessarily zero). Optional Outlook-open of a nested message is evidence, not CI.
- [ ] **DoD-6 — Recorded:** `review.md`; conductor **Completed**; ledger commit (`FEATURE`).

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p pst-reader -p dedup-engine -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings
cargo test -p pst-writer
cargo test -p pst-reader -- embedded
cargo test -p dedup-engine
cargo test -p pst-dedup-cli --test unique_pst
# operator: unique-pst INC0102784 pair; compare attachments_failed_by_reason vs prior 374 EMBEDDED_UNPARSED
```
