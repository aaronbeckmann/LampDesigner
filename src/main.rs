// Release builds are a windowed app (no console window behind the GUI).
// Command-line use: `lampdesigner-cli`, or `start.ps1 <command>`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        // CLI arguments given to the windowed exe: print into the calling console if there is one
        attach_parent_console();
        if let Some(code) = lampdesigner::cli::run(&args) {
            std::process::exit(code);
        }
    }
    lampdesigner::run_gui();
}

#[cfg(windows)]
fn attach_parent_console() {
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 call without pointers; failure (no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
