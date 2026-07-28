# 0076 — Content-Hash Tier Hardening (Identity Binding)

- **Track ID:** 0076-ContentHashTierHardening
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record:** Series L — Unique export hardening (post-0072 / INC0102784 lessons)
- **Status:** **Completed** — Codex luna PASS WITH DEFERRED P3 (2026-07-28); D-0075-scope closed
- **Depends on:** Hard **0003 / 0065 / 0066** (tier semantics, integrity flags, keep-set grouping). Soft **0074** (attach probe — required only for the *attach-content* identity level §3.6), **0075** (`RankContext` precedent, `decided_by` vocabulary conventions).
- **Downstream:** **0079** (grouping cost is on the hot path), **0080** (QC samples per bind tier), **0081** (operator runbook: which identity level for which collection).
- **Priority:** **P0 within Series L** — this is the first Series L track that fixes a *reachable panic* and a *silent evidence-loss* class, not an explainability gap.
- **Evidence:** INC0102784 — mass-mail duplicates "looked correct" but same-subject size variance exists across mailboxes; ~108k page CRC warnings mean degraded bodies are routine in real inputs. Real PSTs stay operator-local; CI is synthetic only.
- **Deferred ledger:** append **D-0076-***. Never mutate source PSTs. Default changes may only **split** groups, never merge them (§2.3 rule 1).

---

## 1. Objective

Make dedup **identity binding** correct and honest. Today the Tier-2 key is a 4 KB body preview plus attachment *names and sizes*, with no recipients, no guard against binding two messages that carry **different** Message-IDs, and no guard against binding two messages whose bodies we **failed to read**. It also contains a reachable panic.

| Capability | P0 |
|---|---|
| **Hash-input safety** | Character-clamped truncation — removes a proven scan-killing panic *without* shrinking non-Latin comparison windows (§3.2) |
| **Never bind on unread content** | Body-unavailable / degenerate-preimage items stop deduping by Tier 2 (§3.3) |
| **Tier-1 authority** | Two items with *different* non-empty Message-IDs can no longer merge on content hash (§3.4) |
| **Honest bind provenance** | Tier recorded **at bind time**, replacing a post-hoc guess that could not fail (§3.5) |
| **Tier 2.5 strong identity** | Opt-in, **layered** — full body, then recipients, then attachment content; provably a *refinement* of Tier 2 (§3.6) |
| **Component attribution** | Every split and divergence is attributed to header / body / recipients / attachments (§3.6, §3.7) |
| **Tier-1 divergence signal** | Report MID-group divergence **split by component**, so body drift is not buried under timestamp drift; opt-in split (§3.7) |
| **Custodial scope** | Rolls in **D-0075-scope** — `--dedupe-scope per-source` vertical dedupe (§3.8) |
| **Split-only defaults** | Every default-on change provably refines pre-0076 groups (§3.11) |

**Outcome:** a message that was exported before 0076 is still exported after it; a message that was *silently suppressed by a weak key* is no longer suppressed; and the report can say which key actually bound each duplicate.

**Industry anchors (researched 2026-07-28):**

- **Relativity** (*Deduplication considerations*, RelativityOne processing, current): email dedup composes **four** SHA-256 component hashes — `MessageBodyHash` (PR_BODY as a Unicode string with CR/LF/space/tab removed), `HeaderHash` (`Subject<crlf>SenderName<crlf>SenderEMail<crlf>ClientSubmitTime`), `RecipientHash` (loops **all** recipients — *"Note that BCC is included"*), `AttachmentHash` (**a SHA-256 per attachment's content**). The Processing Duplicate Hash is a SHA-256 over all four. *"If two emails have an identical body, attachment, recipient, and header hash, they are duplicates."* Our Tier 2 matches the header component, whitespace-strips like the body component, and then **diverges on the two remaining components**: body is a 4 KB preview, attachments are name+size metadata, and recipients are absent entirely.
- **Relativity** also states plainly that *"the MD5, SHA256, SHA1 … are not considered in deduplication of email files"* — whole-item hashing is **not** the industry answer, so 0076 hardens the field-selected hash rather than switching to file digests.
- **Microsoft Purview** (*Deduplication in eDiscovery search results*, Learn, updated 2026-06-11): duplicates are determined by `InternetMessageId` + `ConversationTopic` + `BodyTagInfo`, and the page documents two false-duplicate cases: (a) a message a user **edits but never sends** keeps its original `InternetMessageId` — so a MID match is *not* proof of identical content; (b) **copy-on-write** items in `Recoverable Items\Versions` may be treated as duplicates of the revised item. (a) is the ground for §3.7; (b) is why 0075 shipped the folder-class ladder and the `winners_from_recoverable_items` signal.
- **Global vs custodial dedupe** (Lexbe / GoldFynch / Prosearch, 2025–2026): horizontal (global) dedupe is one defensible mode, **vertical (per-custodian)** is the other, and mature tools expose the choice. 0075 recorded this as out-of-scope **D-0075-scope**; 0076 owns grouping and takes it.

---

## 2. Context (ground truth)

### 2.1 What exists today (verified in tree 2026-07-28, commit `f996392`)

| Layer | State |
|---|---|
| Tier-2 preimage | `dedup_engine::hasher::compute_content_hash` = normalized subject `\|` submit FILETIME `\|` sender email `\|` ≤4 KB normalized body preview `\|` sorted `name:size` attachment pairs |
| Body source | `pst_reader::read_message_properties` decodes **the whole body**, then keeps `b.chars().take(4096)` — the full string is already in RAM and is dropped |
| Recipients | **Not in the hash at all.** `MessageProperties` has `display_to` and (since 0075) `display_bcc`; `PID_TAG_DISPLAY_CC` (0x0E03) is defined in `nid.rs` but never read here |
| Attachments | `read_attachment_metadata` → `AttachmentInfo { filename, size }`. No content digest |
| Streaming index | `dedup_engine::index::DedupIndex::check_and_insert` — Tier 1 then Tier 2, first-seen wins |
| Keep-set grouping | `dedup_engine::keepset::group_candidates(items, tier2_enabled)` — same rules, all members collected |
| Bind tier reporting | `keepset::member_tier(...)` reconstructs the tier **after the fact** and ends in `// Fallback … (should be rare)` returning `ContentHash` unconditionally |
| Degraded bodies | `body_incomplete` / `body_unavailable` set `body_preview = None`; the hash is still computed and still binds |
| Scan item | `RecoverableScanItem { locus, message_id_norm, content_hash, size, integrity, scan_order, submit_time, delivery_time, has_bcc }` (Serde, `#[serde(default)]` pattern from 0075) |
| Surfaces | `--no-tier2` on `scan` / `dups` / `keep-set` / `unique-eml` / `unique-pst`; `stats.tier1_dups` / `tier2_dups`; decision CSV `tier` ∈ {`message_id`, `content_hash`} |

### 2.2 Defects this track exists to fix

| # | Defect | Evidence | Direction |
|---|---|---|---|
| **D1** | **Reachable panic.** `hasher.rs:97` tests `normalized.len() > 4096` (bytes) then slices `&normalized[..4096]` (bytes). `body_preview` is capped at 4096 **chars**, so any non-ASCII body overflows that byte budget and slices mid-character. | Reproduced 2026-07-28 with `rustc 1.97.1`: a 4096-char CJK body normalizes to 12288 bytes, `is_char_boundary(4096) == false`, and the slice panics — *"end byte index 4096 is not a char boundary; it is inside 'あ'"*. No `catch_unwind` on the CLI scan or GUI worker path. | crash |
| **D2** | **Binding on content we never read.** When the body read fails, `body_preview` is `None` and the preimage silently contributes nothing. Two unrelated messages with unreadable bodies and a shared/empty subject+sender+time collide. All-empty inputs hash to one constant. | ~108k CRC warnings on the INC run; `RecoverableIntegrity` already carries `body_incomplete` / `body_unavailable` and the hasher ignores both. | false merge |
| **D3** | **Cross-MID merge.** `group_candidates` and `DedupIndex` fall through to the content hash whenever the MID lookup misses — including when the item **has** a MID that simply differs from the group's. Mail-merge sends (distinct MIDs, identical subject/sender/time/preview) collapse into one winner. | `keepset.rs:698` / `index.rs:154`; the existing test `test_cross_tier_no_false_positive` documents the behavior as *"ACCEPTABLE"*. INC mass-mail duplicates "looked correct" — this is exactly where they would not be. | false merge |
| **D4** | **Weak Tier-2 key vs. industry.** No recipients; attachments by `name:size` only; body by first 4 KB. Long quoted threads, newsletters and template attachments differ *after* the preview window. | Relativity's four-component hash (§1) includes full body, all recipients incl. BCC, and per-attachment content SHA-256. | false merge |
| **D5** | **Bind tier is guessed, not recorded.** `member_tier` compares the member against the group *seed* after grouping, and its final arm returns `content_hash` for anything it cannot explain. | `keepset.rs:779` `// Fallback: treat as content_hash when in same group (should be rare).` | dishonest report |
| **D6** | **Missed merge (registration gap).** A MID is registered only when its item *creates* a group. An item that joins by content hash never registers its MID, so a later item sharing that MID starts a new group. | `keepset.rs:709` (insert inside the `else` arm only); same shape in `index.rs:166`. | false split |

D1–D5 all move in the "we suppressed something we should have kept" direction except D5 (reporting). D6 moves the other way and is therefore **opt-in** (§3.9).

### 2.3 Product rules (LOCKED)

1. **Split-only defaults.** Any change that ships **on by default** must be *monotone*: for every input, the 0076 grouping is a **refinement** of the pre-0076 grouping (every 0076 group is a subset of some pre-0076 group). A message exported before 0076 is still exported after it. Anything that would **merge more** ships behind a flag, default off. Proven by a fixture-wide refinement assertion (§3.11).
2. **Never dedupe on a field we failed to read.** Absent is not empty. A body we could not decode must not be hashed as "no body" and then treated as equal to another body we could not decode. (0075's "never invent a date", applied to content.)
3. **Tier 1 stays authoritative and unchanged.** Message-ID normalization is frozen. A MID match still binds by default; a MID *conflict* now blocks a Tier-2 merge.
4. **Tier 2.5 may only subdivide.** The v2 preimage is the v1 preimage **plus** additional fields, so equal-v2 ⇒ equal-v1 by construction. Enabling it can never create a group that Tier 2 did not already contain.
5. **`content_hash` v1 stays the default identity** and its hex value stays stable in existing reports — with **one named, bounded exception**: items whose normalized body preview exceeds 4096 *bytes* rehash under the §3.2 char clamp. That population is counted (`tier2_preview_bytes_over_budget`), the change is split-only, and no other input's hex moves. Switching the default identity to v2 is a separate product decision (**D-0076-default-v2**).
6. **CSV columns append-only; JSON additive; `keep_set_v1` schema id retained** (0075 rules 7–8 carry forward). New *values* in the closed `tier` vocabulary are documented before use.
7. **One set of grouping semantics.** `DedupIndex` (streaming) and `group_candidates` (keep-set) must agree under every option combination, proven by a shared equivalence test — not by a third reconciliation path.
8. **Source PSTs read-only.** No repair, no re-read of a file to "confirm" a hash.
9. **No new I/O on the default path.** The full-body digest is computed from bytes already in memory; attachment-content digests require an explicit level (§3.6) and reuse 0074's budgeted probe.

### 2.4 Deferred roll-in

| Item | Action in 0076 |
|---|---|
| **D-0075-scope** — custodial / vertical dedupe (`--dedupe-scope per-source`) | **Rolled in** (§3.8). It is a grouping change, and 0076 owns grouping. Split-only by construction. |
| **D-0065-soft-body** — soft partial body recovery | **Not closed**, but its risk is contained: §3.3 stops a partially-read body from binding at all, which is the harm the residual describes. Cross-referenced. |
| **D-0073-eml** / **D-0074-\*** | **Out** — unrelated surfaces. |
| **D-0066-disk-groups** — disk-backed group store at multi-million scale | **Out** → 0079. 0076 adds ≤ 40 bytes/item to the in-RAM candidate (§3.12) and must not regress the ceiling further. |
| Placeholder §3.2 "cheap size/count gate before Tier-2 equality" | **Declined as specified**, redirected (§3.9). |

---

## 3. Design

### 3.1 `GroupingContext` (no behavior change)

Mirror 0075's `RankContext` refactor so reviewers see the same shape:

```rust
pub struct GroupingContext {
    pub tier2_enabled: bool,        // existing --no-tier2
    pub scope: DedupeScope,         // §3.8  Global (default) | PerSource
    pub tier1_authority: bool,      // §3.4  true by default (split-only)
    pub require_readable_body: bool,// §3.3  true by default (split-only)
    pub identity: IdentityLevel,    // §3.6  V1 (default) | V2Body | V2BodyAttach
    pub tier1_verify: Tier1Verify,  // §3.7  Off (default) | Content
}
```

`group_candidates(items, ctx)` and `DedupIndex::with_context(ctx)` replace the bare `tier2_enabled` bool at every call site (`scan.rs`, `keep_set_cmd.rs`, `unique_pst_cmd.rs`, `attach_probe.rs`, `rebuild_dedup_results`). `GroupingContext::default()` is exactly pre-0076 semantics **except** the two split-only guards, which are introduced in their own phases so the golden diff is attributable.

### 3.2 P0 — hash-input safety (D1)

**Clamp by characters, not bytes.** Replace `&normalized[..4096]` with `normalized.chars().take(4096)`.

A byte-boundary clamp (`floor_char_boundary`) also stops the panic, and it would have been hash-preserving — but it hands an English email a 4096-character comparison window and a Japanese email a ~1365-character one. Where a long boilerplate disclaimer or a quoted header block sits at the top of the body, a 1365-character window may never reach the substantive text, so CJK mail would false-merge at exactly the rate this track exists to reduce. Equal treatment across scripts wins over hash stability.

- The upstream cap is already **characters**: `read_message_properties` keeps `b.chars().take(4096)`, so for every current caller the char clamp is a no-op and the hashed bytes are the whole preview. The clamp stays in `compute_content_hash` rather than being deleted because that function is **public API of `dedup-engine`** and the 4 KB bound is its documented invariant — deleting it would leave a future caller free to hash an unbounded body.
- **Honest consequence — a named, bounded exception to rule 5.** For inputs where `normalized.len() > 4096` bytes *and* byte 4096 happens to fall on a char boundary, today's code does **not** panic; it hashes a byte-truncated prefix. Those hashes change. In practice that is 2-byte-script mail (Cyrillic, Greek, Hebrew, Arabic) with more than ~2048 characters of body; 3-byte CJK almost always panics instead. **The change is split-only:** the new preimage is a strict extension of the old prefix, so two items that hashed differently before still differ, and only items that agreed on the truncated prefix while differing later can now separate. That is the direction rule 1 licenses.
- `stats.tier2_preview_bytes_over_budget` counts items whose normalized preview exceeded 4096 bytes — i.e. exactly the population whose v1 hex is not reproducible from a pre-0076 run. Reported in JSON and the human summary so a re-run mismatch is explainable rather than mysterious.
- Same defect class in `normalize_subject`: `s.trim()[3..]` / `[4..]` slice by byte after a `to_lowercase()` prefix test. Guard the slice on `is_char_boundary` (or match the ASCII prefix on the original string) so a pathological subject cannot panic.
- **Dead GUI setting:** `pst_dedup_gui::app::Config::body_hash_len` is exposed in the settings pane as "Body preview length for Tier 2 hash (bytes)" with a KB slider, and **nothing reads it** — `worker.rs` passes `props.body_preview` straight through. Either wire it (as a *character* budget, threaded into the reader) or delete the setting; a control that claims to change the dedup key and does not is an operator-facing lie. Default choice: **delete** — the preview budget belongs to the reader, not to a GUI slider.
- **Tests:** the exact reproduced input `"\u{3042}".repeat(4096)` hashes without panicking and covers all 4096 characters; a pure-ASCII 5000-char body hashes to a checked-in pre-fix digest (unchanged); a 4096-char Cyrillic body is asserted to change *and* to change split-only; pathological subjects (multibyte before and after the prefix test) do not panic.

### 3.3 P0 — never bind on unread or degenerate content (D2)

An item is **Tier-2 ineligible** (it may still bind by Tier 1) when either:

1. its integrity carries `body_incomplete` or `body_unavailable` — we know a body existed and we failed to read it; or
2. its preimage is **degenerate**: no body preview **and** fewer than two non-empty values among {normalized subject, submit FILETIME, sender, ≥1 attachment}.

Ineligible items are simply never inserted into, or matched against, the content-hash map. They still get a `content_hash` value in the CSV/JSON (column meaning unchanged) — they just stop *binding* on it. Split-only, therefore default on.

- `stats.tier2_blocked_unreadable_body` and `stats.tier2_blocked_degenerate` are reported in JSON **and** in the human summary of `scan`, `keep-set`, and `unique-pst` (0075 fix-round precedent: honesty stats must reach the human path, not just JSON).
- Escape hatch `--allow-degenerate-tier2` restores pre-0076 binding exactly, for an operator reproducing an older run.
- Honest limit to document: rule 2 is about *reads we know failed*. A body that decoded cleanly but is genuinely empty is not degraded and still binds.

### 3.4 P0 — Tier-1 authority: no cross-MID merges (D3)

Each group carries an optional **bound MID**. An item may join a group by content hash only when the item's MID and the group's bound MID are **compatible**:

| Group bound MID | Item MID | Content hash equal | Join? |
|---|---|---|---|
| none | none | yes | **yes** |
| none | `m1` | yes | **yes**, and the group's MID becomes `m1` (see §3.9) |
| `m1` | none | yes | **yes** |
| `m1` | `m1` | — | **yes** (Tier 1) |
| `m1` | `m2` | yes | **no** — new group; `stats.cross_mid_blocked += 1` |

Empty-string MIDs count as absent throughout (existing `is_empty()` convention). Split-only, therefore default on; `--allow-cross-mid-tier2` restores the old behavior.

This is the contrapositive of the repo's own core mandate ("Message-ID matches are definitive"): two RFC 5322 Message-IDs that differ were minted by different sends, so a body-preview collision between them is a *collision*, not an identity.

**Cost, stated plainly: this raises unique counts most on bulk mail.** Newsletters, HR templates and automated mailers are dispatched per recipient, so every custodian's copy carries a distinct Message-ID. Under the guard those collapse-to-one groups explode back into one item per recipient, and an operator who was relying on Tier 2 to cull newsletters will see the review population jump.

The default stays **on** anyway, for a reason the operator can defend: in that same scenario the *recipient lists also differ*, so Relativity's four-component hash would **not** call those copies duplicates either (its `RecipientHash` diverges). Today's behavior — merging them on a subject/sender/time/preview collision while discarding who received what — is the outlier, not the guard. Aggressive newsletter culling remains one flag away (`--allow-cross-mid-tier2`), and §3.13 documents that trade explicitly.

Measurement instead of a taxonomy: `stats.cross_mid_blocked` (items), `cross_mid_blocked_groups` (distinct content hashes affected) and `cross_mid_blocked_max_group` (largest MID-distinct cluster sharing one content hash). A `max_group` of 4,000 tells the operator "you have a 4,000-copy blast" without this track building a Template/Newsletter classifier → **D-0076-bulk-class**.

### 3.5 P0 — bind provenance recorded, not guessed (D5)

`group_candidates` returns, per item, the tier that actually bound it:

```rust
pub enum BoundBy { Seed, MessageId, ContentHash, StrongContentHash }
```

`member_tier` — including its `// should be rare` fallback — is **deleted**, not patched. The decision CSV `tier` column keeps its existing values and gains one documented value, `content_hash_strong`, used only at identity level ≥ V2. A new append-only column `identity_version` (`v1` | `v2`) records which preimage produced the row's `content_hash`. `DedupIndex` returns the same enum so both paths report identically (rule 7).

### 3.6 Tier 2.5 — strong content identity (D4), opt-in

`--strong-content-hash <off|body|body-recip|body-recip-attach>` (default `off`; bare flag = `body`).

The levels are **ordered by how much store-to-store variance each added component carries**, so an operator can take the reliable strictness without the unreliable kind. Body text is stable between copies; recipient *display strings* are not (§ below); attachment content is stable but expensive.

**v2 preimage = v1 preimage, unchanged, followed by (per level):**

| Level | Adds | Source | Cost |
|---|---|---|---|
| `body` | `body_sha256`, `body_char_len` | SHA-256 over the **full** normalized body | **zero extra I/O** — `read_message_properties` already decodes the whole body before truncating (§2.1); only hashing cycles are new |
| `body-recip` | normalized `display_to` / `display_cc` / `display_bcc` | `PID_TAG_DISPLAY_TO` / `0x0E03` / `0x0E02` (trim, lowercase, split `;`, sort, rejoin) | one `get_string` each on the **already-loaded** PC — the 0075 pattern; soft `None` on decode error |
| `body-recip-attach` | per-attachment content SHA-256 | reuses 0074's budgeted `attach_probe` stream reads | expensive; explicitly gated |

**Componentized digests (attribution, never binding).** The preimage is assembled from four named components — `header` (subject/time/sender), `body`, `recipients`, `attachments` — mirroring Relativity's four-component model. Alongside the single binding hash, each candidate carries a **truncated 64-bit fingerprint per component** (32 B/item total). Those fingerprints are used only to *attribute* a split or a divergence to a component; they never bind, so their collision rate is irrelevant to correctness. This is what makes §3.7's `divergent_body` vs `divergent_metadata` and the recipient counters below exact rather than guessed.

**Properties:**

- **Refinement (rule 4):** v2 fields ⊇ v1 fields and the v1 fields are byte-identical in the preimage, so equal-v2 ⇒ equal-v1. Enabling any level can only split existing Tier-2 groups. Asserted over generated field tuples, not just examples.
- **Recipient fields are display strings, not addresses — and they vary between copies of the same message.** `PidTagDisplayTo`/`Cc`/`Bcc` hold resolved *display names*, so the sender's copy and the recipient's copy of one message can carry `"Smith, John"` vs `"John Smith"` vs a raw `/O=EXCHANGELABS/OU=…` X.500 string depending on which address book resolved them and which client wrote the item. A raw string comparison therefore **fails to match copies that are genuinely identical**. This is why recipients are their own level rather than being folded into `body`, and why the honest long-term fix is reading the **recipient table** (per-recipient SMTP address + `PidTagRecipientType`) instead of the display strings → **D-0076-recipient-table**. An X.500→SMTP fallback is *not* implementable at this layer: there is no address in these properties to map, only a display string.
- Recipient honesty signals, all reported whether or not the level is enabled above `body`: `stats.tier2_5_splits_recipients_only` (splits attributable solely to the recipient component, via the fingerprints above) and `stats.x500_recipient_items` (items whose recipient string contains an `/O=`-prefixed segment — the cheap detector for the failure mode). A large `tier2_5_splits_recipients_only` next to a small `body` split count is the operator's signal to drop back to `--strong-content-hash body`.
- Recipients follow Relativity in including **BCC**. Documented consequence, and the direct sequel to 0075: a sender's copy carries BCC where the recipient's copy does not, so at level ≥ `body-recip` the two copies **stop being Tier-2 duplicates**. When they share a MID they stay Tier-1 bound and 0075's `--prefer-bcc-copy` rung still decides between them; when the MID is missing they split and both are exported. `stats.tier2_5_splits` and `tier2_5_splits_bcc_only` make that visible rather than surprising.
- **Inline / embedded attachments** (signature logos, `image001.png`) are a known false-split source: one copy of a message carries the rendered logo as an attachment and another, written by a different client, does not, so any attachment-parity comparison separates two substantively identical emails. `--identity-ignore-inline-attachments` excludes them from the attachment component. Detection uses the **MAPI signal, not a size threshold**: `PidTagAttachContentId` (0x3712) present, or `PidTagAttachFlags` (0x3714) `attRenderedInBody`, or `PidTagAttachmentHidden`. `list_attachments` already loads each attachment's PC (it surfaces `mime_tag` / `attach_method` that `read_attachment_metadata` currently discards), so these are zero-extra-I/O reads. The flag is **merge-increasing**, therefore opt-in under rule 1, and `stats.inline_attachments_ignored` reports what it dropped. If the reader work exceeds this track's surface, defer to **D-0076-inline-attach** and ship the counter alone.
- Level `body-recip-attach` **skips, never fails**, on any attachment 0074's probe could not read: the item falls back to the next level down and is counted in `stats.strong_hash_attach_unread`. Fabricating a digest for an unread attachment would violate rule 2.
- If reusing `attach_probe` needs surgery beyond this track's surface, ship `off|body|body-recip` and record **D-0076-attach-content**.

### 3.7 Tier-1 divergence: always report, optionally split

Purview documents that an edited-but-unsent copy keeps its `InternetMessageId`, so a MID group can legitimately contain **different content**. 0076 does not change the default (rule 3), but it stops being silent:

- **Always** (any level): count MID groups whose members' content components are not all equal, **attributed by component** via §3.6's fingerprints:

  | Stat | Meaning | Actionability |
  |---|---|---|
  | `tier1_divergent_body` | body text itself differs inside a MID group | **high** — a real content difference under one Message-ID (the Purview edited-but-unsent case) |
  | `tier1_divergent_metadata` | only header / attachment-metadata components differ | **low** — a one-second submit-time drift or a plugin-renamed attachment |
  | `tier1_divergent_recipients` | only the recipient component differs | **low** — usually the display-string variance of §3.6 |

  An undifferentiated counter would be worthless in exactly the case it matters: a run reporting "10,000 divergent" that turns out to be attachment-size drift teaches operators to ignore the warning, and the one group with a genuinely edited body is lost in it. The human-summary hint fires on `tier1_divergent_body` only; the other two are reported but not escalated. Signal only — winners must not move. (Same shape as 0075's `winners_from_recoverable_items`.)
- **Opt-in** `--tier1-verify <off|content|body>`: subdivide a MID group by the full content hash (`content`) or by the **body component alone** (`body`). `body` is the defensible middle setting — it splits genuinely different text while ignoring the metadata drift above. Split-only; off by default because a legitimate duplicate can still differ in body encoding across stores.

### 3.8 Dedupe scope — rolls in D-0075-scope

`--dedupe-scope <global|per-source>`, default `global` (unchanged).

`per-source` partitions **both** key maps by `locus.source_path` (its `path_compare_key`, so Windows case folding matches everything else), so identical messages held by two custodians each survive as winners — vertical/custodial dedupe. Refines `global` by construction ⇒ split-only, but **not** default because global is the current and more common deliverable.

- Interacts cleanly with 0075: per-source runs make the "All Custodians" aggregate degenerate (each winner lists one source), which is correct and must be documented rather than special-cased.
- `scope` is echoed in `keep_set_v1` JSON and the report pack so a run can be reproduced.
- Out of scope: custodian *grouping* across multiple PSTs belonging to one custodian (a custodian map) → **D-0076-custodian-map**.

### 3.9 Recorded decisions

| Raised | Decision |
|---|---|
| Placeholder §3.2: "body length + attach count/size must match before Tier-2 equality (cheap filter)" | **Declined as specified.** A gate on `PidTagMessageSize` would split true duplicates: the INC evidence is explicitly *"same-subject size variance exists across mailboxes"* — store overhead differs per PST for the same message, so message size is not an identity input. **Redirected:** body char length and attachment count/sizes become *preimage fields of v2* (§3.6), where a difference splits only under an explicit flag. Pre-filtering to avoid computing the strong hash is also unnecessary, because the strong hash costs no I/O (§3.6). |
| D6 missed merge (MID never registered when an item joins by hash) | **Fixed only in the merge-safe direction.** Registering the joining item's MID on a group that has none (§3.4 row 2) is split-only and ships default-on. Retroactively *merging* two existing groups that turn out to share a MID would violate rule 1, so it ships as `--tier1-backfill` (default off) with `stats.tier1_backfill_candidates` always reported. |
| Fix the panic with a **byte**-boundary clamp (`floor_char_boundary`) | **Superseded** (§3.2). It preserves hashes but gives CJK mail a ~1365-character comparison window against ASCII's 4096, which manufactures false merges in exactly the languages the panic already punished. Char clamp adopted instead; the resulting hash change is bounded, named, and split-only. |
| X.500→SMTP fallback for recipient normalization | **Declined as specified, risk accepted and measured** (§3.6). `PidTagDisplayTo`/`Cc`/`Bcc` carry *display names*, not addresses — there is no SMTP address in the property to fall back to. Instead: recipients get their own opt-in level, splits are attributed to the recipient component, `x500_recipient_items` counts the failure mode, and the real fix (recipient table) is named **D-0076-recipient-table**. |
| Exclude attachments under a size threshold (e.g. <4 KB images) from the hash | **Problem accepted, heuristic declined** (§3.6). A byte threshold silently drops small responsive images and would differ per collection. The MAPI signal is exact and free on the already-loaded attachment PC: `PidTagAttachContentId` / `attRenderedInBody` / `PidTagAttachmentHidden`. Shipped as `--identity-ignore-inline-attachments`, **opt-in** because it is merge-increasing (rule 1). Note this is *not* DeNISTing — NSRL hash-list culling is 0024's `cull` job and operates on what gets reviewed, not on identity. |
| Turn the cross-MID guard off by default so bulk mail keeps collapsing | **Declined** (§3.4). The scenario that motivates it is one where recipient lists also differ, so the industry reference would not merge those copies either. Documented, measured, and one flag from reversible. |
| Switch the default identity to v2 | **Out.** It would change `content_hash_hex` in every existing report and every downstream join key. → **D-0076-default-v2**. |
| Replace SHA-256 with BLAKE3 for the body digest | **Out.** `sha2 0.11` is already the workspace pin, Relativity's components are SHA-256, and hashing is not the measured bottleneck (0079 owns performance). No new dependency for this track. |

### 3.10 Surfaces

**Flags** — identical names and help text on `scan`, `dups`, `keep-set`, `unique-eml`, `unique-pst`:

| Flag | Default | Direction |
|---|---|---|
| `--strong-content-hash <off\|body\|body-recip\|body-recip-attach>` | `off` | split |
| `--dedupe-scope <global\|per-source>` | `global` | split |
| `--tier1-verify <off\|content\|body>` | `off` | split |
| `--tier1-backfill` | off | **merge** |
| `--identity-ignore-inline-attachments` | off | **merge** |
| `--allow-cross-mid-tier2` | off | **merge** (restores pre-0076) |
| `--allow-degenerate-tier2` | off | **merge** (restores pre-0076) |

Both duplicated policy/flag parsers (`main.rs` and `unique_pst_cmd.rs`) must be updated together — 0075's fix rounds show this is the standard miss.

**Decision CSV** (append-only): `identity_version`, `bound_by`, `tier2_eligible`.
**`keep_set_v1` JSON** (additive): `identity_level`, `dedupe_scope`, and the stats below.
**Stats** (JSON *and* human summary):

| Group | Stats |
|---|---|
| Guards | `tier2_blocked_unreadable_body`, `tier2_blocked_degenerate`, `cross_mid_blocked`, `cross_mid_blocked_groups`, `cross_mid_blocked_max_group` |
| Tier-1 divergence | `tier1_divergent_body`, `tier1_divergent_metadata`, `tier1_divergent_recipients`, `tier1_backfill_candidates` |
| Tier 2.5 | `tier2_5_splits`, `tier2_5_splits_bcc_only`, `tier2_5_splits_recipients_only`, `x500_recipient_items`, `inline_attachments_ignored`, `strong_hash_attach_unread` |
| Hash safety | `tier2_preview_bytes_over_budget` |

**Desk:** the wizard gains a single "Strong content hash" checkbox mapped to `body`; ordered/enum surfaces stay CLI-only (consistent with **D-0075-gui**) → **D-0076-gui** for the rest.

### 3.11 Compatibility, determinism, equivalence

- **Refinement assertion (the load-bearing test):** for `fixtures/aspose_outlook.pst`, `promotions_spam.pst` and a synthetic multi-source set, compute pre-0076 groups (checked-in baseline captured in Phase 0) and 0076 groups under defaults; assert **every 0076 group is a subset of some baseline group**. Repeat for each split-only flag and for every pair of them.
- **Winner golden:** 0075's checked-in ASPOSE winner golden must hold under defaults, *or* every difference must be attributable to a non-zero split-only stat and be re-baselined in the same commit with the reason recorded in `review.md`. A silent golden diff is a bug.
- **Index/grouping equivalence (rule 7):** for shuffled inputs under every `GroupingContext` combination, the set of `DedupIndex` first-seen uniques equals the set of `group_candidates` seeds, and the reported `BoundBy` values agree.
- **Determinism:** grouping output is independent of `HashMap` iteration order — groups are keyed by first-appearance in scan order, and the tests assert stability across shuffles of equal-key items.
- **Round-trip:** a pre-0076 `keep_set_v1` JSON still deserializes (new fields `#[serde(default)]`, 0075 pattern).

### 3.12 Performance budget

- Default path: no new I/O and no new hashing. Target **≤ +2%** wall time on `scan` over the fixture set; the placeholder's ±10% is the hard ceiling.
- `--strong-content-hash body`: one extra SHA-256 pass over body bytes already resident, plus three `get_string` calls on an already-loaded PC. Measured on fixtures and recorded in `review.md`.
- `--strong-content-hash body-attach`: charged against 0074's existing probe budget; documented as the expensive level, not benchmarked as a default.
- RAM: the candidate item grows by `Option<[u8;32]>`, four `u64` component fingerprints and two small ints — **~80 B/item**. At 1 M candidates that is ~80 MB. Component fingerprints are truncated to 64 bits precisely to keep this bounded; storing four full digests would have cost ~160 B/item. Noted against **D-0066-disk-groups**, not solved here.
- Fixture-scale timings are **not** proof at multi-GB scale → operator residual **D-0076-operator-perf**.

### 3.13 Docs

`docs/unique-pst-export.md` gains an "Identity and binding" section:

- The tier table: Tier 1 MID → Tier 2 v1 → Tier 2.5 levels, with what each one actually compares.
- **Named divergence from Relativity's four components** (body preview vs full body; name+size vs attachment content; recipients absent at v1), so an operator can answer the question opposing counsel will ask.
- **Bulk-mail warning (§3.4):** blocking cross-MID merges inflates unique counts most for newsletters, HR templates and automated mailers, because each dispatch carries its own Message-ID. Read `cross_mid_blocked_max_group` first; use `--allow-cross-mid-tier2` when the goal is aggressive bulk culling and the recipient-level evidence is not at issue.
- **Recipient warning (§3.6):** `display_to`/`cc`/`bcc` are *display names*, not addresses, and vary between copies of one message (`"Smith, John"` vs `"John Smith"` vs `/O=EXCHANGELABS/…`). Enable `body-recip` only when recipient differences matter; check `tier2_5_splits_recipients_only` and `x500_recipient_items` before trusting the result.
- **Inline attachments (§3.6):** signature logos cause attachment-parity false splits; what `--identity-ignore-inline-attachments` does and why it is off by default.
- When to use `per-source` (custodial deliverable) vs `global`.
- The BCC consequence at level ≥ `body-recip` (§3.6) and its interaction with 0075's `--prefer-bcc-copy`.
- The Purview edited-but-unsent / copy-on-write cases as the reason the `tier1_divergent_*` stats exist, and why they are split by component.
- **Reproducibility note (§3.2):** for non-Latin bodies over ~2048 characters, `content_hash_hex` from a pre-0076 run will not reproduce; `tier2_preview_bytes_over_budget` names that population.
- Closed vocabularies (`bound_by`, `identity_version`, `dedupe_scope`) for downstream parsers.
- Cross-link 0080 (QC sampling per bind tier) and 0081 (runbook).

### 3.14 Tests (minimum)

1. CJK 4096-char body hashes without panic **and covers all 4096 characters** (a 4096-char CJK body and one differing only at character 3000 must hash differently — the test that a byte clamp would fail); ASCII long-body digest byte-identical to a checked-in pre-fix value.
1b. 4096-char Cyrillic body: hash differs from the pre-0076 byte-clamped value, the difference is split-only (a pair differing only after character 2048 separates; a pair differing before it stays separated), and `tier2_preview_bytes_over_budget` counts it.
2. Pathological subject (`"Re:"` + multibyte tail, and a multibyte char before the prefix test) does not panic.
3. `body_unavailable` / `body_incomplete` items do not Tier-2 bind; they still Tier-1 bind; stats increment.
4. Degenerate preimage (no body, one weak field) stays unique; a body-bearing pair still merges.
5. `--allow-degenerate-tier2` reproduces pre-0076 grouping exactly on the same input.
6. Cross-MID: `{m1,h}` + `{m2,h}` → two groups, `cross_mid_blocked == 1`; with `--allow-cross-mid-tier2` → one group.
7. MID-none group adopts a joining item's MID; a third item with that MID joins (D6 partial fix) — and the *backfill* case stays split until `--tier1-backfill`.
8. v2 refinement: over ≥1000 generated field tuples, `v2_equal ⇒ v1_equal`; and a pair identical in the first 4 KB but divergent after it splits at level `body` and not at `off`.
9. Recipient normalization: `"A@x.com; b@X.com"` vs `"b@x.com;a@x.com"` hash equal; sender-copy-with-BCC vs recipient-copy splits at level `body-recip` **but not at `body`**, and remains Tier-1 bound when both carry the same MID.
9b. Recipient variance is attributed, not hidden: `"Smith, John"` vs `"John Smith"` vs `/O=EXCHANGELABS/OU=…` for the same message splits only at `body-recip`, increments `tier2_5_splits_recipients_only`, and the X.500 form increments `x500_recipient_items`.
9c. Inline attachments: two copies differing only by a `PidTagAttachContentId`-bearing `image001.png` split by default and merge under `--identity-ignore-inline-attachments`, with `inline_attachments_ignored` counting; a *non*-inline 2 KB image still splits (the size-threshold trap must not reappear).
9d. Component attribution: for a pair differing only in submit time, `tier1_divergent_metadata` increments and `tier1_divergent_body` does not; for a pair differing only in body text, the reverse.
10. `per-source`: the same message in two sources yields two winners; `global` yields one; the All Custodians aggregate degenerates correctly.
11. `tier1_content_divergent` counts without moving winners; `--tier1-verify content` splits the divergent pair.
12. Bind provenance: every duplicate row's `bound_by` is one of the closed values and matches the tier the grouping actually used (no `content_hash` fallback for a MID-bound row).
13. Index/grouping equivalence across shuffles × all `GroupingContext` combinations.
14. Refinement assertion over fixtures for defaults and each split-only flag (§3.11).
15. CLI: `--help` snapshot; both parsers reject an unknown enum value with the same message; decision CSV header prefix unchanged; pre-0076 JSON deserializes.
16. Source PSTs unchanged — full-file SHA-256 before/after an integration run.

---

## 4. Out of scope

| Item | Why | Residual |
|---|---|---|
| Changing the default identity to v2 | Breaks every stored `content_hash_hex` | **D-0076-default-v2** |
| Near-duplicate / semantic similarity | Owned by 0023 matter jobs | — |
| Changing Tier-1 MID normalization | Frozen (rule 3) | — |
| Email threading / conversation index | 0022 | — |
| Attachment-content digests if 0074 probe reuse is non-trivial | Cost/complexity gate | **D-0076-attach-content** |
| Recipient **table** reads (per-recipient SMTP + `PidTagRecipientType`) instead of display strings | Real fix for X.500 / display-name variance; new reader surface in `pst-reader` messaging | **D-0076-recipient-table** |
| Inline-attachment detection if `PidTagAttachContentId` / `AttachFlags` reads grow past the attachment PC | Ship the counter only | **D-0076-inline-attach** |
| "Template / Newsletter" class for large MID-distinct clusters | Stats surface the cluster; classification is a product feature | **D-0076-bulk-class** |
| Custodian map (many PSTs → one custodian) | Needs an operator-supplied mapping surface | **D-0076-custodian-map** |
| Desk surfaces for scope / identity enums | Consistent with D-0075-gui | **D-0076-gui** |
| Disk-backed grouping at multi-million scale | 0079 / D-0066-disk-groups | — |
| Multi-GB performance proof | Operator-local only | **D-0076-operator-perf** |
| Full body normalization parity with Relativity (`PR_BODY` exact whitespace rules) | We strip CR/LF/tab and keep spaces; Relativity strips spaces too. Changing v1 normalization is a hash change. | **D-0076-normalize-parity** |

---

## 5. Preconditions

1. **0074 and 0075 are merged to `main`** (`28c0065`, `f996392`) — verified 2026-07-28. No rebase gate this time.
2. **0077 also touches `scan.rs`** (CRC noise). If 0077 starts first, rebase before Phase 2; the surfaces are adjacent but not overlapping.
3. Capture the **pre-0076 grouping baseline** (groups + winners + hashes) on the committed fixtures **before any edit** — it is worthless captured afterwards.
4. `ledgerful ledger start 0076-contenthashtierhardening --category FEATURE`, `ledgerful scan --impact`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| A split-only guard changes 0075's checked-in winner golden | Phase 0 baseline + refinement assertion; any golden change must map to a non-zero stat and be justified in `review.md` (§3.11) |
| Operators see unique counts **rise** and read it as a regression | Human summary names the cause and count for each guard; docs state plainly that a rise is the guards refusing an unsafe merge |
| **Bulk-mail inflation** — cross-MID guard multiplies newsletter copies and blows up a review budget | `cross_mid_blocked_max_group` surfaces the largest cluster in the run summary; docs give the culling lever and the reasoning for both choices (§3.4, §3.13); one flag reverts |
| Non-Latin `content_hash_hex` no longer reproduces a pre-0076 run | Bounded, named exception in rule 5; counted by `tier2_preview_bytes_over_budget`; direction proven split-only in test 1b |
| `--identity-ignore-inline-attachments` drops a genuinely responsive small image | MAPI-flag detection, not a size threshold; opt-in; `inline_attachments_ignored` reports the count; test 9c pins that a non-inline small image still splits |
| The degenerate rule over-splits on legitimately sparse items (calendar/contacts) | Rule is two-clause and conservative; test 4 pins both directions; `--allow-degenerate-tier2` is the immediate operator escape |
| v2 recipient normalization diverges across stores (display strings, not addresses) | Documented as a *display-string* comparison, not address resolution; that is why v2 is opt-in and why `tier2_5_splits` is reported |
| Two grouping implementations drift again | Rule 7 equivalence test across all option combinations (§3.11) |
| Scope creep into near-dup or threading | §4 named residuals; grouping-key changes only |
| `--strong-content-hash` re-reads bodies by accident during materialize | Digest is computed in the scan pass only; materialize never recomputes identity |

---

## 7. Definition of Done

- [ ] **DoD-1** `GroupingContext` threaded through `DedupIndex`, `group_candidates`, `rebuild_dedup_results`, `attach_probe`, and all CLI call sites; `Default` = pre-0076 minus the split-only guards.
- [ ] **DoD-2** **Character**-clamped preview (all 4096 chars hashed regardless of script) and guarded subject slices; CJK repro passes; ASCII digest unchanged against a checked-in value; Cyrillic change proven split-only and counted; dead `body_hash_len` GUI setting removed or wired (§3.2).
- [ ] **DoD-3** Unreadable-body and degenerate-preimage items are Tier-2 ineligible; both stats reported in JSON and human summaries; `--allow-degenerate-tier2` restores exactly (§3.3).
- [ ] **DoD-4** Cross-MID merges blocked with `cross_mid_blocked` / `_groups` / `_max_group`; bulk-mail inflation documented per §3.13; `--allow-cross-mid-tier2` restores exactly (§3.4).
- [ ] **DoD-5** `member_tier` deleted; `BoundBy` recorded at bind time; `bound_by` + `identity_version` on decision rows; `DedupIndex` agrees (§3.5).
- [ ] **DoD-6** `--strong-content-hash off|body|body-recip` with the layered v2 preimage of §3.6; refinement property test green; `display_cc` read soft on the already-loaded PC; component fingerprints stored and used for attribution only.
- [ ] **DoD-6b** Level `body-recip-attach` shipped over 0074's probe **or** declined in `review.md` as **D-0076-attach-content** with the reason.
- [ ] **DoD-6c** Recipient honesty: `tier2_5_splits_recipients_only` + `x500_recipient_items` reported; display-string variance documented; **D-0076-recipient-table** recorded.
- [ ] **DoD-6d** `--identity-ignore-inline-attachments` via MAPI flags (not a size threshold) with `inline_attachments_ignored`, **or** declined as **D-0076-inline-attach** with the counter shipped alone.
- [ ] **DoD-7** `tier1_divergent_body` / `_metadata` / `_recipients` reported separately with the hint on body only; `--tier1-verify content|body` splits; winners unmoved when off (§3.7).
- [ ] **DoD-8** `--dedupe-scope per-source` (D-0075-scope closed); scope echoed in JSON + report pack; All Custodians degeneracy documented (§3.8).
- [ ] **DoD-9** `--tier1-backfill` default off with `tier1_backfill_candidates` always reported (§3.9).
- [ ] **DoD-10** Flags on all five subcommands, both parsers, identical help; Desk checkbox for `body`.
- [ ] **DoD-11** Refinement assertion green over fixtures for defaults and each split-only flag; 0075 winner golden holds or is re-baselined with recorded justification.
- [ ] **DoD-12** Index/grouping equivalence test across shuffles × all option combinations.
- [ ] **DoD-13** Pre-0076 `keep_set_v1` JSON deserializes; decision CSV header prefix unchanged; `--help` snapshot.
- [ ] **DoD-14** Source PSTs byte-identical (full-file SHA-256) across an integration run.
- [ ] **DoD-15** Performance: default ≤ +2% (hard ceiling +10%) on fixtures; `body` level cost measured; both recorded in `review.md`.
- [ ] **DoD-16** `docs/unique-pst-export.md` "Identity and binding" section per §3.13, including the named Relativity divergences.
- [ ] **DoD-17** Full gate green: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`.
- [ ] **DoD-18** `review.md`; `D-0076-*` rows in `docs/deferred.md`; **D-0075-scope marked closed**; `conductor.md` + `sequencing.md` → Completed; `ledgerful verify` + ledger commit.

---

## 8. Verification

```powershell
cargo test -p dedup-engine --lib
cargo test -p pst-dedup-cli --test keep_set
cargo test -p pst-dedup-cli --test unique_pst
cargo test -p pst-dedup-cli --test scan_integrity
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Operator-local smoke (never committed, never `git add`-ed):

```powershell
cargo run -p pst-dedup-cli --release -- scan <local.pst> --json
cargo run -p pst-dedup-cli --release -- scan <local.pst> --strong-content-hash body --json
cargo run -p pst-dedup-cli --release -- keep-set <local.pst> --dedupe-scope per-source --json
```

Compare `unique_count`, the four guard stats, and wall time between runs 1 and 2; unique counts may only rise.

---

## 9. Handoff

**Do**

- Capture the Phase 0 baseline before touching any file.
- Keep every default-on change split-only, and prove it with the refinement assertion rather than by inspection.
- Delete `member_tier` rather than repairing it.
- Report every guard's count in the human summary, not only JSON.
- Update both flag parsers and both grouping implementations together.

**Do not**

- Re-introduce a **byte** clamp on the body preview — it shrinks non-Latin comparison windows (§3.2). Character clamp only, and no other change to the v1 preimage.
- Gate Tier-2 equality on `PidTagMessageSize`, or drop attachments from identity by a **size threshold** (§3.9) — inline detection is by MAPI flag.
- Treat `display_to`/`cc`/`bcc` as addresses; they are display strings (§3.6).
- Fabricate a digest for a body or attachment that failed to read.
- Ship any merge-increasing behavior on by default.
- Add a hashing dependency, re-read a PST to confirm identity, or touch Tier-1 normalization.
- Widen into near-dup, threading, or custodian mapping.

**Rollback:** unregister the CLI flags and set `GroupingContext::default()`'s two guards to `false` — grouping returns to pre-0076 byte-for-byte, with the §3.2 panic fix retained.
