//! `silo-ctr tap --state DIR --sock PATH`: opens the running container's network device again and
//! hands it to the engine over a socket.
//!
//! The device (`tap0`) is kept by the namespace (`net::keep_tap`), so an engine that started later
//! than the container can take the container back: it runs this, receives the descriptor as the
//! holder did at start and starts a new gateway on it. The device's address and route are
//! unchanged.

/// The `--state` and `--sock` options.
pub fn parse_args(args: &[String]) -> Result<(String, String), String> {
    let usage = "usage: silo-ctr tap --state DIR --sock PATH";
    match args {
        [a, state, b, sock] if a == "--state" && b == "--sock" => Ok((state.clone(), sock.clone())),
        [a, sock, b, state] if a == "--sock" && b == "--state" => Ok((state.clone(), sock.clone())),
        _ => Err(usage.into()),
    }
}

#[cfg(target_os = "linux")]
pub fn command(args: &[String]) -> Result<i32, String> {
    use std::os::fd::AsRawFd;
    let (state, sock) = parse_args(args)?;
    crate::enter::join_holder(&state)?;
    let tap = crate::net::create_tap()?;
    let stream = crate::sock::connect(&sock)?;
    crate::sock::send_tap(&stream, tap.as_raw_fd())?;
    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn command(_: &[String]) -> Result<i32, String> {
    Err("Linux only".into())
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn both_options_are_required() {
        assert_eq!(
            parse_args(&args(&["--state", "/s", "--sock", "/k"])).unwrap(),
            ("/s".to_owned(), "/k".to_owned())
        );
        assert_eq!(
            parse_args(&args(&["--sock", "/k", "--state", "/s"])).unwrap(),
            ("/s".to_owned(), "/k".to_owned())
        );
        assert!(parse_args(&args(&["--state", "/s"])).is_err());
        assert!(parse_args(&args(&[])).is_err());
    }
}
