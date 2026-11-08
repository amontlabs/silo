//! silo-ctr: the host side of a container computer, run as the ordinary user.
//!
//!   silo-ctr userns [--sub-uid N] [--sub-gid N] [--net] -- CMD ARGS...
//!   silo-ctr run --state DIR --lower L1:L2:... --upper U --work W --merged M --bundle B --id ID
//!                [--sock PATH] [--crun PATH] [--sub-uid N --sub-gid N]
//!   silo-ctr version

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod common;
mod net;
mod run;
mod sock;
mod subid;
mod userns;

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("userns") => userns::command(&args[1..]),
        Some("run") => run::command(&args[1..]),
        Some("version") => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        _ => Err("usage: silo-ctr userns|run|version (see the module documentation)".to_string()),
    };
    match outcome {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("silo-ctr: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux only");
    std::process::exit(2);
}
