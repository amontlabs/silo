//! `macos.display.stream`: the owner's splice between a controller's bridge stream and the
//! guest's Screen Sharing connection.
use super::*;
use std::net::{TcpListener, TcpStream};

/// A TCP server standing in for the guest's Screen Sharing: it sends a banner, then echoes.
fn guest(banner: &'static [u8]) -> (std::net::SocketAddr, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.write_all(banner).unwrap();
        let mut received = Vec::new();
        let mut buffer = [0u8; 256];
        while let Ok(count) = socket.read(&mut buffer) {
            if count == 0 {
                break;
            }
            received.extend_from_slice(&buffer[..count]);
            if socket.write_all(&buffer[..count]).is_err() {
                break;
            }
        }
        received
    });
    (address, worker)
}

static BUDGET: Budget = Budget::new(2);

fn serve(
    mut server: UnixStream,
    params: Value,
    open: impl FnOnce(&Value) -> Result<TcpStream, String> + Send + 'static,
    allowed: impl Fn() -> bool + Send + 'static,
) -> thread::JoinHandle<Result<(), BridgeError>> {
    thread::spawn(move || {
        let mut permit = None;
        serve_display_stream(&mut server, &mut permit, &BUDGET, &params, open, allowed)
    })
}

fn client_of(pair: (UnixStream, UnixStream)) -> (UnixStream, UnixStream) {
    pair.0
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    pair
}

#[test]
fn bytes_are_spliced_both_ways_after_the_reply_and_closing_ends_the_guest_connection() {
    let _test_state = crate::test_support::global_state();
    let (address, guest) = guest(b"RFB 003.889\n");
    let (mut client, server) = client_of(UnixStream::pair().unwrap());
    let worker = serve(
        server,
        json!({"computerId": "c"}),
        move |_| TcpStream::connect(address).map_err(|e| e.to_string()),
        || true,
    );
    assert_eq!(read_frame(&mut client).unwrap()["result"], json!({}));
    let mut banner = [0u8; 12];
    client.read_exact(&mut banner).unwrap();
    assert_eq!(&banner, b"RFB 003.889\n");
    client.write_all(b"\x00\x01\x02binary").unwrap();
    let mut echoed = [0u8; 9];
    client.read_exact(&mut echoed).unwrap();
    assert_eq!(&echoed, b"\x00\x01\x02binary");
    drop(client);
    worker.join().unwrap().unwrap();
    assert_eq!(guest.join().unwrap(), b"\x00\x01\x02binary");
}

#[test]
fn a_guest_that_ends_the_connection_ends_the_stream() {
    let _test_state = crate::test_support::global_state();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let (mut client, server) = client_of(UnixStream::pair().unwrap());
    let worker = serve(
        server,
        json!({}),
        move |_| TcpStream::connect(address).map_err(|e| e.to_string()),
        || true,
    );
    let (accepted, _) = listener.accept().unwrap();
    assert_eq!(read_frame(&mut client).unwrap()["result"], json!({}));
    drop(accepted);
    let mut byte = [0u8];
    assert!(matches!(client.read(&mut byte), Ok(0) | Err(_)));
    worker.join().unwrap().unwrap();
}

#[test]
fn revoked_access_ends_the_stream() {
    let _test_state = crate::test_support::global_state();
    let (address, _guest) = guest(b"x");
    let (mut client, server) = client_of(UnixStream::pair().unwrap());
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = allowed.clone();
    let worker = serve(
        server,
        json!({}),
        move |_| TcpStream::connect(address).map_err(|e| e.to_string()),
        move || flag.load(std::sync::atomic::Ordering::Acquire),
    );
    assert_eq!(read_frame(&mut client).unwrap()["result"], json!({}));
    allowed.store(false, std::sync::atomic::Ordering::Release);
    worker.join().unwrap().unwrap();
}

#[test]
fn a_refusal_is_returned_before_any_stream_starts_and_frees_its_permit() {
    let _test_state = crate::test_support::global_state();
    let (client, server) = UnixStream::pair().unwrap();
    let worker = serve(
        server,
        json!({}),
        |_| Err("The computer is not running. Start it to show its screen.".into()),
        || true,
    );
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.message.contains("not running"));
    drop(client);
    assert!(BUDGET.acquire().is_some());
}

#[test]
fn the_stream_budget_is_enforced() {
    let _test_state = crate::test_support::global_state();
    static FULL: Budget = Budget::new(1);
    let _held = FULL.acquire().unwrap();
    let (_client, mut server) = UnixStream::pair().unwrap();
    let mut permit = None;
    let result = serve_display_stream(
        &mut server,
        &mut permit,
        &FULL,
        &json!({}),
        |_| panic!("no stream is opened without a permit"),
        || true,
    );
    assert!(result.unwrap_err().message.contains("too many active"));
}

#[test]
fn the_display_methods_are_classified_for_replay_and_streaming() {
    assert_eq!(access("macos.display.stream"), Some(Access::Stream));
    assert!(is_stream_method(Some("macos.display.stream")));
    assert!(is_stream_method(Some("guest.ssh")));
    assert!(!is_stream_method(Some("macos.display.resize")));
    assert!(!is_stream_method(None));
    assert_eq!(access("macos.display.resize"), Some(Access::Read));
    assert_eq!(access("macos.create"), Some(Access::Change));
    assert_eq!(access("macos.action"), Some(Access::Change));
}

mod opening {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn reply_bytes(value: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_reply(&mut bytes, value).unwrap();
        bytes
    }

    #[test]
    fn a_reply_that_arrives_is_returned_with_the_reader_after_it() {
        let (mut peer, ours) = UnixStream::pair().unwrap();
        peer.write_all(&reply_bytes(&json!({"result":{}}))).unwrap();
        peer.write_all(b"RFB").unwrap();
        let (reply, mut rest) = read_reply_cancellable(
            std::io::BufReader::new(ours),
            &|| false,
            Duration::from_secs(5),
            || panic!("nothing to abort"),
        )
        .unwrap();
        assert_eq!(reply["result"], json!({}));
        let mut raw = [0u8; 3];
        rest.read_exact(&mut raw).unwrap();
        assert_eq!(&raw, b"RFB");
    }

    #[test]
    fn cancelling_stops_a_stalled_open_and_ends_what_it_reads_from() {
        let (_peer, ours) = UnixStream::pair().unwrap();
        let ender = ours.try_clone().unwrap();
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(150));
                cancelled.store(true, Ordering::Release);
            });
            read_reply_cancellable(
                std::io::BufReader::new(ours),
                &|| cancelled.load(Ordering::Acquire),
                Duration::from_secs(30),
                || {
                    let _ = ender.shutdown(std::net::Shutdown::Both);
                },
            )
        });
        assert_eq!(result.err().as_deref(), Some(OPEN_CANCELLED));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_device_that_never_answers_times_out_and_is_aborted() {
        let (_peer, ours) = UnixStream::pair().unwrap();
        let aborted = AtomicBool::new(false);
        let result = read_reply_cancellable(
            std::io::BufReader::new(ours),
            &|| false,
            Duration::from_millis(200),
            || aborted.store(true, Ordering::Release),
        );
        assert_eq!(result.err().as_deref(), Some(OPEN_TIMED_OUT));
        assert!(aborted.load(Ordering::Acquire));
    }
}
