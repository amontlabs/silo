//! The conversation with the engine over a Unix stream socket:
//!
//!   holder -> engine   `tap\n` carrying the TAP descriptor as SCM_RIGHTS
//!   engine -> holder   `key=value` lines and an empty line (see `net::parse_config`), or `error=<text>`
//!   holder -> engine   `up\n` once the interface is configured

use crate::common::{io_err, os_err};
use crate::net::{parse_config, NetConfig};
use std::io::{BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

pub fn connect(path: &str) -> Result<UnixStream, String> {
    UnixStream::connect(path).map_err(|error| io_err(&format!("connect {path}"), &error))
}

/// Sends `tap\n` with `fd` attached.
pub fn send_tap(stream: &UnixStream, fd: RawFd) -> Result<(), String> {
    let data = b"tap\n";
    let mut iov = libc::iovec {
        iov_base: data.as_ptr() as *mut libc::c_void,
        iov_len: data.len(),
    };
    // SAFETY: CMSG_SPACE is a pure size computation.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    // 8-byte aligned storage for the control message.
    let mut control = vec![0u64; space.div_ceil(8)];
    // SAFETY: a zeroed msghdr is valid; the pointers set below outlive the sendmsg call, `control` is
    // large and aligned enough for one SCM_RIGHTS message holding one descriptor, and CMSG_FIRSTHDR points
    // inside it.
    let sent = unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr() as *mut libc::c_void;
        message.msg_controllen = space as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
        std::ptr::copy_nonoverlapping(
            &fd as *const RawFd as *const u8,
            libc::CMSG_DATA(header),
            std::mem::size_of::<RawFd>(),
        );
        libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL)
    };
    if sent != data.len() as isize {
        return Err(os_err("send the TAP descriptor"));
    }
    Ok(())
}

/// Reads the engine's configuration lines.
pub fn read_config(stream: &UnixStream) -> Result<NetConfig, String> {
    parse_config(&mut BufReader::new(stream))
}

/// Tells the engine the interface is configured.
pub fn send_up(mut stream: &UnixStream) -> Result<(), String> {
    stream
        .write_all(b"up\n")
        .map_err(|error| io_err("write to the engine", &error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::{FromRawFd, OwnedFd};

    /// Receives one message and the descriptor attached to it.
    fn receive(stream: &UnixStream) -> (Vec<u8>, Option<OwnedFd>) {
        let mut data = [0u8; 16];
        let mut iov = libc::iovec {
            iov_base: data.as_mut_ptr() as *mut libc::c_void,
            iov_len: data.len(),
        };
        let mut control = [0u64; 8];
        // SAFETY: the buffers outlive the call; the descriptor in an SCM_RIGHTS message is a fresh
        // descriptor of this process, so owning it is sound.
        unsafe {
            let mut message: libc::msghdr = std::mem::zeroed();
            message.msg_iov = &mut iov;
            message.msg_iovlen = 1;
            message.msg_control = control.as_mut_ptr() as *mut libc::c_void;
            message.msg_controllen = std::mem::size_of_val(&control) as _;
            let got = libc::recvmsg(stream.as_raw_fd(), &mut message, 0);
            assert!(got > 0);
            let header = libc::CMSG_FIRSTHDR(&message);
            let fd = if header.is_null() {
                None
            } else {
                let mut fd: RawFd = -1;
                std::ptr::copy_nonoverlapping(
                    libc::CMSG_DATA(header),
                    &mut fd as *mut RawFd as *mut u8,
                    std::mem::size_of::<RawFd>(),
                );
                Some(OwnedFd::from_raw_fd(fd))
            };
            (data[..got as usize].to_vec(), fd)
        }
    }

    #[test]
    fn the_descriptor_travels_with_the_tap_line() {
        let (holder, engine) = UnixStream::pair().unwrap();
        let (mut pipe_read, pipe_write) = {
            let mut fds = [0; 2];
            // SAFETY: fds has room for two descriptors.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            // SAFETY: pipe returned two fresh descriptors.
            unsafe {
                (
                    std::fs::File::from_raw_fd(fds[0]),
                    std::fs::File::from_raw_fd(fds[1]),
                )
            }
        };
        send_tap(&holder, pipe_write.as_raw_fd()).unwrap();
        let (data, fd) = receive(&engine);
        assert_eq!(data, b"tap\n");
        let mut received = std::fs::File::from(fd.expect("a descriptor"));
        received.write_all(b"x").unwrap();
        drop((received, pipe_write));
        let mut text = String::new();
        pipe_read.read_to_string(&mut text).unwrap();
        assert_eq!(text, "x");
    }

    #[test]
    fn the_config_and_up_lines_round_trip() {
        let (holder, mut engine) = UnixStream::pair().unwrap();
        engine
            .write_all(b"mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\n\n")
            .unwrap();
        assert_eq!(read_config(&holder).unwrap().prefix_len, 24);
        send_up(&holder).unwrap();
        let mut line = [0u8; 3];
        engine.read_exact(&mut line).unwrap();
        assert_eq!(&line, b"up\n");
    }

    #[test]
    fn connecting_to_a_missing_socket_names_the_path() {
        let error = connect("/nonexistent/silo.sock").unwrap_err();
        assert_eq!(
            error,
            "connect /nonexistent/silo.sock: No such file or directory"
        );
    }
}
