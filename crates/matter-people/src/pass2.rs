//! Pass 2: atomic SQL aggregates from `item_participants`.

use matter_core::{people_graph_pass, Matter};

use crate::error::Result;
use crate::params::PeopleGraphParams;

/// Rebuild people / edges / timeline aggregates (single transaction inside Matter).
///
/// Does **not** set `built_at` / fingerprint — caller does after success.
pub fn run_pass2(matter: &Matter, params: &PeopleGraphParams) -> Result<()> {
    matter.set_people_graph_pass(Some(people_graph_pass::PASS2), None)?;
    matter.rebuild_people_graph_aggregates(&params.grain)?;
    Ok(())
}
