use super::Frontend;
use crate::session::Session;

/// Placeholder: implement with the windows crate on a Windows host.
pub struct WindowsFrontend;
impl Frontend for WindowsFrontend {
    fn run(self, _session: Session) -> Result<(), String> {
        Err("The Windows native frontend is planned. Use --cli for now.".into())
    }
}
