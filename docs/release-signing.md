# Windows Authenticode / SmartScreen (track 0062)

## Policy

| Audience | Requirement |
|---|---|
| **Counsel / operator handoff** | All shipped `.exe` files **must** be Authenticode-signed |
| **Engineering / CI local builds** | May be unsigned; must be labeled **unsigned / not for production handoff** |

If a code-signing certificate is **not** available, the RC track may still complete docs, version, gates, and packaging, but **external operator handoff is blocked** (residual **D-0062-codesign**). Do not present an unsigned ZIP as the official counsel RC.

## Signing procedure (template)

Secrets and PFX/cert material live only in CI or a secure store — **never** committed to git.

### Tools

- Windows SDK **`signtool.exe`**
- Timestamp server (example): `http://timestamp.digicert.com` (or org-standard TSA)

### Example (local release desk)

```powershell
# After scripts/package-release.ps1 builds exes into dist\...
$files = @(
  "dist\0.2.0-rc.1\dedupe-desk.exe",
  "dist\0.2.0-rc.1\pst-dedup.exe",
  "dist\0.2.0-rc.1\pst-dedup-gui.exe"
)

foreach ($f in $files) {
  signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a $f
  signtool verify /pa $f
}
```

### Certificate source

| Item | Location |
|---|---|
| Code signing cert | Org secure store / CI secret (e.g. Azure Key Vault, GitHub Actions secret) |
| Who holds keys | Release owner / platform ops |
| Timestamp | Always use a public TSA so signatures remain valid after cert expiry |

### CI (recommended pattern)

1. Build release binaries + PDBs + SBOM via `scripts/package-release.ps1`.
2. Sign exes in a restricted job with access to signing secrets.
3. Publish **operator ZIP** (signed exes + `bom.json` + `README-RELEASE.txt`).
4. Publish **symbols ZIP** (or `symbols/` folder) for support — may remain internal.

## SmartScreen

First-run SmartScreen warnings can still appear for new publishers until reputation accumulates. Signing is required but not always sufficient on day one. Document expected first-run prompts for operators.

## Residual

**D-0062-codesign** — operator handoff blocked until signing cert + procedure are exercised on a real RC ZIP.
