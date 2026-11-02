// Hides the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // ssh starts this executable to carry a connection through Tor. First:
    // askpass's variables reach it too.
    if let Some(code) = sshee_core::tor::proxy_main() {
        std::process::exit(code);
    }
    // ssh starts this executable to ask for a saved password: answer and exit.
    if let Some(code) = sshee_core::secrets::askpass_main() {
        std::process::exit(code);
    }
    ssheesh_lib::run()
}
