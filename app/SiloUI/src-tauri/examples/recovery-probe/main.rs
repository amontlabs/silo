//! Boots a copy of a macOS computer into Recovery and logs what Silo's screen
//! reader sees. macOS only; see `macos.rs`.
#[cfg(target_os = "macos")]
mod macos;

fn main() {
    #[cfg(target_os = "macos")]
    macos::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("recovery-probe runs on macOS only.");
}
