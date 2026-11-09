//! silo-ctr: the host side of a container computer, run as the ordinary user.
//!
//!   silo-ctr userns [--sub-uid N] [--sub-gid N] [--net] [--keep-id N] -- CMD ARGS...
//!   silo-ctr run --state DIR --lower L1:L2:... --upper U --work W --merged M --bundle B --id ID
//!                [--sock PATH] [--crun PATH] [--sub-uid N --sub-gid N]
//!   silo-ctr enter --state DIR -- CMD ARGS...
//!   silo-ctr connect --state DIR --host HOST --port N
//!   silo-ctr tap --state DIR --sock PATH   (hands the running container's network device to the engine)
//!   silo-ctr merge --out DIR LAYER...   (inside `userns`: folds overlay layers into one)
//!   silo-ctr pack LAYER                 (inside `userns`: the layer's files onto standard output)
//!   silo-ctr unpack [--max-bytes N] [--max-entries N] LAYER   (inside `userns`: standard input into an empty layer)
//!   silo-ctr version

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod common;
mod connect;
mod enter;
mod merge;
mod net;
mod pack;
mod run;
mod sock;
mod subid;
mod tap;
mod userns;

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The engine runs with a private umask; what runs in the container (crun's mount points,
    // the user's files) must get the ordinary one.
    // SAFETY: umask has no preconditions.
    unsafe { libc::umask(0o022) };
    let outcome = match args.first().map(String::as_str) {
        Some("userns") => userns::command(&args[1..]),
        Some("connect") => connect::command(&args[1..]),
        Some("enter") => enter::command(&args[1..]),
        Some("run") => run::command(&args[1..]),
        Some("merge") => merge::command(&args[1..]),
        Some("pack") => pack::pack_command(&args[1..]),
        Some("unpack") => pack::unpack_command(&args[1..]),
        Some("tap") => tap::command(&args[1..]),
        Some("version") => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        _ => Err(
            "usage: silo-ctr userns|run|enter|connect|merge|pack|unpack|tap|version (see the module documentation)".to_string(),
        ),
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
