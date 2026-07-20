# People–comms graph fixtures (track 0047)

Synthetic only — **example.com** / RFC reserved addresses. Never commit real custodian mailboxes or address books.

## Scenarios to exercise

| Case | Notes |
|---|---|
| Multi-party To | A → B,C yields two directed edges |
| BCC | Stored with `role=bcc`; not in `visible_count` |
| Self-mail | A → A: no edge; `self_mail_count` |
| Display name | `"John Doe"` becomes `identity_kind=display` |
| X.500 | `/o=…/cn=…` becomes `identity_kind=x500` |
| SMTP punctuation | `bob@example.com,` same key as without comma |

Use `matter-people` integration tests for automated coverage; this folder holds human-readable scenario notes.
