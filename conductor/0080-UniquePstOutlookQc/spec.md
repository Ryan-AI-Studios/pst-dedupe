# 0080 — Unique PST Output QC (source-differential + external validators)

- **Track ID:** 0080-UniquePstOutlookQc
- **Status:** Ready
- **Series:** L
- **Depends on:** 0071 verification block · 0078 exit contract · 0079 `export_oracle`
- **Verified against:** `ce9cfc8` (0079 merged)
- **Revised 2026-07-28:** folded in cross-model review — clean-room re-verification via
  `content_digests.json` (§3.1/§3.4, DoD-21), cloud/modern-attachment blind spot (Q10, §3.2,
  D-0080-cloud-attachments, DoD-22), empty/zero-byte sampling floor (§3.3), BYOB sidecar rule
  (rule 15, §3.5, DoD-12). No locked rule removed or weakened; scope did not shrink.

## 1. Objective

Make the unique-PST deliverable **provably faithful to its sources**, and replace the
self-referential verify block with QC that can fail on a real writer defect.

Today's verification answers *"did we write what we said we wrote?"*. Nobody has ever
answered *"does the output match the source, and can anything but our own reader open it?"*
D-0068-02 has been open since track 0068 and re-raised three times.

| Capability | Today | After 0080 |
|---|---|---|
| Output opens (our reader) | yes | yes |
| Message count vs **own report** | yes | yes |
| Message content vs **source** | **no** | yes (levelled) |
| Attachments present in output | **never checked** | yes, payload-hashed |
| Folder tree vs source | **no** (counts summed only) | yes |
| CC / BCC preserved | **silently dropped** | declared + counted (§3.11) |
| Independent (non-our-reader) proof | **no** | optional sidecar (§3.5) |
| Structural validation by Microsoft's own tool | **no** | optional scanpst `-no repair` (§3.6) |
| Known limitation vs real defect | indistinguishable | machine-readable contract (§3.2) |
| Re-verify a deliverable after sources are archived ("clean room") | **no** — output-only QC is structural-only | payload-level, if `content_digests.json` was persisted at export time (§3.4) |
| Cloud/modern attachments (OneDrive/SharePoint links) | **invisible** — no named-property resolution exists | documented blind spot, not silently `preserved` (Q10, §3.2) |

### Industry anchors (researched 2026-07-28 — this is time-sensitive)

- **Classic Outlook entered its opt-out phase in April 2026** — new Outlook is now the
  default client. Classic retires **Q1–Q2 2028**; **EOL Q2 2029**.
- **New Outlook has no COM/VSTO/VBA object model, and none is planned.** "Automate Outlook
  to open the PST" is not a strategy with a future.
- New Outlook gained PST **import** (March 2026 mail; calendar/contacts rolling out
  mid-2026) — *import*, not *mount*. It still requires classic Outlook installed at the
  same bitness, a requirement Microsoft says it will drop later in 2026. Microsoft has
  stated that once bulk import ships it has **no plans to continue developing .PST support**.
- `scanpst.exe` gained real command-line arguments in **Outlook 2016 v1807
  (16.0.10325.20082)**: `-file`, `-no repair`, `-silent`, `-force`, `-log`, `-backupfile`,
  `-rescan`. **`-no repair` is a read-only validation mode** — this is what makes automated
  structural validation possible at all. Microsoft states plainly: **"There is no support
  for networked .pst files."**
- **`scanpst` reports no exit-code contract.** Microsoft's own guidance is that the user
  monitors the `.log`, the `.bak`, or Task Manager. Treat the process as untrusted.
- libpff (`pffinfo`/`pffexport`, **LGPL-3.0-or-later**) and libpst (`readpst`, GPL) are the
  viable independent readers. Both are **process sidecars only** — never bundled, linked,
  or vendored (§2.3.9).

**Consequence, stated up front:** every external validator available to this track is on a
retirement clock or a copyleft licence. The durable signal must be the **source-differential
reader QC (§3.3)**; scanpst and external readers are corroboration, not foundation.

## 2. Starting position

### 2.1 What exists today (verified, not assumed)

- `verify_volumes` (`unique_pst_cmd.rs:2981`) opens each volume, sums
  `folder.message_nids.len()`, compares to `vol.messages_written`, and samples
  `min(5, N)` export rows against MIDs/subjects.
- It **already reads properties of every message in the volume** (`:3017-3030`) to build
  `written_mids` / `written_subjects` — a Codex r2 P2 fix. Full-scan cost is *already paid*.
- `export_oracle::structural_digest_pst` (`export_oracle.rs:491`) already computes folder
  paths + per-message content digests + **attachment payload SHA-256** — built by 0079,
  currently **test-only**.
- 0079 shipped `PstHandleCache` (bounded LRU, `--max-open-psts` default 32) and
  `PhaseTimings` incl. `verify_ms`.
- 0078 owns the exit contract: `classify_export`, closed `exit_reason` vocabulary
  (`VERIFY_FAILED` already exists), `CliExit::PartialFidelity = 64` / `ExportRiskBlocked = 65`
  / `Cancelled = 130`.
- `docs/pst-writer-fidelity-v1.md` carries a **prose** fidelity matrix — accurate, but not
  machine-readable and not consulted by any check.
- `Qc` is already taken by matter-produce QC (`main.rs:467`). New surface must not collide.

### 2.2 Defects

| # | Defect | Evidence |
|---|---|---|
| **Q1** | **Verification is self-referential.** It compares the output to `vol.messages_written` and `export_rows` — both produced by the same run. A systematic loss is recorded consistently in both and passes. | `unique_pst_cmd.rs:3004`, `:3007-3010` |
| **Q2** | **Attachments are never read back.** The export's most-worked fidelity claim (0069 write, 0073 ledger, 0074 preflight) has no output-side check at all. | `verify_volumes` touches no attachment API |
| **Q3** | **Folder structure is never verified.** 0069 preserves paths under IPM_SUBTREE; verify only *sums* `message_nids` across folders, so a tree collapsed to one folder verifies clean. | `:3003` |
| **Q4** | **Reader/writer circularity.** Output is validated by the reader that shares the writer's assumptions about MS-PST. A mutual misreading is invisible. D-0068-02, open since 0068. | `deferred.md` D-0068-02 |
| **Q5** | **The capability exists and is not exposed.** `structural_digest_pst` does folder + content + attach-payload digests today, reachable only from tests. | `export_oracle.rs:491` |
| **Q6** | **Sample is `take(5)` in export-row order** — not the messages likely to be broken. Nothing targets XBLOCK-boundary bodies, max-attachment messages, oversized string props (the D-0068-01 hard-fail class), volume-split seams, or per-source prefixes. | `:3011`, `:3031` |
| **Q7** | **`display_cc` and `display_bcc` are silently dropped.** `CanonicalMessage` carries both (`keepset.rs:694-695`); `from_canonical_message` maps neither (`production.rs:802-818`); `WriteMessage` has no field for them. The adapter's `dropped` return is hardcoded `0` (`:819`). The fidelity matrix documents "recipient table: No" but never says the **display strings the source did have** are discarded. Nothing counts it; Q1 guarantees nothing catches it. | `keepset.rs:694`, `production.rs:785-819` |
| **Q8** | **Known limitation and real defect are indistinguishable.** Any source→output comparison will diff on recipient tables, RTF, named props and encryption — all deliberate. Without a declared contract, deep QC is red forever and gets ignored. | `docs/pst-writer-fidelity-v1.md` is prose |
| **Q9** | **No operator evidence artifact.** The manual Outlook open is the only thing that has ever validated the writer against a real client, and it is recorded nowhere machine-readable. | D-0068-02 / D-0071-operator-outlook |
| **Q10** | **Cloud/modern attachments are invisible, not just unresolved.** M365 "Share via link" attachments are stored via named properties (e.g. `PidNameAttachmentProviderType`); a message can look fully preserved (link text matches, size matches) while the actual document payload was never in the PST. `pst-reader` has **no named-property resolution at all** — grep for `PidName`/`NamedProp` across the crate returns zero matches — so this isn't a QC gap in the usual sense, it's a reader capability gap QC cannot check around. | verified: no `PidName`/`NamedProp` symbol anywhere in `crates/pst-reader/src` |

### 2.3 Locked rules

1. **Sources stay read-only.** QC re-opens source PSTs; it never writes to them.
2. **QC never repairs.** `-no repair` is mandatory; repair mode is never invoked, on any file, under any flag.
3. **Never validate the deliverable in place.** External validators run on a **copy in a local temp dir** — Microsoft states networked `.pst` is unsupported, and the deliverable's recorded `sha256_hex` must remain true.
4. **A `.bak` next to the copy is a hard QC error**, not a warning: it proves repair ran.
5. **No new exit integers.** QC folds into 0078's `classify_export`; hard findings surface as the existing `VERIFY_FAILED`.
6. **QC may not lower an exit** that another condition already set.
7. **Contract-classified `known_gap` never fails the build.** Only `defect` and `unexplained_loss` do (§3.2).
8. **External validators are skip-safe and never required.** Absent tool ⇒ `skipped` + reason, never a failure, never a hang.
9. **No copyleft in the binary.** libpff/libpst are invoked as processes only — never bundled, linked, vendored, or added as a Cargo dependency.
10. **No COM.** Declined with reasons in §3.9; recorded so it is not re-proposed as free.
11. **Untrusted child processes.** Every external tool gets a hard timeout, a kill path, and log-based result parsing. Exit codes are corroboration, never the contract.
12. **Determinism.** The risk-weighted sample is a pure function of the keep-set — two QC runs over one export pick the same messages.
13. **Attestation is human-signed.** The tool never self-attests that a human opened the file in Outlook.
14. **QC additions must not flip green pipelines red on a known gap** — the shipped contract must classify every current gap before QC affects exits (§3.10).
15. **Sidecars are Bring-Your-Own-Binary.** The tool never auto-downloads, fetches, or installs `pffinfo`/`readpst`. Only an operator-supplied absolute path is accepted; missing or invalid ⇒ `skipped` + reason (rule 8), never a network fetch. Auto-fetching a GPL/LGPL binary alongside proprietary software is a distribution-clause hazard in enterprise eDiscovery environments, not just a convenience question.

### 2.4 Rolled-in deferred

| ID | Disposition |
|---|---|
| **D-0068-02** | **Headline.** Automatable half closed by §3.6 (scanpst `-no repair`); human half by §3.7 attestation. Carried through 0069/0070 — closes for all three. |
| **D-0071-operator-outlook** | Same family; closed by §3.6 + §3.7 across multi-volume output. |
| **D-0074-e2e-fixture** | The QC fixture matrix (§8) *is* the production-path E2E matrix. Closed. |
| **D-0068-01** | **Narrowed, not closed.** §3.3 sampling must include the longest-subject message so the oversized-non-body-string hard-fail class is exercised, not just theorised. |
| **D-0067-embedded-depth** | **Narrowed.** QC counts embedded messages and cross-checks `embedded_message_unparsed` / `embedded_depth_limit_hits` against the output. |
| **D-0070-multi-source-stream-prefix** | **Narrowed.** Per-source sampling makes the "prefix from sources seen so far" residual observable instead of theoretical. |
| **D-0079-operator-multigb** / **D-0070-operator-multigb** | **Supported, not closed.** 0080 gives the operator run an evidence artifact worth producing. Still needs real PSTs, still cannot be CI. |
| **D-0062-audit-warnings** | Untouched — §3.5/§3.6 add **no** Cargo dependencies. |

## 3. Design

### 3.1 `qc_report_v1` artifact

New JSON under the report dir, plus `qc_findings.csv` for row-level detail. Shape:

```jsonc
{
  "schema": "qc_report_v1",
  "contract": "fidelity_contract_v1",
  "qc_level": "sample",
  "volumes": [ /* per-volume: open_ok, folder tree match, counts, digests compared */ ],
  "messages_compared": 0,
  "attachments_compared": 0,
  "findings": { "defect": 0, "unexplained_loss": 0, "known_gap": 0, "explained": 0 },
  "external": {
    "independent_reader": { "status": "skipped", "reason": "…", "tool": null, "version": null },
    "scanpst":            { "status": "skipped", "reason": "…", "build": null, "log_path": null }
  },
  "attestation": null,
  "qc_ms": 0
}
```

`qc_ms` joins 0079's `PhaseTimings` (data path, not log path — 0077 rule 2 / 0078 rule 8).

**`content_digests.json`** (sibling artifact, written when QC ran at export time): the source-side
`structural_digest_pst` output — per-message content digest + per-attachment payload SHA-256 —
persisted into the report directory instead of discarded after the run. Granularity matches
whatever `--qc-level` was requested at export time (`sample` persists digests only for the
risk-weighted set; `full` persists all). This is what makes payload-level re-verification
possible after the source PSTs are archived or moved out of reach (§3.4) — without it, a
"clean room" re-check can only ever be structural.

### 3.2 `fidelity_contract_v1` — the classifier

A machine-readable declaration of what unique-pst preserves, derived from
`docs/pst-writer-fidelity-v1.md`. Each property is `preserved`, `best_effort`, or
`dropped_by_design` (with a reason string). Every source→output difference is classified:

| Contract says | Difference observed | Classification | Fails? |
|---|---|---|---|
| `preserved` | differs / missing | **`defect`** | yes |
| `best_effort` | missing **with** a 0073 ledger row or 0065/0075 fidelity flag explaining it | `explained` | no |
| `best_effort` | missing with **no** explanation | **`unexplained_loss`** | yes |
| `dropped_by_design` | missing | `known_gap` (counted, reported) | no |
| *(absent from contract)* | anything | **`unexplained_loss`** | yes |

The last row is the point of the whole design: **the contract is an allowlist, not a
denylist.** A property nobody thought about fails closed. `known_gap` counts are printed in
the summary so a "known" gap can never quietly grow.

The contract is versioned. Changing a `preserved` → `dropped_by_design` is a fidelity
regression and requires a version bump plus a `review.md` entry — it is the one edit that
can silence a real defect, so it must be expensive.

**Cloud/modern attachments (Q10) get an explicit contract line, not silence.** Since
`pst-reader` has no named-property resolution, the contract cannot classify
`PidNameAttachmentProviderType` the normal way — it can't detect the property to classify it.
The contract entry instead documents the blind spot in words: *"cloud-attachment link
attachments: reader does not resolve named properties; a message referencing a OneDrive/
SharePoint file cannot be distinguished from an ordinary small attachment; payload
completeness for such messages is unverified, not confirmed."* This keeps the allowlist
honest — the alternative, saying nothing, would let a message with a cloud-attachment link
pass every check while its actual document was never in the PST.

### 3.3 Reader QC (Tier A — always available, the durable signal)

Surface: `--qc-level off|structure|sample|full` on `unique-pst`, plus a standalone
`pst-dedup qc-pst <out.pst> --report-dir <dir>` for re-running QC on an existing pack
(`qc-pst`, **not** `qc` — `Qc` is matter-produce, `main.rs:467`).

| Level | Does | Cost |
|---|---|---|
| `structure` | open each volume; **compare the folder tree** to the keep-set's expected tree (Q3); per-folder counts, not just the sum | ~ what verify already pays |
| `sample` *(default)* | `structure` + full source↔output comparison of the risk-weighted set | + bounded reads |
| `full` | `structure` + every message compared | O(export) |

Comparison reuses `structural_digest_pst`'s per-message digest (Q5 — **promote it out of
test-only, do not write a second one**), extended with recipients (§3.11).

**Risk-weighted, deterministic sampling** (Q6). Sort by keep-set index; from each stratum
take the extremum, dedupe, cap at `--qc-sample-max` (default 64). Strata:

- largest `body_plain` and largest `body_html` (XBLOCK / XXBLOCK boundaries)
- **smallest / zero-byte `body_plain`** — the floor, not just the ceiling: a parser that
  silently fails on a malformed or orphaned item (corrupted calendar invite, stubbed
  Enterprise Vault shortcut) produces a "ghost" message with no body and no subject, and
  extreme-only sampling never looks there
- most attachments; largest single attachment; any zero-byte attachment
- longest subject and longest sender/display string (**D-0068-01** hard-fail class)
- any message with a non-empty `degraded_reasons` set, and any with a 0073 ledger row
- non-ASCII subject (Unicode heap paths)
- **first and last message written in each volume** (multi-volume split seam)
- **one message per distinct source PST** (multi-source prefix, D-0070-multi-source-stream-prefix)
- any message with an embedded attachment (D-0067-embedded-depth)

### 3.4 Source-differential discipline

QC re-opens the **source** and compares against it — never against the same run's report
(Q1). Notes:

- Reuse 0079's `PstHandleCache`; QC must not open a second unbounded set of handles.
- Sources are opened read-only (rule 1).
- If a source is unreachable at QC time (removable media, SMB drop), that message is
  `skipped_source_unavailable` — reported, **not** silently passed and **not** a defect.
- `qc-pst` standalone can run in output-only mode when sources are gone; it then reports
  `source_differential: false` and cannot emit `defect` — only structural findings. This
  distinction must be visible in the artifact, because an output-only QC that *looks* green
  is exactly the false comfort this track exists to remove.
- **Exception — the clean-room case:** if `content_digests.json` (§3.1) is present beside the
  report, output-only `qc-pst` uses it in place of a live source re-read and *can* emit
  `defect`/`unexplained_loss` at payload granularity, still tagged `source_differential:
  false` (it's a persisted proxy, not a live source) but with `content_digest_backed: true`
  in the artifact. Without that file, output-only QC stays structural-only — it must never
  silently upgrade its own confidence.

### 3.5 Tier B — independent reader cross-check (optional sidecar)

`--qc-external-reader <path>` pointing at `pffinfo` (libpff) or `readpst` (libpst).
Pattern is `ocr-plugin`'s Tesseract sidecar: operator installs, path supplied, absent ⇒
skip with a reason (rule 8). Licence posture is rule 9 — **process invocation only**, and
rule 15 — **BYOB**: the flag accepts a path, never a URL; the tool never fetches the binary
itself, even if the operator would clearly consent. An enterprise legal-review environment
treats "the tool downloaded a GPL binary" as a distribution event regardless of intent.

Compare **item counts and folder counts only**. Do not attempt content equality: these
tools' text extraction differs from ours by design, and a content diff would produce noise
that gets the whole check disabled. Counts are what breaks the reader/writer circularity
(Q4), and counts are what these tools report stably across versions.

### 3.6 Tier C — scanpst structural validation (optional, Windows, classic Outlook)

```
scanpst.exe -file <local-temp-copy.pst> -no repair -silent -log replace
```

This is the only mechanism that validates our output against **Microsoft's own** notion of
a well-formed PST. Rules, each traceable to a documented fact:

1. **Copy to a local temp dir first** — Microsoft: *"There is no support for networked .pst
   files."* Also keeps the deliverable's recorded hash true (rule 3).
2. **`-no repair` is mandatory.** Microsoft's doc: if **none** of the new arguments are
   used, *the legacy code path runs* — and the legacy path repairs. A typo'd flag silently
   becomes a repair. Therefore: **verify the exact argument token against the installed
   build; if it cannot be verified, skip.** Never guess (rule 2). Microsoft's own page
   prints it as `-no repair` in one table and in the enabling list; treat the spelling as
   *unconfirmed* until an operator run shows the log honouring it.
3. **A `.bak` appearing next to the copy is a hard QC error** (rule 4) — it proves repair
   ran. Quarantine the copy, fail the scanpst check, keep the deliverable untouched.
4. **Parse the log, not the exit code** (rule 11) — Microsoft documents no exit contract and
   tells users to watch the `.log`/`.bak`/Task Manager.
5. **Hard timeout + kill** ⇒ `SCANPST_TIMEOUT`. `-silent` on a build that ignores it leaves
   a GUI waiting on a human forever; that must not hang an export.
6. **Discover the path**; never hardcode `Office16`. Probe the Click-to-Run roots and the
   registry, record what was found.
7. **Skip on builds < 16.0.10325.20082** with a recorded reason — pre-1807 is GUI-only and
   would hang.
8. **Skip when only new Outlook is present** — it does not ship scanpst.

### 3.7 Operator attestation (`qc_attestation_v1`)

A signed-by-a-human block appended to the pack recording the manual client open: tool and
version (classic Outlook 2019 / new Outlook import / third-party), date, operator, volumes
opened, messages seen, whether an attachment was opened successfully, free-text notes.

The tool never writes this itself (rule 13). This is what actually closes the human half of
D-0068-02 — replacing "someone opened it once" with evidence attached to a specific export.

### 3.8 Client-retirement honesty

`docs/unique-pst-export.md` gains a short, dated section: classic Outlook is opt-out-default
since April 2026, retires Q1–Q2 2028, EOL Q2 2029; new Outlook offers **import, not mount**,
has no COM, and Microsoft has said PST support stops developing after bulk import ships.

Two consequences to write down, because 0081's runbook depends on both: Tier C has a shelf
life, and "open it in Outlook to check" will stop being available to operators before this
product stops emitting PSTs. **PST remains a correct deliverable format** — Purview exports
it and eDiscovery consumes it — but our *proof* strategy cannot lean on Microsoft's client.

### 3.9 Outlook COM — declined

The placeholder spec proposed `--outlook-com-smoke` (create a temporary profile, open the
data file). Declined on three independent grounds:

1. **It has no future and barely has a present.** New Outlook — default since April 2026 —
   has no COM/VSTO/VBA object model and none is planned. Classic retires 2028. Building
   `windows`-crate COM automation in 2026 H2 buys a path that is already off by default.
2. **It mutates the operator's environment.** Adding a store to an Outlook profile touches a
   live client on a machine holding real client mail. QC must not have side effects there.
3. **scanpst gives a strictly better signal for a fraction of the machinery.** `-no repair`
   validates the file *format* against Microsoft's own validator — a process invocation with
   no `unsafe`, no FFI, and no new dependency, versus COM automation whose panic paths would
   have to be reconciled with the repo's no-`unwrap`/no-`expect` production rule.

Recorded as **D-0080-com-declined** so it is not re-raised as a free win.

### 3.10 Exit contract (0078 compatibility)

QC hard findings (`defect`, `unexplained_loss`) set `verify_ok = false` ⇒ existing
`VERIFY_FAILED` reason ⇒ exit **1**. No new integers (rule 5).

The compatibility hazard is real and is rule 14: `verify_ok` is a **hard** input to
`classify_export` (`export_outcome.rs:173`), so any newly-detected difference turns a
previously-green export into exit 1. Therefore the ordering is fixed:

1. Author `fidelity_contract_v1` to cover every current gap (§3.2), Q7 included.
2. Prove the whole fixture matrix runs **zero `defect` / zero `unexplained_loss`** at
   `--qc-level full`.
3. Only then may `sample` be the default.

If a fixture shows a defect that is genuinely a known gap, **the contract is wrong and gets
fixed** — never the exit mapping, and never by demoting the finding.

### 3.11 CC / BCC (Q7) — split decision

- **CC:** add `PidTagDisplayCc` to `WriteMessage` and the adapter. `display_cc` already sits
  in `CanonicalMessage`; CC is a standard produced field (0040's DAT emits it); dropping it
  from a legal deliverable while shipping a QC track that reports the drop would be
  perverse. Contract marks it `preserved`. Contract version bumps.
- **BCC:** **declare, do not auto-add.** Writing BCC into a deliverable can disclose
  recipients that the custodian's own copy would reveal — a disclosure decision, not a
  writer bug. 0047 already defaults to visible-only (to+cc). Contract marks it
  `dropped_by_design` with that reason; QC counts it as a `known_gap`; an opt-in flag is a
  **product** decision recorded as **D-0080-bcc-policy**.
- The adapter's reserved `dropped` return (`production.rs:819`, always `0`) is the natural
  place to start counting genuinely-unmappable fields.

## 4. Out of scope

- Full MAPI property parity audit (recipient **table** remains D-0068-04 / **D-0080-recipient-table**).
- GUI wizard wiring for QC (thin, later — D-0073-gui family).
- Repairing anything, ever.
- Bundling or linking libpff / libpst.
- Outlook COM (§3.9).
- Making the writer byte-reproducible (D-0079-deterministic-key — product).

### New residuals

| ID | Item |
|---|---|
| **D-0080-com-declined** | Outlook COM automation declined with reasons (§3.9); revisit only if Microsoft ships an automation surface for new Outlook. |
| **D-0080-recipient-table** | Real recipient table (SMTP + `PidTagRecipientType`); §3.11 display strings are the stopgap. Joins D-0068-04 / D-0076-recipient-table. |
| **D-0080-bcc-policy** | Whether unique-pst should ever write BCC — product/disclosure decision. |
| **D-0080-scanpst-arg** | Exact `-no repair` token + `-silent` behaviour confirmed on real Outlook builds; until then §3.6 rule 2 skips rather than guesses. |
| **D-0080-external-reader-matrix** | Which libpff/libpst versions were actually validated against our output. |
| **D-0080-newoutlook** | No mount/automation successor once classic Outlook retires; revisit when Microsoft's bulk PST import ships. |
| **D-0080-cloud-attachments** | (Q10) `pst-reader` has no named-property resolution; cannot detect or count cloud-attachment (OneDrive/SharePoint link) payload gaps. Documented as an explicit blind spot in `fidelity_contract_v1` rather than silently passed. Real fix needs named-prop resolution — likely its own track. |

## 5. Preconditions

- 0071 verify block, 0078 `classify_export`, 0079 `export_oracle` + `PstHandleCache` — all merged at `ce9cfc8`.
- Synthetic fixtures only. Real multi-mailbox PSTs stay operator-local and are never `git add`ed.
- scanpst / external readers are **operator-local**; CI never requires either.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Deep QC flips green pipelines to exit 1 | §3.10 ordering + rule 14; contract authored before default-on |
| Contract becomes a dumping ground that hides defects | Allowlist semantics (§3.2 last row); `known_gap` counts printed; version bump + `review.md` to widen |
| scanpst silently repairs the deliverable | Copy-first (rule 3), `-no repair` verified-or-skip (rule 2), `.bak` = hard error (rule 4) |
| scanpst hangs on a GUI-only build | Build gate + hard timeout + kill (rule 11, §3.6.5/7) |
| External reader diff produces noise, check gets disabled | Counts only, never content (§3.5) |
| QC doubles export wall time | Levelled; `structure` ≈ today's cost (verify already full-scans); `qc_ms` reported separately |
| Output-only QC mistaken for source-differential proof | `source_differential: false` in the artifact; cannot emit `defect` (§3.4) |
| Copyleft contamination | Rule 9 — process invocation only, zero Cargo deps added |
| `content_digests.json` missing/stale for older exports | Output-only QC falls back to structural-only; `content_digest_backed` flag never silently assumed true (§3.4) |
| Cloud-attachment payload gaps invisible, mistaken for `preserved` | Explicit contract line + D-0080-cloud-attachments residual (Q10, §3.2) — documented, not resolved |
| Sidecar auto-fetch treated as a distribution event by legal review | Rule 15 — BYOB, path only, never a URL (§3.5) |

## 7. Definition of Done

- [ ] **DoD-1** `fidelity_contract_v1` machine-readable, derived from `pst-writer-fidelity-v1.md`, covering every property the writer touches; unknown property ⇒ `unexplained_loss`
- [ ] **DoD-2** `qc_report_v1` + `qc_findings.csv` in the report pack; `qc_ms` in `PhaseTimings`
- [ ] **DoD-3** `--qc-level off|structure|sample|full` on `unique-pst`; standalone `pst-dedup qc-pst`
- [ ] **DoD-4** **Folder tree** compared to expected, not just summed counts (Q3)
- [ ] **DoD-5** **Attachments read back** from output and payload-hashed against source (Q2)
- [ ] **DoD-6** Comparison is against **source**, not the run's own report (Q1); `source_differential` flag honest (§3.4)
- [ ] **DoD-7** `structural_digest_pst` promoted out of test-only and **reused**, not duplicated (Q5)
- [ ] **DoD-8** Risk-weighted deterministic sampling with every §3.3 stratum; test asserts identical selection across two runs (rule 12)
- [ ] **DoD-9** Negative tests: a deliberately corrupted/short-changed output PST **fails** QC — one per finding class (`defect`, `unexplained_loss`), built at test time via `pst-writer` + byte edits (the 0077 `crc_integrity_0077` pattern), never from real files
- [ ] **DoD-10** `known_gap` never fails; `defect`/`unexplained_loss` always do; QC never lowers an exit (rules 6, 7)
- [ ] **DoD-11** Contract-before-default-on proven: zero `defect`/`unexplained_loss` at `--qc-level full` across the whole fixture matrix (§3.10)
- [ ] **DoD-12** External reader sidecar: skip-safe, counts-only, no Cargo dep, **no auto-download/vendoring (BYOB, rule 15)**, licence note in `review.md`
- [ ] **DoD-13** scanpst runner: local copy, `-no repair` verified-or-skip, log-parsed, timeout+kill, `.bak` ⇒ hard error, build/version gate — each with a test using a **stub executable**, no Outlook needed in CI
- [ ] **DoD-14** `qc_attestation_v1` recordable; never self-attested; documented in the runbook handoff to 0081
- [ ] **DoD-15** §3.11 CC shipped as `preserved` (contract version bump); BCC declared `dropped_by_design`; both covered by QC tests
- [ ] **DoD-16** `docs/unique-pst-export.md` client-retirement section (§3.8), dated
- [ ] **DoD-17** `deferred.md`: **D-0068-02**, **D-0071-operator-outlook**, **D-0074-e2e-fixture** closed; D-0068-01 / D-0067-embedded-depth / D-0070-multi-source-stream-prefix narrowed; D-0080-* added
- [ ] **DoD-18** `conductor.md` + `sequencing.md` rows updated
- [ ] **DoD-19** Operator smoke recorded in `review.md` — scanpst result **or** an explicit "absent, reason" (never a silent gap)
- [ ] **DoD-20** `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`
- [ ] **DoD-21** `content_digests.json` persisted at export time when QC ran; output-only `qc-pst` uses it for payload-level, `defect`-capable verification when present (`content_digest_backed: true`), falls back to structural-only otherwise — never silently upgrades its own confidence (§3.4)
- [ ] **DoD-22** Cloud/modern-attachment blind spot (Q10) has an explicit `fidelity_contract_v1` entry and a D-0080-cloud-attachments residual — never silently `preserved`

## 8. Verification

- Fixture matrix (synthetic only): single-volume, multi-volume split, multi-source prefix,
  attachments incl. zero-byte and XBLOCK-sized, embedded message, non-ASCII subject,
  oversized subject, degraded/soft-fail messages, zero-winner export.
- Negative fixtures per DoD-9, generated at test time.
- Stub-executable tests for both sidecars (DoD-12/13) — CI needs neither Outlook nor libpff.
- Operator-local: multi-GB run with `--qc-level sample` + scanpst; feeds D-0079-operator-multigb.

## 9. Handoff

**Do:** treat Tier A as the durable proof and the external tiers as corroboration; keep the
contract an allowlist; make every skip carry a reason; date anything that depends on
Microsoft's client roadmap.

**Do not:** run repair on any file; validate the deliverable in place; add a Cargo dependency
on libpff/libpst; auto-download a sidecar binary under any flag; add a new exit integer; let
`known_gap` grow silently; claim Outlook compatibility from a scanpst pass alone; claim
clean-room re-verification without `content_digests.json` present; claim cloud-attachment
payload completeness — named-property resolution doesn't exist yet; ship default-on QC
before the contract is complete.
