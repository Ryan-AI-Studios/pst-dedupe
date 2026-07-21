# stt-plugin

Opt-in **offline speech-to-text** for Dedupe Desk (track **0053**).

## What it does

- Job kind **`transcribe`**: resumable, checkpointed; cancel between items **and** mid ffmpeg/whisper (Job Object terminate)
- Candidates: audio/video natives (wav/mp3/m4a/flac/ogg + mp4/mov/mkv/webm)
- Success: transcript plain text → CAS via `text_sha256` (with concat policy)
- Bookkeeping: `transcript_*` columns (schema **v32**)
- Default engine: **whisper.cpp CLI** sidecar
- Common audio (wav/flac/ogg/mp3, including non-canonical WAV): **Symphonia** pure-Rust decode + linear resample → 16 kHz mono s16le (no ffmpeg)
- Video / complex containers: optional **ffmpeg** sidecar
- CI: **`MockSttEngine`** — no Whisper weights or ffmpeg required
- After STT, run **`fts_index`** so keyword search finds multimedia via transcript

## Body / metadata policy

| Case | Behavior |
|---|---|
| No existing text | Write transcript only |
| Existing metadata/body | **Concatenate**: `{existing}\n\n--- TRANSCRIPT ---\n\n{stt}` |
| Re-transcribe (`reset`) | Strip after first `--- TRANSCRIPT ---`, re-append new STT |
| Parent email + attach | Transcribe **attachment child** only — never parent body |

**Forbidden:** blind overwrite of non-empty `text_sha256` with transcript-only content.

## Operator install

STT is **off by default**. Core Desk builds and runs without Whisper or ffmpeg.

1. Build / install [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (`whisper-cli` / `main`).
2. Download a model **yourself** (e.g. `ggml-base.bin`) — Desk **never** downloads weights.
3. For video / complex containers: install **ffmpeg** (common audio works without it via Symphonia).
4. In Desk **Settings**:
   - Check **Enable local STT**
   - Set model path; optionally whisper-cli and ffmpeg paths
5. Workspace → **Run transcription**
6. Optionally run **FTS index** so transcripts become searchable

## ffmpeg conversion (LOCKED)

whisper.cpp is brittle on format. Conversion **must** force:

```text
ffmpeg -y -i <in> -ar 16000 -ac 1 -c:a pcm_s16le <temp.wav>
```

Never demux original audio “as-is” without resample/channel/codec coerce.

## Child process lifetime (Job Object)

On Windows, whisper/ffmpeg children run inside a **Job Object** with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so OS kills them if Desk crashes or is
force-quit. Cooperative cancel polls during the sidecar wait and calls
`TerminateJobObject` on the active child (not only between items).

## Honesty / limits (P0)

- **Un-diarized** — speakers are **not** identified. A keyword hit does **not**
  attribute speech to a person; **human must listen** to original media before
  treating the transcript as attributed evidence.
- Whisper-class models can **hallucinate** on silence/noise.
- Accuracy varies by language/accent/noise; English default.
- Not a substitute for certified court reporting.
- Large models need operator download; **not in git**.

| Limit | Default |
|---|---|
| Max duration | 3600 s (native WAV header; post Symphonia/ffmpeg converted WAV re-checked) |
| Max native | 500 MiB |
| Max transcript text | 10 MiB |

Temps live under `<matter>/workspace/temp/stt/` with **Drop guards** and a
**startup purge**. Source media is never mutated.

## Params (job JSON)

```json
{
  "enabled": false,
  "engine": "auto",
  "model_path": null,
  "whisper_cli_path": null,
  "ffmpeg_path": null,
  "language": "en",
  "max_duration_secs": 3600,
  "max_native_bytes": 500000000,
  "reset": false,
  "batch_size": 5,
  "scope": "all"
}
```

- `enabled: false` → job fails immediately; no item mutation
- `engine: "mock"` → tests only (rejected on production path)
- `reset: true` (alias `force`) → re-transcribe prior successes
- Digest skip: same `transcript_native_sha256` + status done → skip

## Tests

```powershell
cargo test -p stt-plugin
# Optional live smoke (requires local whisper.cpp + model):
# cargo test -p stt-plugin -- --ignored
```
