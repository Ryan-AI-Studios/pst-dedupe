# Fixtures

**Synthetic / sample data only.** Safe to commit. Used by automated tests and light smokes.

## Policy

| In this directory | Not in this repository |
|---|---|
| Small Aspose/sample Unicode PSTs | Real case PSTs (e.g. multi-GB Desktop exports) |
| `purview/sample_package/` synthetic layout | Client Purview packages |
| Generated test zips in unit tests | Matter DBs with client mail |

Real multi-mailbox PSTs are valuable for **manual** CLI/Desk smoke. Keep them on a **local path outside the repo** (Desktop, encrypted volume). Point `dedupe-desk` / `pst-dedup` at absolute paths. Do not copy them under `fixtures/`, `evidence/`, or `output/` if you might `git add -A` without care — prefer Desktop + gitignored `output/`.

See [`conductor/ROADMAP.md`](../conductor/ROADMAP.md) (Evidence & fixtures policy).

## Layout

| Path | Role |
|---|---|
| `*.pst` | Small Unicode samples for `pst-reader` / extract tests |
| `pdf/` | Synthetic PDFs for `extract-pdf` (must remain binary — see below) |
| `office/` | Synthetic OOXML for `extract-office` |
| `calendar/` | Synthetic ICS for `extract-calendar` |
| `purview/` | Synthetic Purview-ish package (see `purview/README.md`) |

## Binary fixtures and line endings

Repo root [`.gitattributes`](../.gitattributes) marks `*.pdf`, `*.pst`, OOXML, and archives as **`binary`**.

**Why:** synthetic PDFs often have no `NUL` bytes, so Git may treat them as text. With Windows `core.autocrlf=true` (default + GitHub `windows-latest`), checkout can insert CR bytes, break PDF xref offsets, and fail CI with `invalid file trailer` even when local tests (LF working tree) pass.

Do not strip those attributes; regenerate PDFs with `cargo run -p extract-pdf --example gen_pdf_fixtures` if needed.

## Creating matters for local smoke

```powershell
# Example only — paths on your machine
.\target\release\dedupe-desk.exe
# Create matter under: C:\dev\dedupe\output\matters\smoke1
# Add PST from: C:\Users\<you>\Desktop\YourCase.pst   (not in git)
```
