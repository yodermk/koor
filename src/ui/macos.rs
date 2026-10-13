use super::Frontend;
use crate::session::Session;

/// Placeholder: implement with objc2-app-kit on a macOS host.
pub struct MacFrontend;
impl Frontend for MacFrontend {
    fn run(self, _session: Session) -> Result<(), String> {
        Err("The macOS AppKit frontend is planned. Use --cli for now.".into())
    }
}
