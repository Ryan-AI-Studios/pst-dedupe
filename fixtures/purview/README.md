# Synthetic Purview fixtures

Test-only synthetic package layout. **No client mail.**

`sample_package/` is a Purview-ish directory used for manual smoke and as a reference layout; automated tests also generate equivalent packages under `tempfile` dirs.

| File | Role |
|---|---|
| `mail.pst` | Dummy bytes with `!BDN` magic (not a real PST) |
| `files.zip` | Nested `inner.zip` + text/eml leaves |
| `ExportSummary.csv` | Export noise for `purview_package` heuristic |
