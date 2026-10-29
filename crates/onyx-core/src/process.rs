//! Starting another program. A release build has no console on Windows
//! (`windows_subsystem` in the app's `main.rs`), and a console program
//! started from it opens a console window of its own unless it is told
//! not to. Every child goes through `command`, never `Command::new`.

use std::ffi::OsStr;
use std::process::Command;

/// `Command::new`, with no console window for the child on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}
