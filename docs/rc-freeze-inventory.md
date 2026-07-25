# RC freeze inventory — 0.2.0-rc.1 (track 0062)

**Feature freeze:** track **0062** does not implement product features (no new jobs, schema migrations, writer fidelity, service APIs, or Desk Connect).

## Shipped in this RC

| Surface | Status |
|---|---|
| Series A–H matter Desk spine | Completed (prior tracks) |
| Series I platform (0057–0061) | Completed; schema through **v39** |
| Series K clean unique export (0065–0072) | Completed; CLI unique-pst + optional GUI wizard |
| Aligned crate version | **0.2.0-rc.1** |
| CHANGELOG | Present |
| Golden path + mode matrix | `docs/operator-golden-path.md` |
| Operator residual checklist | `docs/operator-rc-checklist.md` |
| `deny.toml` strict licenses | Permissive allow; GPL/AGPL not allowed |
| `cargo audit` / `cargo deny check` | CI + local gates |
| Release `debug = 1` + PDB packaging | Root `Cargo.toml` + package script |
| CycloneDX `bom.json` | Generated into release package |
| Authenticode process | Documented; operator handoff requires sign or D-0062-codesign block |
| track011 archival | Completed (fixture goals met; production writer is 0068+) |

## Explicitly deferred (not RC blockers)

| Item | Owner |
|---|---|
| Security red-team + P0/P1 exploit fixes | **0063** |
| Desk Connect / SSO browser UX / produce profile dropdown | **0064** |
| Operator Outlook / scanpst on unique-pst volumes | D-0068-02 / operator checklist |
| Multi-GB operator soak | D-0070 / D-0071 operator residuals |
| Bundle Tesseract / Whisper | residual packaging |
| Full interactive GUI smoke automation | residual P3s |
| Force-close entire `docs/deferred.md` | inventory only |

## Advisory residuals (documented)

| ID | Item |
|---|---|
| D-0062-audit-rsa | `rsa` Marvin via `openidconnect` (SSO opt-in); no fixed upgrade |
| D-0062-codesign | Blocks **operator-facing** handoff until Authenticode available; engineering gates may complete |

## Tag strategy

- Git tag after merge: **`v0.2.0-rc.1`**
- Rationale: first counsel-facing RC after Series I + Series K; **0.2** (not 1.0) because Outlook/scanpst residual and Connect residual remain honest limits.
