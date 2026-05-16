//! Message Store — MS-PST §2.4.3
//!
//! The message store is the root object of the PST, providing the store's
//! display name and entry to the root folder hierarchy.

use crate::error::Result;
use crate::ltp::pc;
use crate::ndb::nid;
use crate::PstFile;

impl PstFile {
    /// Get the display name of the PST message store.
    pub fn display_name(&mut self) -> Result<String> {
        let crypt = self.header.crypt_method;
        let prop_ctx = pc::load_pc(
            &mut self.reader,
            &self.nbt,
            &self.bbt,
            nid::NID_MESSAGE_STORE,
            crypt,
        )?;

        prop_ctx.get_string(nid::PID_TAG_DISPLAY_NAME)?.ok_or(
            crate::error::PstError::PropertyNotFound(nid::PID_TAG_DISPLAY_NAME),
        )
    }
}
