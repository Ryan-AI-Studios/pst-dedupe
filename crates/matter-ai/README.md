# matter-ai

Opt-in **AI provider trait** + **first-pass code suggestions** for Dedupe Desk (tracks **0051** / **0052**).

## Architecture locks

| Rule | Detail |
|---|---|
| AI off by default | Core product works without AI; jobs fail closed when disabled |
| Trait + Mock + OpenAI-compatible | One HTTP shape for local (Ollama / LM Studio) and cloud |
| Remote requires `allow_remote` | No silent cloud fallback |
| Suggestions ≠ final codes | Job writes `item_ai_suggestions` only; human accept → `item_codes` |
| Grounded citations (0052) | Contiguous verbatim quotes + UTF-8 byte offset hints + verify status |
| No splice/ellipsis quotes | Prompt + verify reject non-contiguous splices |
| No hard quote truncate | Soft ~50-word guidance only; SQLite stores full quote |
| Accept audit pointers only | suggestion_id + template/model + offsets — **no** quote cleartext |
| Keys not in SQLite | OS keyring (Desk) + env `PST_DEDUPE_AI_API_KEY` (headless) |
| Full catalog guidance | Prompt embeds operator definitions — no name-only inventing |
| Middle-drop truncation | Head + tail kept when text exceeds cap |
| Skip withheld | Privilege withhold items never sent to the model |
| Mock-only CI | Default tests never hit the network |

## Provider kinds

| Kind | Use |
|---|---|
| `none` | Disabled (default) |
| `mock` | Deterministic JSON for tests / offline demos |
| `openai_compatible` | `POST {base}/v1/chat/completions` |

Loopback hosts (`127.0.0.1`, `localhost`, `::1`) are **local**. Any other host is **remote** and requires `ai_allow_remote = 1`.

**Redirects are not followed** on OpenAI-compatible HTTP calls (`reqwest` redirect policy `none`). A 3xx response is treated as an error so a loopback base URL cannot silently hop to a remote host when `allow_remote` is false.

### Local servers (operator-installed)

| Server | Typical base URL |
|---|---|
| Ollama | `http://127.0.0.1:11434/v1` |
| LM Studio | `http://127.0.0.1:1234/v1` |

Desk does **not** ship or auto-start these daemons.

## Secrets

Resolution order:

1. Env **`PST_DEDUPE_AI_API_KEY`** (headless CLI, CI, services)
2. OS keyring service `dedupe-desk` / user `ai_api_key` (interactive Desk)

Never store API keys in `matter.db` or audit content. Keyring failures fail closed with a clear error (no hang).

## Job `ai_suggest_codes`

```json
{
  "scope": "in_review",
  "max_items": 100,
  "max_text_bytes": 8000,
  "reset": false,
  "temperature": 0.0
}
```

```powershell
# Enable AI (mock) on a matter, then:
.\target\release\pst-dedup.exe job run --path $m --kind ai_suggest_codes --json
```

- Fail if AI disabled
- Skip missing text; skip withheld
- Middle-drop text; full catalog guidance in prompt (`suggest_codes_v2`)
- Robust JSON extract (fence / prose / bare array) including optional `citations[]`
- Verify citations (whitespace/case normalize; re-find on mismatch; cap **count** ≤ 5)
- Write **only** suggestions + citation rows (`pending`); never `item_codes` from the job
- Fingerprint skip when `reset=false` (text + model + template + catalog hash)
- Audit: kind / model / is_remote / template / counts — **no** keys or full bodies

## Human path

- **Accept** → `Matter::accept_ai_suggestion` → `apply_codes` + audit with provenance + citation **offset pointers** (no quote cleartext)
- **Reject** → status `rejected`
- Desk promote panel: citation list + **mandatory** in-doc scroll/highlight on citation click
- Unverified citations show a badge; Accept still allowed (P0 warn) with `citation_unverified` audit flag
- Multi-turn chat / cross-doc RAG → residual (D-0052-*)

## Honesty

- AI is optional; core review works without it
- Local models require an operator-installed OpenAI-compatible server
- Cloud may transmit matter text — privilege and confidentiality risk
- Suggestions hallucinate — human review mandatory; definitions come from **your catalog**
- Citations can be wrong or cherry-picked; human must read **in context** (highlight/scroll)
- Unverified quotes are labeled — not hidden
- Offsets are hints after re-OCR/re-extract; audit stores offsets not cleartext quotes
- Middle-drop truncation may omit mid-document body
- JSON extract is best-effort against chatty models
- Mock is not a real model
- Not a substitute for privilege review
- Not privilege determination

## License

Proprietary commercial (see repository root [`LICENSE`](../../LICENSE)). Runtime deps include `reqwest` (rustls), `keyring`, `matter-core` under their own licenses.
