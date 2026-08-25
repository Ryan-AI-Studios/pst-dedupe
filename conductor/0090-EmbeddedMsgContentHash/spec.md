# 0090 — Embedded Message Content Hash

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0090-EmbeddedMsgContentHash
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M continuation
- **Cross-repo contract:** n/a
- **Status:** Ready — not started (review-folded 2026-08-24; **re-baselined**)
- **Depends on:** 0082 · 0086 (all **Completed**)
- **Spec authored:** 2026-08-24
- **Series:** M (Unique export fidelity residuals — continuation)
>
> **Review fold-in (2026-08-24):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.8. This spec **re-baselines** the original draft: method-5 is a subnode object, not a blob; naming is *not* Relativity parity.

---

## 1. Objective

Under `--strong-content-hash body-recip-attach`, compute a **documented, bounded, pst-dedup-specific embedded-aware identity hash** (`embedded-msg-hash/v1`) for nested email attachments so nested **header/body/recipient/child-attach** changes can split keep-set groups — instead of today’s **Choice B unread sentinel** on `ATTACH_EMBEDDED_MSG` (method 5, no binary) and raw-blob SHA-256 on by-value `message/rfc822` (method 1).

**This is not Relativity dedupe parity.** Relativity Server 2024 uses four *separate* component hashes, extracts embedded emails as **child documents**, and **does not apply parent-level dedupe to children**. Recursive hash-in-parent is a **pst-dedup product choice** (keep-set is parent-centric; matter family graph already models extraction elsewhere). Document that explicitly in operator docs.

**Closes:** `D-0086-embedded-email-hash` (may open a residual for unbounded depth / production nested extract — still `D-0067-embedded-depth`).

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0086-embedded-email-hash** | P3 | Deferred text said “raw attach-stream SHA-256 blob” — **inaccurate for method 5** |
| Live 0086 path | — | Method 5 typically **fails** `open_attachment_data` (`PropertyNotFound` / no binary) → **Choice B unread sentinel** (`attach_content_hash.rs`) |
| Operator risk | — | Nested content changes do not split groups; operators may assume Relativity AttachmentHash parity |

### 2.2 Live code snapshot (verified 2026-08-24)

| Surface | State |
|---|---|
| `IdentityLevel::BodyRecipAttach` | Live in `dedup-engine` grouping/hasher **preimage layout** |
| **Digest fill** | `crates/pst-dedup-cli/src/attach_content_hash.rs` + `scan.rs` (~802–863) — **not** engine hasher/grouping |
| `open_attachment_data` | Returns error for embedded messages without `PidTagAttachDataBinary` (`pst-reader` attachment.rs) |
| `ATTACH_EMBEDDED_MSG = 0x5` | MAPI: open as **subobject / subnode message**, not a data stream |
| Unread / cloud sentinels (Choice B) | Must remain fail-closed — never invent digests |
| `D-0067-embedded-depth` | Full recursive nested **production extract** — **still out**; this track is **bounded identity parse only** |

### 2.3 Product locks

1. **Opt-in only** — default identity stays `off` / v1; no silent keep-set churn.
2. **Version the preimage** (`embedded-msg-hash/v1`). Explicit docs: **not Relativity dedupe parity**.
3. **Hard depth + byte + count budgets** — fail closed to domain-separated sentinels (never OOM / never all-zero equality).
4. **Two cases (both in scope):**
   - **(a) Method 1 / `message/rfc822` by-value:** raw bytes exist; bounded rfc822 parse.
   - **(b) Method 5 `ATTACH_EMBEDDED_MSG`:** bounded `pst-reader` subnode message property load (subject/body/recipient TC/attach table) under budgets. **This is identity parse, not production nested extract.**
5. Identity/export honesty: keep-set / report must surface `embedded_message_unparsed` / depth-cap flags so 0080 sampling can see identity vs export drift.
6. Synthetic fixtures in CI; **operator-local** real embedded-msg PST smoke noted in `review.md` (not required for CI).

### 2.4 Preimage (normative — Phase 0 may only add length-prefixing, not drop fields)

```
embedded_component = SHA-256(
  b"pst-dedup/embedded-msg-hash/v1\0"
  || depth_u8                    // 0 = this embedded message (not the outer parent)
  || header_hash_32              // normalized subject | submit_time | sender (same rules as outer v2 header slot)
  || body_hash_32
  || recipients_hash_32          // SMTP+EX cascade when table present (0082)
  || attachments_hash_32         // see §2.5
)
```

**Why header is required:** body+recip alone collides two nested messages that differ only in subject/sender/time.

**Not Relativity:** Relativity hashes four components *separately* and does not fold nested email into the parent’s AttachmentHash this way.

### 2.5 Attachments_hash_32 + budgets (locked)

| Rule | Lock |
|---|---|
| Child attach **order** | Attachment **table index** order (stable, not filename sort) |
| Inline-ignored attaches | Honor `identity_ignore_inline_attachments` (0076) — same flag as outer; different flags ⇒ different identity (document) |
| Recursion | Child embedded msgs recurse with `depth+1` until cap |
| Depth cap | `MAX_EMBEDDED_DEPTH = 3` (align D-0067 honesty). At cap: **domain-separated sentinel** `SHA-256(b"pst-dedup/attach-depth-limit/v1\0" \|\| name \|\| size_u32_le)` — not raw blob, not panic |
| Count budget | Cap embedded parses per message **and** per run, consistent with `strong_hash_attach_max_attaches` |
| Byte budget | Existing per-attach / run byte caps apply to nested body reads |
| Unreadable / missing subnode | Choice B unread sentinel (`attach_unread_sentinel`) — never all-zero |

### 2.6 Reader capability (in scope, bounded)

`pst-reader` must expose a **budgeted** helper, e.g. `read_embedded_message_identity(parent_nid, attach_nid, depth) -> Result<EmbeddedIdentityFields>`, loading child Message PC + recipient table + attach table metadata. Fail closed on missing/corrupt subnode.

**Out:** full recursive materialize of nested messages as export artifacts (`D-0067-embedded-depth`).

### 2.7 Affected crates

| Path | Change |
|---|---|
| `pst-reader` | Bounded embedded-message identity load |
| `dedup-engine` | Preimage helpers / tests for `embedded-msg-hash/v1` |
| `pst-dedup-cli` | Wire in `attach_content_hash.rs` + `scan.rs` |
| docs | Not Relativity parity; flags for unparsed / depth-cap |

### 2.8 Dual-AI review disposition (2026-08-24)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | “Relativity-style” is a misattribution; no recursive parent hash | opencode | **Agree** | §1; docs lock |
| O2 | Method 5 is subnode, not bytes; today = unread sentinel | opencode | **Agree** | §2.1–2.2; two cases |
| O3 | Identity vs export divergence needs a DoD/doc hook | opencode | **Agree** | lock 5; DoD-4 |
| O4 | Preimage needs order, inline flag, depth_u8, count budget | opencode | **Agree** | §2.5 |
| O5 | Verification must include `pst-dedup-cli` | opencode | **Agree** | §8 |
| A1 | Add `header_hash_32` to preimage | agy | **Agree** | §2.4 |
| A2 | `pst-reader` subnode property loader | agy | **Agree (bounded identity)** | §2.6 — not full D-0067 extract |
| A3 | Depth-limit domain-separated sentinel | agy | **Agree** | §2.5 |
| A4 | rfc822 and method-5 share hash *semantics* | agy | **Agree (semantics)** | same preimage; different parsers |
| A5 | Relativity recursively computes four-component into parent AttachmentHash | agy | **Decline (fact)** | Relativity extracts children; O1 wins |

---

## 3. In scope

1. Bounded method-5 subnode identity load + method-1 rfc822 parse.
2. `embedded-msg-hash/v1` preimage including **header**.
3. Depth/byte/count budgets + depth-limit sentinel + unread fail-closed.
4. Golden tests: nested **subject** change splits; nested body change splits; depth cap no panic.
5. Docs: not Relativity parity; unparsed/depth-cap flags.
6. Close or narrow `D-0086-embedded-email-hash`.

## 4. Out of scope

- Full recursive nested MAPI **export** (`D-0067-embedded-depth`).
- Unifying L3 probe + digest I/O (`0091`).
- Changing default identity level.
- Matter `extract-pst` participant TC residual (`D-0018-03`).
- Claiming Relativity / other-tool hash parity.

## 5. Preconditions & dependencies

- **P1:** 0086 attach-content identity live; 0082 recipient identity when nested table present.
- *Verified:* digest fill is CLI `attach_content_hash`; method 5 → unread on open failure.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Keep-set splits surprise operators | Opt-in level only; versioned preimage; not-Relativity docs |
| Pathological nesting | Depth + byte + count; sentinel |
| Scope bleed into D-0067 extract | Identity fields only; no nested production write |
| Underestimate (reader + hasher + CLI) | Plan phases all three crates |

## 7. Definition of Done

- [ ] **DoD-1 — Behavior:** Under `body-recip-attach`, (a) method-1 rfc822 and (b) method-5 subnode embeds use `embedded-msg-hash/v1` (header+body+recip+child attaches), not unread-sentinel-only (b) or raw-blob-only (a), except documented unread/depth-cap paths.
- [ ] **DoD-2 — Budgets:** Depth/byte/count caps enforced; overflow → depth-limit or unread sentinel (no panic, no all-zero).
- [ ] **DoD-3 — Tests:** Nested **subject** change splits parent content hash; nested body change splits; depth-cap path covered. Tests exercise **CLI digest fill**, not only engine hasher units.
- [ ] **DoD-4 — Honesty:** Docs state **not Relativity parity**; unparsed / depth-cap flags available for QC. Operator-local real PST smoke **noted** in `review.md` (optional).
- [ ] **DoD-5 — Deferred:** Close or narrow `D-0086-embedded-email-hash`. Do not silently close `D-0067-embedded-depth`.
- [ ] **DoD-6 — Recorded:** `review.md`; conductor **Completed**; ledger TX committed.

## 8. Verification commands

```powershell
cargo test -p pst-reader -- embedded
cargo test -p dedup-engine -- hasher
cargo test -p pst-dedup-cli -- attach_content_hash
cargo test -p pst-dedup-cli -- unique
cargo fmt --all --check
cargo clippy -p pst-reader -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings
ledgerful verify
```
