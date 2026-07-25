# Operator residual checklist (RC 0.2.0-rc.1)

These steps are **operator / soak** work — **not** automated CI. Synthetic fixtures stay in-repo; real client PSTs never enter git.

Mark each row when complete for a given handoff environment.

---

## A. Package smoke (engineering or ops)

| # | Step | Pass criteria |
|---|---|---|
| A1 | Unzip operator package (signed preferred) | Three exes present: `dedupe-desk.exe`, `pst-dedup.exe`, `pst-dedup-gui.exe` |
| A2 | SBOMs present | `bom.json` / `bom-cli.json` + `bom-desk.json` + `bom-gui.json` (CycloneDX; Desk/GUI graphs include egui stack) |
| A3 | `README-RELEASE.txt` present | Version + golden-path pointer match this RC |
| A4 | CLI starts | `.\pst-dedup.exe --help` exits 0 |
| A5 | Unique-pst help | `.\pst-dedup.exe unique-pst --help` exits 0 |
| A6 | Desk starts | `dedupe-desk.exe` launches without immediate crash (close after splash) |
| A7 | GUI starts | `pst-dedup-gui.exe` launches without immediate crash |
| A8 | Authenticode | Exes show valid signature **or** package is labeled engineering-unsigned (not for counsel handoff) |

---

## B. Unique-PST small fixture

| # | Step | Pass criteria |
|---|---|---|
| B1 | Run `unique-pst` on a small synthetic / operator-local PST | Exit 0; volume at `--out` (multi-volume: `{stem}_vol002.pst`, …) |
| B2 | Open report pack | Under `--report-dir` (default: sibling of out stem + `_report`); digests present |
| B3 | Optional rehash | `--overwrite --verify-hash` on a re-run when rehashing content is needed |

---

## C. Multi-GB / multi-volume (optional scale)

| # | Step | Pass criteria |
|---|---|---|
| C1 | Operator-local multi-GB input (not committed) | Completes or clean stop_and_finalize under size policy |
| C2 | Disk headroom | Out dir + temp have sufficient free space; note duration |
| C3 | Multi-volume | If size cap triggers, multiple volume files + consistent report |

Residual: **D-0070-operator-multigb**, **D-0071-operator-outlook**.

---

## D. Outlook / scanpst (optional structural)

| # | Step | Pass criteria |
|---|---|---|
| D1 | Open a unique-pst volume in Outlook | Opens; folders/messages visible enough for smoke |
| D2 | Inbox Repair Tool (`scanpst.exe`) | No critical structural repair required (record result) |

Residual: **D-0068-02**. Until D1/D2 pass in *your* environment, do **not** claim “Outlook production-ready.”

---

## E. Matter golden path (synthetic package)

| # | Step | Pass criteria |
|---|---|---|
| E1 | Create matter in Desk or CLI | `SCHEMA_VERSION` 39 after open |
| E2 | Ingest synthetic package / fixture path | Job completes |
| E3 | Extract + promote path | Items appear in review |
| E4 | Produce small volume | DAT + natives + text layout present |

Day-1 narrative: [`operator-golden-path.md`](operator-golden-path.md).

---

## F. External optional tools

| # | Step | Pass criteria |
|---|---|---|
| F1 | OCR (if needed) | Tesseract installed; Settings path works; one `ocr` job succeeds |
| F2 | STT (if needed) | Whisper CLI path configured; one transcribe smoke |

Not bundled in RC ZIP.

---

## Record

Environment: _______________  
Operator: _______________  
Date: _______________  
RC tag: `v0.2.0-rc.1`  
Notes: _______________
