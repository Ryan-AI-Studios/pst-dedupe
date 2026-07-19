//! Review text synthesis for calendar items (spec §3.6).

use crate::extract::CalendarEventFields;
use crate::limits::{MAX_EXTRACTED_TEXT_BYTES, TRUNCATION_MARKER};

/// Build UTF-8 display text for Review (Subject/When/Where/…).
pub fn synthesize_calendar_review_text(fields: &CalendarEventFields) -> (String, bool) {
    let subject = fields.subject.as_deref().unwrap_or("");
    let when = match (
        fields.cal_start_at.as_deref(),
        fields.cal_end_at.as_deref(),
        fields.cal_all_day,
    ) {
        (Some(s), Some(e), Some(1)) => format!("{s} – {e} [ALL-DAY]"),
        (Some(s), Some(e), _) => format!("{s} – {e}"),
        (Some(s), None, Some(1)) => format!("{s} [ALL-DAY]"),
        (Some(s), None, _) => s.to_string(),
        (None, Some(e), _) => format!("– {e}"),
        _ => String::new(),
    };
    let where_ = fields.cal_location.as_deref().unwrap_or("");
    let organizer = fields.cal_organizer.as_deref().unwrap_or("");
    let attendees = fields.attendee_addrs.join("; ");
    let busy = fields.cal_busy_status.as_deref().unwrap_or("");
    let class = fields.message_class.as_deref().unwrap_or("VEVENT");
    let mut description = fields.description.clone().unwrap_or_default();
    let mut rrule_note = String::new();
    if let Some(ref r) = fields.rrule_text {
        rrule_note = format!("\nRRULE: {r}");
    }
    description.push_str(&rrule_note);

    let header = format!(
        "Subject: {subject}\n\
         When: {when}\n\
         Where: {where_}\n\
         Organizer: {organizer}\n\
         Attendees: {attendees}\n\
         Busy: {busy}\n\
         Class: {class}\n\
         ---\n"
    );
    let mut full = header;
    full.push_str(&description);

    let mut partial = false;
    if full.len() > MAX_EXTRACTED_TEXT_BYTES {
        partial = true;
        let keep = MAX_EXTRACTED_TEXT_BYTES.saturating_sub(TRUNCATION_MARKER.len());
        // Truncate on char boundary.
        let mut end = keep.min(full.len());
        while end > 0 && !full.is_char_boundary(end) {
            end -= 1;
        }
        full.truncate(end);
        full.push_str(TRUNCATION_MARKER);
    }
    (full, partial)
}
