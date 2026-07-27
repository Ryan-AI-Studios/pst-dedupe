# 0078 — Unique Export Exit Codes & Automation Contract

- **Track ID:** 0078-UniqueExportExitCodes
- **Status:** Ready
- **Series:** L

## 1. Objective

Give scripts a **stable multi-outcome contract**: full success vs message-complete-but-attach-partial vs hard fail — without treating exit 1 as “no PST written.”

## 2. Context

- INC unique-pst: exit **1**, `ok: false`, but PST has all **3728** messages and verification open_ok.
- CLI already documents some matter exit codes (0/1/2/3/4/5); unique-pst attach soft-fail uses generic failure.
- Automation best practice: separate **process success**, **data completeness**, **fidelity**.

## 3. In scope

1. Document and implement exit codes for unique-pst / unique-eml / keep-set:
   - `0` — complete fidelity (or policy-allowed soft residuals only if flagged)
   - `3` — **partial fidelity** (messages complete; attach/body soft-fail) — PST retained
   - `4` — hard fail (no usable volume / count mismatch)
   - keep existing usage/busy codes if overlapping — document matrix
2. JSON always includes `ok`, `fidelity: complete|partial|failed`, `exit_code`.
3. Optional `--fail-on-partial-fidelity` (default true for back-compat) vs `--allow-partial-fidelity` → exit 0 with fidelity=partial.
4. Tests for each exit class.

## 4. Out of scope

- Changing matter service HTTP status codes (unless shared helper).

## 5. DoD

- [ ] Documented matrix in README / operator-golden-path
- [ ] INC-like partial attach maps to exit 3 (or 0 with allow flag)
- [ ] Hard fail still non-zero without artifact lies
- [ ] review.md
