# Track 004 Spec: GUI Errors And Partial Results

## Expected Behavior

- Users can see which PST files completed and which failed.
- Error messages are specific enough to distinguish unsupported format, parse failure, permission problems, and cancellation.
- Partial scan results remain available when safe.

## Edge Cases

- User cancels while a PST is being parsed.
- One PST succeeds and another fails.
- User removes or moves a selected file before scan starts.
- Dialog returns no path or an inaccessible path.
- GUI dependency update changes `eframe` trait signatures, viewport setup, or `rfd` dialog behavior.

## Verification

- `cargo check -p pst-dedup-gui`
- Manual GUI scan with one valid and one invalid path.
