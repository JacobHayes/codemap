//! What `codemap serve` tells open pages.

use serde::Serialize;
use ts_rs::TS;

/// Sent on `GET /api/events` once per completed re-index, and once on connecting with the current
/// revision. The revision counts completed indexes since the server started; a page that sees a
/// new one asks its queries again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "Events.ts")]
pub struct Refresh {
    pub revision: u32,
}
