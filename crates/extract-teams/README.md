# extract-teams

Offline **Teams / chat export adapters** for Dedupe Desk (track **0055**).

Normalizes **local export packages already on disk** (HTML, JSON, PST-shaped
mail items) into Normalized Items with chat metadata and **plain-text** review
bodies. Category: `chat`. Job: `teams_extract`.

## Honesty / limits

- Adapter normalizes **exports on disk**, not live Teams. **No Graph API.**
- HTML vs PST exports differ; completeness depends on collection options.
- Private/shared channel mailbox mapping is a **collection** concern — adapter
  only sees package content.
- **`conversation_id` is day-bucketed (UTC)** — multi-day channels appear as
  multiple conversation groups by design (RSMF-like reviewability).
- Reactions preserved **when present in export** as `[Reaction:…]` lines;
  incomplete exports still possible.
- Attachments: **filename/URL visibility** in text; physical SharePoint resolve
  may be residual. PST injects filenames from matter child items
  (`role=attachment`, `title`/`subject`/path leaf) as `[Attachment: …]`.
- Review body is **plain text** (XSS-safe via **ammonia**); not full HTML fidelity.
- Not Relativity conversation review parity (**0056** owns conversation UI).
- **Synthetic fixtures only** in git (`fixtures/teams/`).
- **No silent message truncation:** if a file has more messages than
  `max_messages_per_file`, the leaf is marked `teams_extract_status=error` with
  code `max_messages_exceeded` (no partial success; no children created for that
  leaf). Raise the cap and re-run with `force`/`reset` to process larger exports.
- **PST CAS honesty:** when `text_sha256` is set but CAS open/read/UTF-8 fails,
  the item is `error` (`teams_cas_error` / `teams_utf8_error`), not a silent
  subject-only success. When `text_sha256` is absent, subject-only plain body is
  an allowed documented fallback and status may still be `ok`.

## Skip vs error semantics

| Situation | Status | `item_error`? |
|---|---|---|
| Non-Teams HTML (no conversation markers) | `skipped` (`not_teams_html`) | **No** |
| Non-Teams / random JSON config | `skipped` (`not_teams_json`) | **No** |
| Format disabled in params / not a candidate format | `skipped` | **No** |
| Teams-shaped HTML/JSON that fails parse | `error` (`teams_parse_error`) | **Yes** |
| Message count > `max_messages_per_file` | `error` (`max_messages_exceeded`) | **Yes** |
| HTML/JSON CAS size over `max_html_bytes` | `error` (limit) | **Yes** |
| PST `text_sha256` CAS/UTF-8 failure | `error` | **Yes** |
| PST not Teams-shaped after detect | `skipped` (`teams_not_teams`) | **No** |

## `conversation_id` hash (frozen)

```text
components joined by "\0":
  team_key (or empty)
  channel_or_chat_key (channel name / chat id / path parent folder / "unknown")
  bucket_date (YYYY-MM-DD UTC of sent_at, or "unknown")
  thread_key_if_any (parent/export thread id or empty)

conversation_id = lowercase hex sha256 of UTF-8 bytes of joined string
conversation_bucket_date = bucket_date string (denorm)
```

### Identity key honesty

- **HTML:** `data-team` / `data-channel` on the conversation container.
- **JSON:** team/channel fields when present; otherwise `conversationId` /
  `conversation_id` / `chatId` / `chat_id` become `channel_or_chat_key` so
  distinct 1:1/group chats on the same UTC day do not collide.
- **PST:** team/channel from path under `Team Chat` / `Conversation History`
  (`…/Team Chat/<team>/<channel>/…`). When those are empty, **path parent
  folder name** (folder containing the message leaf) is used as the chat key,
  then `message_class` as a last resort — never intentionally hash every chat
  to bare `"unknown"` when a stable path segment exists.

## chat_type

`one_to_one` | `group` | `channel` | `meeting` | `unknown`

Aliases normalized (case-insensitive): `1:1` / `dm` / `direct` → `one_to_one`;
`team` → `channel`; `groupchat` → `group`; etc. Unrecognized → `unknown`.

PST derives type from `message_class` / path when possible (`Team Chat` →
`channel`).

## chat_export_format

`pst` | `html` | `json`

## Synthetic HTML layout (`html_fixture_v1`)

```html
<div class="conversation" data-team="Team Alpha" data-channel="General" data-chat-type="channel">
  <div class="message" data-id="msg-1" data-from="alice@example.com"
       data-from-name="Alice" data-sent-at="2024-06-01T10:00:00Z">
    <div class="body">Hello <script>alert(1)</script> world</div>
    <div class="reactions">
      <span class="reaction" data-from="bob@example.com" data-emoji="👍"></span>
    </div>
    <div class="attachments">
      <a class="attachment" href="https://sharepoint.example/Contract_v2.docx"
         data-name="Contract_v2.docx">Contract_v2.docx</a>
    </div>
  </div>
</div>
```

Body HTML is sanitized with **ammonia** (no tags allowed) → plain text, then
reaction/attachment lines are appended.

## JSON field mapping (best-effort)

See `json_parse` module docs. Non-Teams JSON is **skipped** (no `item_error`).
Teams-shaped but unusable schema → parse **error** + `item_error`.

## PST enrich

Detect: `message_class` contains `SkypeTeams` / `IPM.SkypeTeams`, or path contains
`Team Chat` / `Conversation History`. Enriches existing items from metadata +
CAS text (HTML → plain if needed) + attachment child titles. Does not re-open
PST files.

## Job `teams_extract`

```json
{
  "source_id": null,
  "formats": ["pst", "html", "json"],
  "max_html_bytes": 20000000,
  "max_messages_per_file": 50000,
  "reset": false,
  "batch_size": 50,
  "force": false
}
```

- `reset`/`force` false: skip leaves with `teams_extract_status=ok|skipped`
- Cancel + checkpoint cursor
- Audit: `teams_extract.start` / `.complete` / `.fail`
- Complete audit includes format counts: `html_count`, `json_count`, `pst_count`
- Per-file errors continue

## Fixtures

| File | Purpose |
|---|---|
| `fixtures/teams/multi_day_channel.html` | two UTC days → two conversation_ids |
| `fixtures/teams/xss_script.html` | script + onclick → no script in plain text |
| `fixtures/teams/reactions_attachments.html` | reaction + attachment lines |
| `fixtures/teams/corrupt.html` | non-Teams HTML → **skipped** (no item_error) |
| `fixtures/teams/messages.json` | minimal valid JSON array |

## Blocking worker

Call `run_teams_extract` only from the process-runner matter worker thread.
