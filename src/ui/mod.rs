//! Native frontends depend on Session/View/Action, never on hidden game state.
use crate::session::Session;

pub mod cli;
#[cfg(all(target_os = "linux", feature = "gui"))]
pub mod linux;
#[cfg(all(target_os = "macos", feature = "gui"))]
mod macos;
#[cfg(feature = "gui")]
mod presentation;
#[cfg(all(target_os = "windows", feature = "gui"))]
mod windows;

pub trait Frontend {
    fn run(self, session: Session) -> Result<(), String>;
}

pub fn native(session: Session) -> Result<(), String> {
    #[cfg(all(target_os = "linux", feature = "gui"))]
    {
        linux::GtkFrontend.run(session)
    }
    #[cfg(all(target_os = "macos", feature = "gui"))]
    {
        macos::MacFrontend.run(session)
    }
    #[cfg(all(target_os = "windows", feature = "gui"))]
    {
        windows::WindowsFrontend.run(session)
    }
    #[cfg(not(any(
        all(target_os = "linux", feature = "gui"),
        all(target_os = "macos", feature = "gui"),
        all(target_os = "windows", feature = "gui")
    )))]
    {
        let _ = session;
        Err(
            "Native UI unavailable. Use --cli, or build on Linux, macOS, or Windows with the gui feature."
                .into(),
        )
    }
}
