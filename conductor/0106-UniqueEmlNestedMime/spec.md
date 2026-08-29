# 0106 — Unique-EML Nested MIME (`message/rfc822`)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-pst nested write, identity-hash
> depth, BCC default, HNBITMAPHDR, or frontend during implementation.

- **Track ID:** 0106-UniqueEmlNestedMime
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-eml-import.md` + `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0067 (Completed — unique-eml pack) · 0089 (Completed — unique-eml attach ledger) · 0094 (Completed — `NestedCanonicalMessage` + `materialize_nested_for_winner`) · 0101 (Completed — unique-pst `--max-embedded-depth`). Series Q **0105** Completed is not a code dependency.
- **Spec authored:** 2026-08-28
- **Series:** Q (Unique-export honesty residuals, post-0105)
>
> **Narrows:** `D-0067-embedded-depth` (unique-eml nested MIME half only — does **not** close the row).
> **HITL:** none required. Synthetic method-5 chain fixtures are enough. No INC* unique-eml smoke.
>
> **Last-PR fold-in (2026-08-28):** PRs **#101, #100, #99, #98**. Disposition in §2.8. No Cursor/Bugbot comments in that window. Origin residual is **D-0067** after 0094/0101 (unique-pst nested export shipped; unique-eml still labels raw attach bytes as `message/rfc822`).
>
> **Review fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Lock: method-5 skip/DTO **before** `open_attach_body`; parser is **`pub`** (not `pub(crate)`); skip/reconstruct gates on `attach_method == ATTACH_EMBEDDED_MSG`; parent-depth halt (`depth >= max` skips the child; exactly `max` nests); dedicated skip-error variant (not `Other` substring); inner events use inner subject; inner headers always emit `Subject:`.
>
> This ID was unused. It is **not** stolen for Hermes Series O (frontend stays **0107+**).

---

## 1. Objective

Make `pst-dedup unique-eml` write **real nested RFC 5322** inside `Content-Type: message/rfc822` for method-5 (`ATTACH_EMBEDDED_MSG`) winners, using the 0094 `NestedCanonicalMessage` DTO already extracted by `materialize_nested_for_winner`. Stop labeling raw MAPI attach bytes as `message/rfc822`. Wire the same `--max-embedded-depth` knob unique-pst already has (default **3**, clap reject outside **1–8**).

Today unique-eml always dumps the attach stream as 8bit `message/rfc822` and sets `embedded_message_unparsed = true`. `EmlWriteOpts.max_embedded_depth` exists but is unused (`let _ = (opts, depth)`). unique-eml never calls `materialize_nested_for_winner`. Counsel importing the EML pack sees an “embedded message” whose body is not RFC 822.

This advances unique-export **defensibility**: the unique-EML pack must be honest MIME, not a MAPI blob with a rfc822 label. Depth-capped and unreadable nests stay ledgered (`ATTACH_DEPTH_LIMIT` / `ATTACH_EMBEDDED_UNPARSED`) — never silently dropped, never invented.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (`D-0067-embedded-depth` unique-eml half, still live)

Deferred row (do **not** close): unique-pst nested extract/write + CLI depth shipped in **0094 / 0101**. Residual stays unique-eml nested MIME, matter/Relativity child-document extract, 32 MiB per-nest, hard cap 8.

Live confirmation 2026-08-28, `main` @ `40d5a43` (re-verify line numbers at execute):

| Surface | State |
|---|---|
| unique-eml CLI | `UniqueEmlCliArgs` has **no** `max_embedded_depth`. `main.rs` UniqueEml clap has **no** `--max-embedded-depth`. `run_unique_eml` builds `EmlWriteOpts { family_policy, ..Default }` (depth 3 default, unused). |
| Write loop | Re-materialize each winner then `write_canonical_eml` — **no** `materialize_nested_for_winner`. unique-pst calls it during finalize `on_winner` (`unique_pst_cmd.rs` ~2002). |
| `prepare_one_attach` | Method-5 / mime-rfc822: dump attach bytes as `message/rfc822` + 8bit; `unparsed: true`. Comment: residual D-0067; `let _ = (opts, depth)`. |
| `write_prepared_part` | RFC 2046 CTE 8bit (never base64) already locked for rfc822 parts. Counts `embedded_messages_written`; sets `embedded_message_unparsed` when `unparsed`. |
| DTO | `CanonicalAttachment.embedded_message: Option<Box<NestedCanonicalMessage>>` (`#[serde(skip)]`). Comment: “unique-eml ignores this field this track.” `embedded_extract_limit` maps unique-pst `ATTACH_DEPTH_LIMIT`. |
| Extract | `materialize_nested_for_winner` + `fill_nested_on_attach` already fill the DTO; `MAX_NESTED_EXPORT_PAYLOAD_BYTES` = 32 MiB; clamp 1–8. Child streams via `PstAttachStreamSource::open_attachment_data_reader` (registered `MessageNodeRef`; nested NIDs are not in the NBT). unique-eml uses the same `PstAttachStreamSource`. |
| Tests | `embedded_message_rfc822_not_octet_stream`: method-5 + in-memory `From: x\r\n\r\nbody` + **no** DTO → dump + `unparsed`. `embedded_soft_fail_skips_no_fake_rfc822_body`: stream fail → skip, no rfc822 part. |
| Ledger map | `map_eml_attach_fail_reason` has cloud / stream-open / `ATTACH_UNKNOWN`. **No** `ATTACH_DEPTH_LIMIT` / `ATTACH_EMBEDDED_UNPARSED` first-class path. |
| Summary | `UniqueEmlSummaryOut` is **Serialize-only** (`keep_set_v1` + `eml_pack_v1`). No `max_embedded_depth`. Written to `{out}/summary.json`. `EmlPackManifest` is Serialize+Deserialize — **do not** require a new required manifest field (Deserialize old packs). |
| Identity hash | `MAX_EMBEDDED_IDENTITY_DEPTH = 3`. **Do not change.** |
| unique-pst | Nested write + `--max-embedded-depth` **shipped**. Do not edit writer/reader nested export in this track. |
| BCC | unique-eml **writes** `Bcc:` from `display_bcc` when non-empty (`write_headers` ~715). unique-pst 0082 default suppress is a **different** surface. Do not apply PST BCC suppress to EML. No BCC-default track. |

### 2.2 Why the current rfc822 dump is a lie

Method-5 attach payload in a PST is a nested **message object** (subnode PC), not RFC 822 bytes. unique-eml labels those bytes `Content-Type: message/rfc822`. Outlook/Thunderbird import then shows garbage or a nested part that is not the embedded mail. 0094 already extracts `NestedCanonicalMessage` (subject, sender, display_*, recipients, bodies, child attaches). unique-eml ignores it.

By-value attaches whose MIME is `message/rfc822` (method ≠ 5) **are** RFC 822 bytes (an attached `.eml`). Dumping those as rfc822 8bit stays correct. 0094 lock: do **not** rewire method-1 as nested `WriteMessage` / `NestedCanonicalMessage`.

### 2.3 RFC 2046 / crate APIs (plan-time)

**RFC 2046 §5.2.1** (`message/rfc822`, fetched 2026-08-28 from rfc-editor): *“No encoding other than 7bit, 8bit, or binary is permitted for the body of a message/rfc822 entity.”* RFC 2045: composite types (`message`, `multipart`) must not use base64/quoted-printable on the **wrapper**. Inner parts encode at the innermost level. Live unique-eml already uses 8bit on the wrapper — keep that. Inner nested MIME may use 8bit text and base64 on **file** parts (same as top-level).

**MS-PST:** N/A for new structures. Nested extract + `PidTagAttachDataObject` PtypObject already shipped in **0094**. This track is unique-eml MIME + CLI wire of that extract.

Crate-registry API churn: none expected. No new deps. Schema / matter-core version: N/A (unique-eml pack only). `eml_pack_v1` schema **id** stays; new always-present summary key (see §3).

### 2.4 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3860 pinned).
- `ai-brains sync query` / `recall` — 0067 unique-eml rfc822 8bit residual; 0090 leave D-0067 open; 0094 unique-eml ignores nested DTO; 0101 unique-pst CLI shipped, D-0067 not closed; frontend Series O **if started** uses 0106+ — this pass uses **0106** for unique-eml MIME, so frontend moves to **0107+**.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift (before this planning tx). `scan --impact` **LOW** (HEAD `40d5a43`; dirty tree is skills + `agy-review.md` + `fixtures/keep_set_summary.json`, not product crates). Hotspot `export_exit_0078.rs` is out of scope. Conductor files are gitignored so they do not appear in the git scan.
- Ledger tx for this planning pass: `137b1441-b8d2-4faf-b654-9a8e1ab8923c`.
- ai-brains Ready pin: `144bbcc2-8097-4111-8a8b-3fc8f2fec158`.
- `C:\dev\Dedupe-plan.md` absent.

### 2.5 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0067: unique-eml MIME multipart; rfc822 8bit; UTC Date; no re-dedupe | Keep wrapper CTE 8bit; Date UTC; keep-set winners only |
| 0094: unique-eml ignores nested DTO **this track**; depth owner writer `max_embedded_depth`; 32 MiB; winner-only extract | **This track consumes the DTO** on unique-eml. Do not change unique-pst writer. Same extract helper + budget. |
| 0101: unique-pst `--max-embedded-depth` 1–8 default 3; identity hash stays 3; D-0067 not closed | Same flag semantics on unique-eml. Do not close D-0067. |
| 0082 BCC opt-in (unique-pst) | Unchanged. unique-eml continues to write `Bcc:` from `display_bcc` when present (parent **and** nested). |
| 0089 unique-eml attach ledger | Reuse `EmlAttachEvent`; add depth/unparsed reason codes; counters remain classify source of truth. |
| Frontend 0106+ | **0106 is this unique-eml residual**, not Tauri. Frontend **0107+**. |

### 2.6 How this advances the north star

Counsel-facing unique-EML must be honest MIME. A `message/rfc822` part whose body is MAPI is a silent lie. Parsed nested mail (or a ledgered skip) is the defensible pack. Depth remains bounded so hostile nests cannot unbounded-recurse.

### 2.7 Why not frontend / HNBITMAPHDR / unique-pst / BCC-default

- Hermes Series O (Tauri/Leptos) was reserved at 0106+ **if started**. Unique-eml nested MIME is the remaining **P1** half of `D-0067`. North star is unique-export honesty, not UI polish. Frontend IDs start at **0107**.
- `D-0100-hn-bitmap-hdr`: fail-closed until a corpus hits the error. Writer, not EML.
- unique-pst nested write already shipped (0094/0101). Do not reopen.
- No BCC-default track (0082 unique-pst opt-in stays). unique-eml header policy stays: write `Bcc:` when `display_bcc` is non-empty.

### 2.8 Last-PR Cursor comments (merged #101, #100, #99, #98)

Skill: last 2–4 merged product PRs.

| PR | Comment | Verdict |
|---|---|---|
| **#101** (0105 docs merge record) | No review / issue / inline comments. | n/a |
| **#100** (0105 window-edge normalize) | No review / issue / inline comments. | n/a — 0105 Completed |
| **#99** (0104 docs) | No comments. | n/a |
| **#98** (0104 attach TC) | No comments. | n/a |

Nothing else to mint. Origin work is the deferred unique-eml half of **D-0067** (this track). No BCC-default track. No HNBITMAPHDR track. Frontend stays **0107+**.

### 2.9 Dual-AI review disposition (2026-08-28)

Reviews: `opencode-review.md` (Ready after fixes; M1–M3 + m1–m3 + O2) and `agy-review.md` (PASS; restates the Ready plan). Neither asked to reopen unique-pst nested write, identity-hash depth, BCC default, HNBITMAPHDR, or frontend.

Live re-check this fold-in @ `40d5a43`: `open_attach_body` is at `prepare_one_attach` **:951** before `if embedded` **:953**; `is_embedded_message` is method-5 **or** mime rfc822 (**:1180–1188**); `parse_max_embedded_depth_arg` is private in the **lib** crate (`unique_pst_cmd.rs:573`) while UniqueEml clap lives in the **bin** `main.rs`; writer halt is `if depth >= max_depth` with top-level `build_message_node(..., 0)` (`production.rs:1756` / `:3332`); `write_headers` always emits `Subject:` even when empty (`eml_pack.rs:704–705`). No method-1 mime-rfc822 dump unit test exists.

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-M1 | opencode-review.md | Major | **Agree — fold** | Method-5 depth / DTO / honesty-skip **before** `open_attach_body`. Open the stream only for method-1 rfc822 dump and ordinary file parts. Method-5 skip `Err` must not become `ATTACH_STREAM_OPEN_FAILED`. |
| opencode-M2 | opencode-review.md | Major | **Agree — fold** | `parse_max_embedded_depth_arg` must be **`pub`** (plain). `pub(crate)` is invisible to bin `main.rs` (E0603); `pub use` of a private item is E0364. Same 1–8 error string. Do **not** mint `UniqueEmlClapArgs` this track. |
| opencode-M3 | opencode-review.md | Major | **Agree — fold** | Reconstruct/skip gates on `att.attach_method == Some(ATTACH_EMBEDDED_MSG)`. The `embedded` / `is_embedded_message` local is **not** the discriminator (it is also true for method-1 mime rfc822). Method-1 rfc822 dump test is **mandatory**. |
| opencode-m1 | opencode-review.md | Minor | **Agree — fold** | Halt compares **parent** depth before writing its method-5 child (`parent_depth >= max` → skip child). Winner is depth 0. Child level = parent + 1. Exactly **`max`** nested rfc822 parts (default 3 writes nests 1–3; 4th is `ATTACH_DEPTH_LIMIT`). Mandatory 2-level DTO @ `max_embedded_depth: 1`. |
| opencode-m2 | opencode-review.md | Minor | **Agree — fold** | Dedicated skip transport (`EmlWriteError` variant or skip-enum carrying the 0073 code). Do **not** encode `ATTACH_DEPTH_LIMIT` / `ATTACH_EMBEDDED_UNPARSED` as `Other` substring probes. Update `map_eml_attach_fail_reason` match arms (clippy `-D warnings` catches non-exhaustive). |
| opencode-m3 | opencode-review.md | Minor | **Agree — fold** | Nested soft-fail events use the **inner** message subject and the inner parent's `attach_index`. `attach_nid` distinguishes collisions. |
| opencode-O1 | opencode-review.md | Opportunity | **Already covered** | Live table @ `40d5a43` matches §2.1. Human-summary `~711` pin is assembly not print — re-verify at execute. |
| opencode-O2 | opencode-review.md | Opportunity | **Agree — fold** | Inner header builder always emits `Subject:` (empty allowed). RFC 2046 §5.2.1 requires at least one of From / Subject / Date; live `write_headers` already always writes Subject. |
| opencode-O3 | opencode-review.md | Opportunity | **Already covered** | Plan-time pin count vs live preflight is cosmetic. |
| opencode-O4 | opencode-review.md | Opportunity | **Already covered** | Deferred / last-PR / crate boundary. |
| agy-0106-1..5 | agy-review.md | — | **Already covered** | Reconstruct-or-skip, stream locus, inner headers, write-loop extract, summary-only key. Audit 0106-1 `depth < max` is the parent-depth reading locked by m1. |
| agy-UniqueEmlClapArgs | agy-review.md | — | **Decline** | There is no `UniqueEmlClapArgs`. Unique-eml clap stays the `UniqueEml` variant in `main.rs`. M2 `pub` parser is the visibility fix; do not lift unique-eml into a lib clap struct this track. |

**Declined / not locked**

- Minting `UniqueEmlClapArgs` (agy Phase 2 wording). Bigger diff than `pub` parser.
- Counting `AttachStreamSource` stub as a **required** DoD test (M1 optional). Behavioral skip + no rfc822 part is the gate; a call-count stub is nice-to-have.
- Closing `D-0067-embedded-depth`.
- BCC-default track / unique-pst suppress on unique-eml.
- Frontend **0107+**.

---

## 3. In scope

1. **Parsed nested MIME (method-5 + DTO):** When `att.attach_method == Some(ATTACH_EMBEDDED_MSG)` (0x5) **and** `embedded_message` is `Some` **and** parent `depth < max`: write a `message/rfc822` part whose **body** is a reconstructed RFC 5322 message from `NestedCanonicalMessage` (Message-ID, **always `Subject:`** (empty allowed — RFC 2046 §5.2.1 From/Subject/Date net), From, To, Cc, Bcc-if-present, Date UTC, plain/html body, child attaches). Wrapper CTE remains **8bit** (never base64). `embedded_message_unparsed` is **false** for that part. Recurse on child method-5 attaches. **Discriminator is `attach_method`, not `is_embedded_message` / the `embedded` local** (that helper is also true for method-1 mime rfc822).
2. **Honesty skip (method-5, no DTO):** If method-5 and `embedded_message` is `None` and `embedded_extract_limit` is **false**: **do not** dump attach/MAPI bytes as rfc822. Soft-skip the part (no headers, no boundary body, no fake RFC822). Emit `EmlAttachEvent` reason **`ATTACH_EMBEDDED_UNPARSED`**. Increment `attachments_failed`. Set `embedded_message_unparsed = true`. Same H1 skip as stream-open fail. Decide this **before** `open_attach_body` (live `:951` opens first today).
3. **Depth skip:** If `embedded_extract_limit` **or** **parent** `depth >=` effective `max_embedded_depth`: skip the child part; reason **`ATTACH_DEPTH_LIMIT`**. Winner is depth 0; child level = parent + 1; exactly **`max`** nested rfc822 parts are writable (matches unique-pst `write_one_attachment` `if depth >= max_depth` with top-level depth 0). `embedded_message_unparsed = true`; `attachments_failed++`. Do **not** invent a truncated inner message. Decide **before** `open_attach_body`.
4. **By-value rfc822 (method ≠ 5, mime contains `message/rfc822`):** keep today’s dump (those bytes are the attached message). Still 8bit wrapper. Still `unparsed` (this track does **not** re-parse rfc822 bytes into `NestedCanonicalMessage`). 0094 method-1 lock stands.
5. **Extract on unique-eml write path:** After re-materialize of each winner (the existing write loop ~364), call `materialize_nested_for_winner(&mut attach_src, &mut msg, effective_depth)` **before** `write_canonical_eml`. Do **not** extract during the promote-only finalize pass (`|_msg| Ok(())`) — that pass drops bodies; nested extract there wastes RAM. Log extract errors as warnings; missing DTO then follows honesty skip (item 2).
6. **CLI `--max-embedded-depth` on `unique-eml`:** long flag name locked (same as unique-pst). Default **3**. **Reject** values outside **1–8** as usage error (do not silently clamp operator typos). Help text states the range and default. Reuse unique-pst’s parser: make `parse_max_embedded_depth_arg` **`pub`** (plain) so bin `main.rs` can use `pst_dedup_cli::unique_pst_cmd::parse_max_embedded_depth_arg`. **`pub(crate)` does not compile** across the lib/bin crate boundary; a `pub use` of a private item is E0364. Same 1–8 error string. Do **not** mint `UniqueEmlClapArgs`.
7. **`UniqueEmlCliArgs.max_embedded_depth: u32`.** `main.rs` copies the parsed value. `run_unique_eml` uses **that same value** for `materialize_nested_for_winner` and `EmlWriteOpts.max_embedded_depth`. Runtime `.clamp(1, 8)` remains as belt-and-suspenders for library callers.
8. **Stream parent for nested child attaches:** `PstAttachStreamSource.open_attach` keys on `(source_path, parent.nid)`. Nested child `attach_nid`s belong to `NestedCanonicalMessage.source_msg_nid`, **not** the top-level winner nid. Recursive write must open child streams with `nid = source_msg_nid` and the winner’s `source_path`. Missing `source_msg_nid` → child attach soft-fail (no invented bytes).
9. **Nested inner headers:** reconstructed inner RFC 5322 **must not** copy parent `X-Pst-Dedupe-Source` / `X-Pst-Dedupe-Folder`. Optional `X-Pst-Dedupe-Nid` from `source_msg_nid` only when present. Date UTC `+0000` (same as parent). `Bcc:` when nested `display_bcc` is non-empty (unique-eml parent policy, **not** unique-pst suppress). **Always emit `Subject:`** even when `dto.subject` is `None` (empty value is fine) so RFC 2046 §5.2.1 “at least one of From / Subject / Date” cannot fail on a From-less, Date-less nest. Live `write_headers` already always writes Subject — reuse or replicate that line.
10. **Boundaries:** inner `multipart/*` boundaries must differ from the outer mixed boundary (`make_boundary` already includes nid; fold **depth** or nested nid into the token so a nest cannot collide with parent).
11. **Summary honesty:** `UniqueEmlSummaryOut.max_embedded_depth: u32` (plain field, **always serialize**, no `skip_serializing_if`, no `serde(default)` — the type is Serialize-only). Set from the **effective clamped** value. Schema ids stay `keep_set_v1` / `eml_pack_v1` (**no** id bump). Do **not** add a required field to `EmlPackManifest` (it is Deserialize; old packs must still parse). Human stderr/stdout: print `max_embedded_depth` near attach counts.
12. **Ledger map:** first-class `ATTACH_DEPTH_LIMIT` and `ATTACH_EMBEDDED_UNPARSED` on `EmlAttachEvent.reason_code` (0073 taxonomy). Transport is a **dedicated** `EmlWriteError` variant (or skip-enum) that carries the 0073 code. Do **not** stringify-match `EmlWriteError::Other`. Do **not** use pack-manifest `ATTACH_PART_FAILED` as a CSV reason. Histogram / classify still key off `attachments_failed` counters. Nested soft-fail rows use the **inner** message subject and the inner attachment index (`attach_nid` distinguishes collisions).
13. **Tests** (synthetic only; see §7 / §10):
    - Nested DTO → inner Subject/From appear inside the rfc822 part; `embedded_message_unparsed == false`; wrapper has 8bit not base64.
    - Nested child **file** attach is base64 inside the **inner** mixed; outer wrapper still 8bit.
    - Method-5 **without** DTO → no `message/rfc822` part, `ATTACH_EMBEDDED_UNPARSED` event (existing dump test **must fail on HEAD** after the honesty skip).
    - Method-1 mime `message/rfc822` dump **mandatory** (bytes preserved, 8bit, `unparsed: true`, no new skip reason). Live tree has **no** such test.
    - Unit: 2-level DTO @ `max_embedded_depth: 1` — nest 1 present, nest 2 `ATTACH_DEPTH_LIMIT` (distinguishes parent-depth vs child-depth halt).
    - Depth-4 chain @ default 3 vs `--max-embedded-depth 4` (CLI, new `unique_eml_depth.rs`; do **not** reuse a helper that injects `--no-attachments`).
    - Ceiling pair 8@7 vs 8@8 (buildable; no CLI chain-of-9).
    - Clap 0 / 9 / non-integer are usage errors.
    - `summary.json` `max_embedded_depth` equals effective value.
    - `parents_only` still omits nested parts.
    - Stream-open fail still skips with no fake rfc822 body.
    - Method-1 mime rfc822 dump still works.
    - Parent `content_hash` unchanged when DTO is populated (already in keepset tests; keep green).
14. **Docs:** `docs/unique-eml-import.md` embedded-messages row: parsed nested MIME when extract succeeds; depth flag; unparsed/depth ledgered skips (no MAPI dump). Flag table row. `docs/unique-pst-export.md` nested paragraph: unique-eml **no longer** ignores the DTO. CHANGELOG. Deferred row **narrowed** (do not close).

---

## 4. Out of scope (do NOT do here)

- unique-pst nested write / `PidTagAttachDataObject` / `--max-embedded-depth` on unique-pst (already 0094/0101).
- Identity hash `MAX_EMBEDDED_IDENTITY_DEPTH` (locked 3).
- Matter / Relativity child-document extract (residual of D-0067 — **do not close** the row).
- Raising or removing the 32 MiB per-nest budget or the hard cap 8.
- Re-parsing by-value `message/rfc822` bytes into `NestedCanonicalMessage`.
- `--also-eml` co-export (`D-0071-also-eml`).
- GUI unique-eml wizard / depth slider (`D-0072` / `D-0067-gui-keepset`).
- Minting `UniqueEmlClapArgs` (agy suggestion). Unique-eml clap stays the `UniqueEml` variant in `main.rs`.
- unique-pst BCC default / `--include-bcc-recipients` (0082 stays).
- HNBITMAPHDR (`D-0100-hn-bitmap-hdr`).
- Per-event attach CRC (`D-0099-attach-crc-job-level`).
- Frontend / Hermes Series O (**0107+**).
- COM Outlook; client PSTs in git; in-tool ScanPST / CRC repair.
- Cloud attach hydration (`D-0067-cloud-attaches`).
- Windows `\\?\` long-path (`D-0067-long-path`).

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0094 Completed (`NestedCanonicalMessage`, `materialize_nested_for_winner`, `source_msg_nid`, `embedded_extract_limit`). 0089 Completed (`EmlAttachEvent` → CSV). 0067 Completed (`write_canonical_eml`). Verified @ `40d5a43`.
- **P2 (soft):** 0101 parser/docs for `--max-embedded-depth` — copy semantics, do not change unique-pst behavior.
- *Verified to date:* unique-eml does not call nested extract; `prepare_one_attach` ignores `embedded_message`; `parse_max_embedded_depth_arg` is private in `unique_pst_cmd.rs`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer keeps dumping method-5 stream when DTO is missing | DoD-2 honesty-skip test **must fail on HEAD**. No MAPI-as-rfc822. |
| Nested child attach opened with winner nid | Spec §3.8: stream parent nid = `source_msg_nid`. Test a nest with a child file attach. |
| Inner X-Pst-Dedupe-Source copies parent path | Nested header lock: no parent Source/Folder X-headers. Assert absent in unit test. |
| Base64 on the rfc822 **wrapper** | Keep `write_prepared_part` 8bit for `embedded: true`. Existing `part_has_base64_cte_for_rfc822` stays green; extend to parsed nested. |
| Unbounded recursion | Same clamp 1–8 as unique-pst; writer depth halt `depth >= max`; extract remaining_depth == 0 sets `embedded_extract_limit`. |
| Extract during promote pass | Write-loop only. Promote finalize still `|_| Ok(())`. |
| Clap parser drift 1–8 | Make `parse_max_embedded_depth_arg` **`pub`** (not `pub(crate)` — bin `main.rs` cannot see crate-private lib items). Same error string. |
| `open_attach_body` before method-5 skip | Decide depth / DTO / honesty-skip **first**. Stream open only for dump paths. Otherwise a dead method-5 stream becomes `ATTACH_STREAM_OPEN_FAILED`. |
| Hanging skip logic off `if embedded` | `is_embedded_message` is also true for method-1 mime rfc822 → those dumps would skip as `ATTACH_EMBEDDED_UNPARSED`. Gate on `attach_method == ATTACH_EMBEDDED_MSG`. |
| Off-by-one depth halt | Parent depth before child; winner depth 0; `depth >= max` skips the child; exactly `max` nests. Mandatory 2-level @ max=1 unit test. |
| `Other` substring for new reasons | Dedicated skip variant carrying the 0073 code. |
| `serde(default)` on Serialize-only summary | Same 0101 trap: **no** `serde(default)` on `UniqueEmlSummaryOut`. |
| Required new `EmlPackManifest` field | Old packs would fail Deserialize. Summary-only for the new key. |
| Treating unique-eml BCC like unique-pst suppress | Nested writes `Bcc:` when `display_bcc` present. No new flag. |
| Chain-of-9 CLI test | Writer cannot emit a 9th nest (0101). Ceiling pair is 8@7 vs 8@8. |
| Touching unique-pst / writer / identity hash | Out of scope. |
| Frontend ID collision | This track **is** 0106. Frontend **0107+**. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Parsed nested MIME + honesty skip:** Method-5 (`attach_method == ATTACH_EMBEDDED_MSG`, **not** `is_embedded_message`) + DTO + `parent_depth < max` writes a `message/rfc822` wrapper (8bit, never base64) whose body is reconstructed RFC 5322 from the DTO (headers including **always `Subject:`**, body, child attaches, recursive). Method-5 skip/DTO/depth decisions run **before** `open_attach_body`. Method-5 without DTO does **not** dump attach bytes as rfc822 (`ATTACH_EMBEDDED_UNPARSED` via dedicated skip variant, not `Other`). Parent `depth >= max` or `embedded_extract_limit` → `ATTACH_DEPTH_LIMIT` skip (exactly `max` nests). By-value mime rfc822 dump unchanged. unique-eml write loop calls `materialize_nested_for_winner` with the **same** effective depth as `EmlWriteOpts.max_embedded_depth`. Nested child streams use `source_msg_nid`. Inner messages do not copy parent `X-Pst-Dedupe-Source`/`Folder`. Nested ledger events use the inner subject. `parents_only` omits nested parts. No `unwrap`/`expect` in production. Source PSTs read-only.
- [ ] **DoD-2 — CLI depth + tests:** `unique-eml --max-embedded-depth` default 3; clap rejects outside 1–8 (`pub` parser). `summary.json` always has `max_embedded_depth` = effective clamp. Tests in §10.2: (a) nested DTO unit **must fail on HEAD**; (b) method-5 no-DTO skip **must fail on HEAD** (today dumps rfc822); (c) **mandatory** method-1 mime rfc822 dump (bytes + 8bit + unparsed, no skip event); (d) **mandatory** 2-level DTO @ max=1 (nest1 present, nest2 `ATTACH_DEPTH_LIMIT`); (e) CLI depth-4 pair and 8@7 vs 8@8; (f) clap 0/9; (g) existing unique-eml pack tests + `embedded_soft_fail_skips_no_fake_rfc822_body` + parent-hash skip-serde tests stay green. No client PSTs in git.
- [ ] **DoD-3 — Docs:** `docs/unique-eml-import.md` honesty table + flag row; `docs/unique-pst-export.md` nested paragraph (unique-eml consumes the DTO); CHANGELOG Unreleased; `D-0067-embedded-depth` **narrowed** (unique-eml MIME shipped; row **stays open** for matter children). Schema ids not bumped.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`FEATURE` on `crates/dedup-engine` + `crates/pst-dedup-cli` at implement). No HITL required.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p dedup-engine --lib eml_pack
cargo test -p pst-dedup-cli --test unique_eml
cargo test -p pst-dedup-cli --test unique_eml_depth
cargo fmt --all --check
cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Filter names re-verify at execute (`unique_eml_depth` is the locked new integration module unless execute finds a collision). No operator INC* command. Do **not** use a unique-eml helper that injects `--no-attachments` if one appears (0101 lesson).

---

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0067-embedded-depth** | **Absorb unique-eml MIME half.** Narrow on implement: unique-eml nested RFC 5322 + `--max-embedded-depth` shipped. **Do not close** — matter/Relativity child-document extract, 32 MiB, hard cap 8 remain. |
| **D-0067-long-path** | **Decline.** Path budget residual. |
| **D-0067-cloud-attaches** | **Decline.** No invent file bytes. |
| **D-0067-gui-keepset** | **Decline.** GUI unique-pst is primary (0072). |
| **D-0071-also-eml** | **Decline.** Flag accepted-but-ignored. |
| **D-0094-inc-resmoke** | **Decline.** Operator unique-pst HITL. Not this CLI. |
| **D-0100-hn-bitmap-hdr** | **Decline.** Fail-closed until a corpus hits it. |
| **D-0099-attach-crc-job-level** | **Decline.** 0099 declined per-event split. |
| **D-0077-poly-fingerprint** | **Decline.** Later reader track. |
| **D-0079-reader-buffer** | **Decline.** pst-reader buffer polish. |
| **D-0088-usgovcloud-microsoft-tld** | **Decline.** |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not unique-eml nested MIME. |

Med/high never parked here. No BCC-default track. Frontend **0107+**. Do not mint `UniqueEmlClapArgs`.

---

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundary: MIME writer in `dedup-engine::eml_pack`; CLI orchestration + extract call in `pst-dedup-cli`. Do not teach `pst-writer` EML policy. Do not change `pst-reader` nested extract APIs unless a compile break forces a thin glue fix (prefer not).
5. Unique-export: no silent attach/count drops. Method-5 without a DTO is **ledgered skip**, not a labeled MAPI dump. `known_gap` is **not** added.
6. No in-tool ScanPST / CRC repair of evidence.
7. unique-pst `--include-bcc-recipients` default **off** (untouched). unique-eml continues to emit `Bcc:` from `display_bcc` when present.
8. Identity hash depth stays **3**.
9. Per-nest byte budget stays **32 MiB** (`MAX_NESTED_EXPORT_PAYLOAD_BYTES`). Product ceiling stays **8**.
10. Do not implement HNBITMAPHDR.
11. Do not start Hermes Series O in this folder.
12. Do not bump `eml_pack_v1` / `keep_set_v1` schema ids.
13. Soft attach failures never write a fake rfc822 body (H1).

### 10.1 Locked fix (closed)

**Option: extract then reconstruct; skip when extract did not yield a DTO.**

1. unique-eml `--max-embedded-depth` (clap 1–8, default 3) → one effective clamp used for `materialize_nested_for_winner` **and** `EmlWriteOpts.max_embedded_depth`.
2. Write loop: re-materialize winner → nested extract → `write_canonical_eml`.
3. `prepare_one_attach` **method-5 only** (`attach_method == ATTACH_EMBEDDED_MSG`):
   - Run these checks **before** `open_attach_body`.
   - `embedded_extract_limit` or **parent** `depth >= max` → skip + `ATTACH_DEPTH_LIMIT` (dedicated skip variant).
   - `Some(nested)` and `depth < max` → build rfc822 wrapper; body = recursive RFC 5322 from DTO (`unparsed: false`). Do not open the method-5 attach stream.
   - else → skip + `ATTACH_EMBEDDED_UNPARSED` (no stream dump).
4. Method ≠ 5 and mime rfc822 → existing dump path (`open_attach_body` allowed). `is_embedded_message` may still be true; do **not** send this path through the method-5 skip.
5. Nested stream parent = `(winner.source_path, nested.source_msg_nid)`.
6. Inner headers: RFC 5322 fields from DTO only; **always `Subject:`**; no parent Source/Folder X-headers. Nested events: inner subject + inner `attach_index`.

**Declined:** dumping method-5 attach bytes as rfc822 “best effort” when extract fails.

**Declined:** parsing by-value rfc822 into nested DTO this track.

**Declined:** unique-pst BCC suppress on unique-eml.

**Declined:** raising default depth to 8 (same 0101 reason: RAM/time surprise).

**Declined:** closing `D-0067-embedded-depth`.

**Declined:** `EmlPackManifest` required new field.

**Declined:** CLI chain-of-9 (unbuildable).

**Declined:** `UniqueEmlClapArgs` (keep UniqueEml clap in `main.rs`; `pub` parser is enough).

### 10.2 Test fixtures (locked)

Unit (`eml_pack.rs`):

- **Parsed nest (fail on HEAD):** parent + method-5 `embedded_message` with `subject: "Inner subject"`, `sender: "inner@ex.com"`, `body_plain: "inner body"`. Output contains `Content-Type: message/rfc822`, inner `Subject: Inner subject`, inner `From: inner@ex.com`, inner body; **no** base64 CTE on that rfc822 wrapper; `embedded_messages_written == 1`; `embedded_message_unparsed == false`. Assert **no** `X-Pst-Dedupe-Source:` inside the rfc822 body (parent may still have it on the outer message).
- **Nested file child:** inner DTO has a method-1 attach `data: b"Hello"` → inner mixed + `SGVsbG8=`; outer rfc822 wrapper still 8bit.
- **Honesty skip (fail on HEAD):** today’s `embedded_message_rfc822_not_octet_stream` shape (method-5, `data: Some(b"From: x\r\n\r\nbody")`, `embedded_message: None`) → **no** `message/rfc822` part; `attachments_failed == 1`; event `ATTACH_EMBEDDED_UNPARSED`. Rename/replace the old dump test; do not keep a green dump assertion.
- **Method-1 rfc822 dump (mandatory):** `attach_method: Some(1)`, `mime: Some("message/rfc822")`, `data: Some(b"From: x\r\n\r\nbody")`, `embedded_message: None` → rfc822 wrapper + those bytes, 8bit, `unparsed: true`, **no** `ATTACH_EMBEDDED_UNPARSED` event. **Must fail on HEAD** if M3 gating is wrong (today there is no such test).
- **Parent-depth halt (mandatory):** two-level method-5 DTO chain @ `EmlWriteOpts.max_embedded_depth: 1` → nest 1 inner subject present; nest 2 absent; event `ATTACH_DEPTH_LIMIT`. Distinguishes “halt at child depth == max” (would drop nest 1) from “halt at parent depth >= max” (writes nest 1).
- Keep green: `embedded_soft_fail_skips_no_fake_rfc822_body`; `mixed_with_file_attach_base64`.

CLI (`crates/pst-dedup-cli/tests/unique_eml_depth.rs`, locked name):

Reuse `method5_chain` pattern from `unique_pst_depth.rs` (`pst-writer` already a cli dev-dep). Spawn `pst-dedup unique-eml` (not a helper that strips attaches).

- Depth-4 source @ default → `ATTACH_DEPTH_LIMIT` in `{out}/export_attachments.csv` / histogram; 4th nest absent from the `.eml`; `summary.json` `max_embedded_depth == 3`.
- Same source `@ --max-embedded-depth 4` → no depth-limit for that chain; inner depth-4 subject present; `max_embedded_depth == 4`.
- Ceiling: 8-deep source @ 7 → limit; @ 8 → clean for that chain.
- `--max-embedded-depth 0` and `9` (and non-integer) → usage error.
- Existing `unique_eml.rs` fixture tests stay green.

### 10.3 Depth names (do not conflate)

| Name | Owner | Role |
|---|---|---|
| `EmlWriteOpts::max_embedded_depth` / unique-eml `--max-embedded-depth` | eml_pack + unique-eml CLI | **This track’s** extract/write knob, clamp [1, 8], default 3 |
| `WritePstOpts::max_embedded_depth` / unique-pst `--max-embedded-depth` | writer + unique-pst CLI | Already shipped (0101). Do not change. |
| `MAX_EMBEDDED_IDENTITY_DEPTH` | `pst-reader` | 0090 hash recursion, **locked 3** |
| `DEFAULT_MAX_EMBEDDED_DEPTH` | `eml_pack` | Default 3; now **used** (was reserved) |
