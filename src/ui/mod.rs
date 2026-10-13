//! Native frontends depend on Session/View/Action, never on hidden game state.
use crate::session::Session;

pub mod cli;
#[cfg(all(target_os = "linux", feature = "gui"))]
pub mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub trait Frontend {
    fn run(self, session: Session) -> Result<(), String>;
}

pub fn native(session: Session) -> Result<(), String> {
    #[cfg(all(target_os = "linux", feature = "gui"))]
    {
        linux::GtkFrontend.run(session)
    }
    #[cfg(target_os = "macos")]
    {
        macos::MacFrontend.run(session)
    }
    #[cfg(target_os = "windows")]
    {
        windows::WindowsFrontend.run(session)
    }
    #[cfg(not(any(
        all(target_os = "linux", feature = "gui"),
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        let _ = session;
        Err("Native UI unavailable. Use --cli, or build on Linux with the gui feature.".into())
    }
}
