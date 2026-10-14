use std::collections::VecDeque;
use std::io;
use std::net::Ipv4Addr;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicU64, Ordering};

use msb_krun::backends::net::{NetBackend, ReadError};
use serde::Serialize;

const VIRTIO_NET_HEADER: usize = 12;
const ETH_HEADER_LEN: usize = 14;
const MAX_BATCH: usize = 32;
const MAX_PENDING_FRAMES: usize = 128;
const FRAME_BUFFER_CAP: usize = 1514;

#[derive(Default)]
pub struct FrameStats {
    guest_to_engine_frames: AtomicU64,
    guest_to_engine_bytes: AtomicU64,
    engine_to_guest_frames: AtomicU64,
    engine_to_guest_bytes: AtomicU64,
    malformed_frames: AtomicU64,
    guest_frames_truncated: AtomicU64,
    guest_mac_spoof: AtomicU64,
    guest_ip_spoof: AtomicU64,
    arp_about_guest_dropped: AtomicU64,
    ipv6_rejected: AtomicU64,
    unsupported_ethertype: AtomicU64,
    engine_frames_queued: AtomicU64,
    engine_send_eagain: AtomicU64,
    pending_queue_full: AtomicU64,
    backend_read_errors: AtomicU64,
    backend_write_errors: AtomicU64,
    frame_send_errors: AtomicU64,
    interrupted_polls: AtomicU64,
    interrupted_reads: AtomicU64,
    interrupted_writes: AtomicU64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FrameCounters {
    pub guest_to_engine_frames: u64,
    pub guest_to_engine_bytes: u64,
    pub engine_to_guest_frames: u64,
    pub engine_to_guest_bytes: u64,
    pub malformed_frames: u64,
    pub guest_frames_truncated: u64,
    pub guest_mac_spoof: u64,
    pub guest_ip_spoof: u64,
    pub arp_about_guest_dropped: u64,
    pub ipv6_rejected: u64,
    pub unsupported_ethertype: u64,
    pub engine_frames_queued: u64,
    pub engine_send_eagain: u64,
    pub pending_queue_full: u64,
    pub backend_read_errors: u64,
    pub backend_write_errors: u64,
    pub backend_tx_ring_drops: u64,
    pub frame_send_errors: u64,
    pub interrupted_polls: u64,
    pub interrupted_reads: u64,
    pub interrupted_writes: u64,
}

impl FrameStats {
    pub fn snapshot_with_backend_tx_ring_drops(&self, backend_tx_ring_drops: u64) -> FrameCounters {
        FrameCounters {
            guest_to_engine_frames: self.guest_to_engine_frames.load(Ordering::Relaxed),
            guest_to_engine_bytes: self.guest_to_engine_bytes.load(Ordering::Relaxed),
            engine_to_guest_frames: self.engine_to_guest_frames.load(Ordering::Relaxed),
            engine_to_guest_bytes: self.engine_to_guest_bytes.load(Ordering::Relaxed),
            malformed_frames: self.malformed_frames.load(Ordering::Relaxed),
            guest_frames_truncated: self.guest_frames_truncated.load(Ordering::Relaxed),
            guest_mac_spoof: self.guest_mac_spoof.load(Ordering::Relaxed),
            guest_ip_spoof: self.guest_ip_spoof.load(Ordering::Relaxed),
            arp_about_guest_dropped: self.arp_about_guest_dropped.load(Ordering::Relaxed),
            ipv6_rejected: self.ipv6_rejected.load(Ordering::Relaxed),
            unsupported_ethertype: self.unsupported_ethertype.load(Ordering::Relaxed),
            engine_frames_queued: self.engine_frames_queued.load(Ordering::Relaxed),
            engine_send_eagain: self.engine_send_eagain.load(Ordering::Relaxed),
            pending_queue_full: self.pending_queue_full.load(Ordering::Relaxed),
            backend_read_errors: self.backend_read_errors.load(Ordering::Relaxed),
            backend_write_errors: self.backend_write_errors.load(Ordering::Relaxed),
            backend_tx_ring_drops,
            frame_send_errors: self.frame_send_errors.load(Ordering::Relaxed),
            interrupted_polls: self.interrupted_polls.load(Ordering::Relaxed),
            interrupted_reads: self.interrupted_reads.load(Ordering::Relaxed),
            interrupted_writes: self.interrupted_writes.load(Ordering::Relaxed),
        }
    }

    pub fn interrupted_polls(&self) {
        self.interrupted_polls.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct PollEvents {
    pub frame: libc::pollfd,
    pub backend: libc::pollfd,
}

pub struct FramePump<'a> {
    frame_fd: RawFd,
    guest_ipv4: Ipv4Addr,
    guest_mac: [u8; 6],
    mtu: u16,
    stats: &'a FrameStats,
    backend: Box<dyn NetBackend + Send>,
    pending: PendingFrames,
    engine_first: bool,
}

#[derive(Debug)]
pub enum FramePumpError {
    BackendWrite,
    BackendRead,
    SocketReceive,
    SocketSend,
    PartialDatagramSend,
}

impl<'a> FramePump<'a> {
    pub fn new(
        frame_fd: RawFd,
        guest_ipv4: Ipv4Addr,
        guest_mac: [u8; 6],
        mtu: u16,
        stats: &'a FrameStats,
        backend: Box<dyn NetBackend + Send>,
    ) -> Self {
        Self {
            frame_fd,
            guest_ipv4,
            guest_mac,
            mtu,
            stats,
            backend,
            pending: PendingFrames::new(MAX_PENDING_FRAMES),
            engine_first: false,
        }
    }

    pub fn poll_events(&self) -> PollEvents {
        let mut frame_events = libc::POLLIN;
        if !self.pending.is_empty() {
            frame_events |= libc::POLLOUT;
        }
        PollEvents {
            frame: libc::pollfd {
                fd: self.frame_fd,
                events: frame_events,
                revents: 0,
            },
            backend: libc::pollfd {
                fd: self.backend.raw_socket_fd(),
                events: if self.pending.is_full() {
                    0
                } else {
                    libc::POLLIN
                },
                revents: 0,
            },
        }
    }

    pub fn service(
        &mut self,
        frame_revents: libc::c_short,
        backend_revents: libc::c_short,
    ) -> Result<(), FramePumpError> {
        if backend_revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(FramePumpError::BackendRead);
        }
        if frame_revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(FramePumpError::SocketReceive);
        }
        let guest_ready = frame_revents & libc::POLLIN != 0;
        let engine_ready = backend_revents & libc::POLLIN != 0;
        let mut remaining = MAX_BATCH;

        if self.engine_first {
            remaining = self.service_engine(engine_ready, remaining)?;
            let _ = self.service_guest(guest_ready, remaining)?;
        } else {
            remaining = self.service_guest(guest_ready, remaining)?;
            let _ = self.service_engine(engine_ready, remaining)?;
        }
        self.engine_first = !self.engine_first;
        Ok(())
    }

    fn service_guest(&mut self, ready: bool, budget: usize) -> Result<usize, FramePumpError> {
        if !ready {
            return Ok(budget);
        }
        let mut frame = [0u8; FRAME_BUFFER_CAP];
        let mut handled = 0;
        while handled < budget {
            let (n, flags) = match recv_datagram(self.frame_fd, &mut frame) {
                Ok(result) => result,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    self.stats.interrupted_reads.fetch_add(1, Ordering::Relaxed);
                    handled += 1;
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(FramePumpError::SocketReceive),
            };
            handled += 1;
            if n > frame.len() || flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0 {
                self.stats
                    .guest_frames_truncated
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let frame = &mut frame[..n];
            match validate_guest_frame(frame, self.guest_mac, self.guest_ipv4, self.mtu) {
                Ok(_) => {}
                Err(FrameValidationError::GuestMacSpoof) => {
                    self.stats.guest_mac_spoof.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(FrameValidationError::GuestIpSpoof) => {
                    self.stats.guest_ip_spoof.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(FrameValidationError::Ipv6) => {
                    self.stats.ipv6_rejected.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(FrameValidationError::UnsupportedEtherType) => {
                    self.stats
                        .unsupported_ethertype
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(_) => {
                    self.stats.malformed_frames.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            }

            if arp_target(frame) == Some(self.guest_ipv4) {
                // The guest announces its address, or asks about it, to find out whether another
                // host holds it. The engine answers any ARP request, so its reply would say that
                // another host (the gateway) has the guest's own address, and macOS gives the
                // address up ("address in use").
                self.stats
                    .arp_about_guest_dropped
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.backend.write_frame(0, frame).is_err() {
                self.stats
                    .backend_write_errors
                    .fetch_add(1, Ordering::Relaxed);
                return Err(FramePumpError::BackendWrite);
            }
            self.stats
                .guest_to_engine_frames
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .guest_to_engine_bytes
                .fetch_add(n as u64, Ordering::Relaxed);
        }
        Ok(budget.saturating_sub(handled))
    }

    fn service_engine(&mut self, ready: bool, budget: usize) -> Result<usize, FramePumpError> {
        let mut remaining = budget;
        if !self.pending.is_empty() && remaining > 0 {
            let result = self.flush_pending(remaining)?;
            remaining = remaining.saturating_sub(result);
        }
        if !ready || remaining == 0 {
            return Ok(remaining);
        }

        let mut buffer = [0u8; VIRTIO_NET_HEADER + FRAME_BUFFER_CAP];
        let mut handled = 0;
        while handled < remaining && !self.pending.is_full() {
            handled += 1;
            match self.backend.read_frame(&mut buffer) {
                Ok(n) if n <= buffer.len() && n >= VIRTIO_NET_HEADER => {
                    let frame = &buffer[VIRTIO_NET_HEADER..n];
                    match validate_engine_frame(frame, self.mtu) {
                        Ok(_) => {}
                        Err(FrameValidationError::Ipv6) => {
                            self.stats.ipv6_rejected.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        Err(FrameValidationError::UnsupportedEtherType) => {
                            self.stats
                                .unsupported_ethertype
                                .fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        Err(_) => {
                            self.stats.malformed_frames.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    }
                    if arp_sender(frame) == Some(self.guest_ipv4) {
                        // Nothing but the guest may claim the guest's address.
                        self.stats
                            .arp_about_guest_dropped
                            .fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    if self.pending.push(frame.to_vec()).is_err() {
                        self.stats
                            .pending_queue_full
                            .fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    self.stats
                        .engine_frames_queued
                        .fetch_add(1, Ordering::Relaxed);
                }
                Ok(_) => {
                    self.stats.malformed_frames.fetch_add(1, Ordering::Relaxed);
                }
                Err(ReadError::NothingRead) => break,
                Err(_) => {
                    self.stats
                        .backend_read_errors
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(FramePumpError::BackendRead);
                }
            }
        }
        remaining = remaining.saturating_sub(handled);
        if !self.pending.is_empty() && remaining > 0 {
            self.flush_pending(remaining)?;
        }
        Ok(remaining)
    }

    fn flush_pending(&mut self, budget: usize) -> Result<usize, FramePumpError> {
        let result = self.pending.flush_with(budget, |frame| {
            let sent = unsafe {
                libc::send(
                    self.frame_fd,
                    frame.as_ptr().cast(),
                    frame.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if sent < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOBUFS) {
                    // A datagram socket whose receiver has not read for a while (the guest is
                    // still booting) answers ENOBUFS instead of EAGAIN. The link is lossy
                    // like any other: the frame is dropped and the senders retry.
                    return Ok(frame.len());
                }
                Err(error)
            } else {
                Ok(sent as usize)
            }
        });
        match result {
            Ok(result) => {
                if result.would_block {
                    self.stats
                        .engine_send_eagain
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.stats
                    .interrupted_writes
                    .fetch_add(result.interrupted as u64, Ordering::Relaxed);
                self.stats
                    .engine_to_guest_frames
                    .fetch_add(result.sent as u64, Ordering::Relaxed);
                self.stats
                    .engine_to_guest_bytes
                    .fetch_add(result.bytes as u64, Ordering::Relaxed);
                Ok(result.sent)
            }
            Err(FlushError::PartialDatagram) => {
                self.stats.frame_send_errors.fetch_add(1, Ordering::Relaxed);
                Err(FramePumpError::PartialDatagramSend)
            }
            Err(FlushError::Io(error)) if error.kind() == io::ErrorKind::Interrupted => {
                self.stats
                    .interrupted_writes
                    .fetch_add(1, Ordering::Relaxed);
                Ok(0)
            }
            Err(FlushError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                self.stats
                    .engine_send_eagain
                    .fetch_add(1, Ordering::Relaxed);
                Ok(0)
            }
            Err(FlushError::Io(_)) => {
                self.stats.frame_send_errors.fetch_add(1, Ordering::Relaxed);
                Err(FramePumpError::SocketSend)
            }
        }
    }
}

fn recv_datagram(fd: RawFd, buffer: &mut [u8]) -> io::Result<(usize, libc::c_int)> {
    let mut iovec = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iovec;
    message.msg_iovlen = 1;
    let read = unsafe { libc::recvmsg(fd, &mut message, libc::MSG_DONTWAIT) };
    if read < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok((read as usize, message.msg_flags))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Arp,
    Ipv4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameValidationError {
    TooShort,
    TooLarge,
    Ipv6,
    UnsupportedEtherType,
    MalformedArp,
    MalformedIpv4,
    GuestMacSpoof,
    GuestIpSpoof,
}

/// The target protocol address of an ARP frame.
fn arp_target(frame: &[u8]) -> Option<Ipv4Addr> {
    arp_address(frame, 24)
}

/// The sender protocol address of an ARP frame.
fn arp_sender(frame: &[u8]) -> Option<Ipv4Addr> {
    arp_address(frame, 14)
}

fn arp_address(frame: &[u8], offset: usize) -> Option<Ipv4Addr> {
    if frame.len() < ETH_HEADER_LEN + 28 || ether_type(frame) != 0x0806 {
        return None;
    }
    let bytes = &frame[ETH_HEADER_LEN + offset..ETH_HEADER_LEN + offset + 4];
    Some(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]))
}

pub fn validate_guest_frame(
    frame: &[u8],
    guest_mac: [u8; 6],
    guest_ipv4: Ipv4Addr,
    mtu: u16,
) -> Result<FrameKind, FrameValidationError> {
    validate_frame_structure(frame, mtu)?;
    if frame[6..12] != guest_mac {
        return Err(FrameValidationError::GuestMacSpoof);
    }
    match ether_type(frame) {
        0x0806 => {
            let arp = &frame[ETH_HEADER_LEN..];
            if arp[8..14] != guest_mac {
                return Err(FrameValidationError::GuestMacSpoof);
            }
            if arp[14..18] != guest_ipv4.octets() {
                return Err(FrameValidationError::GuestIpSpoof);
            }
            Ok(FrameKind::Arp)
        }
        0x0800 => {
            let ipv4 = &frame[ETH_HEADER_LEN..];
            if ipv4[12..16] != guest_ipv4.octets() {
                return Err(FrameValidationError::GuestIpSpoof);
            }
            Ok(FrameKind::Ipv4)
        }
        0x86dd => Err(FrameValidationError::Ipv6),
        _ => Err(FrameValidationError::UnsupportedEtherType),
    }
}

pub fn validate_engine_frame(frame: &[u8], mtu: u16) -> Result<FrameKind, FrameValidationError> {
    validate_frame_structure(frame, mtu)?;
    match ether_type(frame) {
        0x0806 => Ok(FrameKind::Arp),
        0x0800 => Ok(FrameKind::Ipv4),
        0x86dd => Err(FrameValidationError::Ipv6),
        _ => Err(FrameValidationError::UnsupportedEtherType),
    }
}

fn validate_frame_structure(frame: &[u8], mtu: u16) -> Result<(), FrameValidationError> {
    if frame.len() < ETH_HEADER_LEN {
        return Err(FrameValidationError::TooShort);
    }
    if frame.len() > usize::from(mtu) + ETH_HEADER_LEN {
        return Err(FrameValidationError::TooLarge);
    }
    match ether_type(frame) {
        0x0806 => validate_arp(frame),
        0x0800 => validate_ipv4(frame, mtu),
        0x86dd => Err(FrameValidationError::Ipv6),
        0x8100 | 0x88a8 => Err(FrameValidationError::UnsupportedEtherType),
        _ => Err(FrameValidationError::UnsupportedEtherType),
    }
}

fn validate_arp(frame: &[u8]) -> Result<(), FrameValidationError> {
    let arp = frame
        .get(ETH_HEADER_LEN..ETH_HEADER_LEN + 28)
        .ok_or(FrameValidationError::MalformedArp)?;
    let hardware = u16::from_be_bytes([arp[0], arp[1]]);
    let protocol = u16::from_be_bytes([arp[2], arp[3]]);
    let operation = u16::from_be_bytes([arp[6], arp[7]]);
    if hardware != 1
        || protocol != 0x0800
        || arp[4] != 6
        || arp[5] != 4
        || !matches!(operation, 1 | 2)
        || arp[8..14] != frame[6..12]
    {
        return Err(FrameValidationError::MalformedArp);
    }
    Ok(())
}

fn validate_ipv4(frame: &[u8], mtu: u16) -> Result<(), FrameValidationError> {
    let ip = frame
        .get(ETH_HEADER_LEN..)
        .ok_or(FrameValidationError::MalformedIpv4)?;
    if ip.len() < 20 || ip[0] >> 4 != 4 {
        return Err(FrameValidationError::MalformedIpv4);
    }
    let header_len = usize::from(ip[0] & 0x0f) * 4;
    let total_len = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
    if header_len < 20
        || header_len > ip.len()
        || total_len < header_len
        || total_len > usize::from(mtu)
        || total_len > ip.len()
    {
        return Err(FrameValidationError::MalformedIpv4);
    }
    Ok(())
}

fn ether_type(frame: &[u8]) -> u16 {
    u16::from_be_bytes([frame[12], frame[13]])
}

struct PendingFrames {
    frames: VecDeque<Vec<u8>>,
    capacity: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct FlushResult {
    sent: usize,
    bytes: usize,
    would_block: bool,
    interrupted: usize,
}

#[derive(Debug)]
enum FlushError {
    Io(io::Error),
    PartialDatagram,
}

impl PendingFrames {
    fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn is_full(&self) -> bool {
        self.frames.len() >= self.capacity
    }

    fn push(&mut self, frame: Vec<u8>) -> Result<(), Vec<u8>> {
        if self.is_full() {
            return Err(frame);
        }
        self.frames.push_back(frame);
        Ok(())
    }

    fn flush_with(
        &mut self,
        budget: usize,
        mut send: impl FnMut(&[u8]) -> io::Result<usize>,
    ) -> Result<FlushResult, FlushError> {
        let mut result = FlushResult {
            sent: 0,
            bytes: 0,
            would_block: false,
            interrupted: 0,
        };
        let mut attempts = 0;
        let attempt_limit = budget.saturating_mul(2).max(budget);
        while result.sent < budget && attempts < attempt_limit {
            let Some(frame) = self.frames.front() else {
                break;
            };
            attempts += 1;
            match send(frame) {
                Ok(n) if n == frame.len() => {
                    result.sent += 1;
                    result.bytes += n;
                    self.frames.pop_front();
                }
                Ok(_) => return Err(FlushError::PartialDatagram),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    result.interrupted += 1;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    result.would_block = true;
                    break;
                }
                Err(error) => return Err(FlushError::Io(error)),
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUEST_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const GUEST_IP: Ipv4Addr = Ipv4Addr::new(172, 16, 0, 2);

    #[test]
    fn reports_bounded_backend_ring_drops() {
        let stats = FrameStats::default();
        let snapshot = stats.snapshot_with_backend_tx_ring_drops(3);
        assert_eq!(snapshot.backend_tx_ring_drops, 3);
    }

    #[test]
    fn accepts_guest_ipv4_and_arp_but_rejects_mac_or_ip_spoofing() {
        let frame = ipv4_frame(GUEST_MAC, GUEST_IP);
        assert_eq!(
            validate_guest_frame(&frame, GUEST_MAC, GUEST_IP, 1500),
            Ok(FrameKind::Ipv4)
        );
        assert_eq!(
            validate_guest_frame(
                &ipv4_frame([2, 0, 0, 0, 0, 3], GUEST_IP),
                GUEST_MAC,
                GUEST_IP,
                1500
            ),
            Err(FrameValidationError::GuestMacSpoof)
        );
        assert_eq!(
            validate_guest_frame(
                &ipv4_frame(GUEST_MAC, Ipv4Addr::new(172, 16, 0, 6)),
                GUEST_MAC,
                GUEST_IP,
                1500
            ),
            Err(FrameValidationError::GuestIpSpoof)
        );
        assert_eq!(
            validate_guest_frame(&arp_frame(GUEST_MAC, GUEST_IP), GUEST_MAC, GUEST_IP, 1500),
            Ok(FrameKind::Arp)
        );
        assert_eq!(
            validate_guest_frame(
                &arp_frame(GUEST_MAC, Ipv4Addr::new(172, 16, 0, 6)),
                GUEST_MAC,
                GUEST_IP,
                1500
            ),
            Err(FrameValidationError::GuestIpSpoof)
        );
    }

    #[test]
    fn rejects_truncated_ipv4_ipv6_and_oversized_frames() {
        assert_eq!(
            validate_guest_frame(&[0; 13], GUEST_MAC, GUEST_IP, 1500),
            Err(FrameValidationError::TooShort)
        );
        let mut truncated = ipv4_frame(GUEST_MAC, GUEST_IP);
        truncated[14] = 0x4f;
        assert_eq!(
            validate_guest_frame(&truncated, GUEST_MAC, GUEST_IP, 1500),
            Err(FrameValidationError::MalformedIpv4)
        );
        let mut ipv6 = vec![0u8; ETH_HEADER_LEN];
        ipv6[6..12].copy_from_slice(&GUEST_MAC);
        ipv6[12..14].copy_from_slice(&0x86ddu16.to_be_bytes());
        assert_eq!(
            validate_guest_frame(&ipv6, GUEST_MAC, GUEST_IP, 1500),
            Err(FrameValidationError::Ipv6)
        );
        let oversized = vec![0u8; 1500 + ETH_HEADER_LEN + 1];
        assert_eq!(
            validate_engine_frame(&oversized, 1500),
            Err(FrameValidationError::TooLarge)
        );
    }

    #[test]
    fn pending_frames_survive_backpressure_and_remain_bounded() {
        let mut pending = PendingFrames::new(1);
        pending.push(vec![1, 2, 3]).unwrap();
        assert_eq!(pending.push(vec![4]), Err(vec![4]));
        let blocked = pending
            .flush_with(4, |_| Err(io::Error::from(io::ErrorKind::WouldBlock)))
            .unwrap();
        assert!(blocked.would_block);
        assert_eq!(pending.frames.front().unwrap(), &[1, 2, 3]);
        let sent = pending.flush_with(4, |frame| Ok(frame.len())).unwrap();
        assert_eq!(sent.sent, 1);
        assert_eq!(sent.bytes, 3);
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_frame_retries_interrupted_send_with_a_bounded_budget() {
        let mut pending = PendingFrames::new(1);
        pending.push(vec![1, 2, 3]).unwrap();
        let mut attempts = 0;
        let sent = pending
            .flush_with(2, |frame| {
                attempts += 1;
                if attempts < 4 {
                    Err(io::Error::from(io::ErrorKind::Interrupted))
                } else {
                    Ok(frame.len())
                }
            })
            .unwrap();
        assert_eq!(attempts, 4);
        assert_eq!(sent.interrupted, 3);
        assert_eq!(sent.sent, 1);
        assert!(pending.is_empty());
    }

    #[test]
    fn recvmsg_reports_truncated_datagram_without_slicing_past_buffer() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;

        let (sender, receiver) = UnixDatagram::pair().unwrap();
        sender.send(&[7; 32]).unwrap();
        let mut buffer = [0u8; 8];
        let (read, flags) = recv_datagram(receiver.as_raw_fd(), &mut buffer).unwrap();
        assert!(read > buffer.len() || flags & libc::MSG_TRUNC != 0);
    }

    /// A burst larger than one service batch must stay readable on the backend's wake
    /// descriptor until the ring is empty, without any further producer activity.
    #[test]
    fn a_burst_over_the_batch_limit_is_fully_delivered_without_new_producer_activity() {
        use microsandbox_network::netstack::{backend::SmoltcpBackend, shared::SharedState};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;
        use std::sync::Arc;

        const BURST: usize = MAX_BATCH + 1;
        let shared = Arc::new(SharedState::new(1024));
        let (gateway_end, guest_end) = UnixDatagram::pair().unwrap();
        guest_end.set_nonblocking(true).unwrap();
        let stats = FrameStats::default();
        let mut pump = FramePump::new(
            gateway_end.as_raw_fd(),
            GUEST_IP,
            GUEST_MAC,
            1500,
            &stats,
            Box::new(SmoltcpBackend::new(shared.clone())),
        );
        // The network stack pushes a frame and then wakes the reader, once per frame.
        for index in 0..BURST {
            let mut frame = ipv4_frame(GUEST_MAC, GUEST_IP);
            frame[0] = index as u8;
            shared.rx_ring.push(frame).unwrap();
            shared.rx_wake.wake();
        }

        let mut delivered = 0;
        let mut buffer = [0u8; FRAME_BUFFER_CAP];
        loop {
            let events = pump.poll_events();
            let mut pollfds = [events.frame, events.backend];
            let ready = unsafe { libc::poll(pollfds.as_mut_ptr(), 2, 0) };
            assert!(ready >= 0);
            let backend_ready = pollfds[1].revents & libc::POLLIN != 0;
            let flush_ready = pollfds[0].revents & libc::POLLOUT != 0;
            if !backend_ready && !flush_ready {
                break;
            }
            pump.service(pollfds[0].revents, pollfds[1].revents)
                .unwrap();
            while guest_end.recv(&mut buffer).is_ok() {
                delivered += 1;
            }
        }
        assert_eq!(delivered, BURST);
        assert!(shared.rx_ring.is_empty());
    }

    /// macOS gives its address up when something else answers for it, and the engine answers
    /// every ARP request, so the guest's announcement of its own address must not reach it.
    #[test]
    fn nothing_answers_for_the_guests_own_address() {
        use microsandbox_network::netstack::{backend::SmoltcpBackend, shared::SharedState};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;
        use std::sync::Arc;

        let shared = Arc::new(SharedState::new(1024));
        let (gateway_end, guest_end) = UnixDatagram::pair().unwrap();
        guest_end.set_nonblocking(true).unwrap();
        let stats = FrameStats::default();
        let mut pump = FramePump::new(
            gateway_end.as_raw_fd(),
            GUEST_IP,
            GUEST_MAC,
            1500,
            &stats,
            Box::new(SmoltcpBackend::new(shared.clone())),
        );
        let run = |pump: &mut FramePump| {
            let events = pump.poll_events();
            let mut pollfds = [events.frame, events.backend];
            let ready = unsafe { libc::poll(pollfds.as_mut_ptr(), 2, 0) };
            assert!(ready >= 0);
            pump.service(pollfds[0].revents, pollfds[1].revents)
                .unwrap();
        };
        let dropped = || {
            stats
                .snapshot_with_backend_tx_ring_drops(0)
                .arp_about_guest_dropped
        };
        let mut buffer = [0u8; FRAME_BUFFER_CAP];

        // The guest's announcement (sender and target are its own address) never reaches the engine.
        let mut announcement = arp_frame(GUEST_MAC, GUEST_IP);
        announcement[ETH_HEADER_LEN + 24..ETH_HEADER_LEN + 28].copy_from_slice(&GUEST_IP.octets());
        assert_eq!(arp_target(&announcement), Some(GUEST_IP));
        guest_end.send(&announcement).unwrap();
        run(&mut pump);
        assert_eq!(dropped(), 1);
        assert_eq!(
            stats
                .snapshot_with_backend_tx_ring_drops(0)
                .guest_to_engine_frames,
            0
        );

        // Its question about the gateway does.
        guest_end.send(&arp_frame(GUEST_MAC, GUEST_IP)).unwrap();
        run(&mut pump);
        assert_eq!(
            stats
                .snapshot_with_backend_tx_ring_drops(0)
                .guest_to_engine_frames,
            1
        );

        // A reply that claims the guest's address never reaches the guest; the engine's request
        // for the guest, and a reply for another address, do.
        let mut claim = arp_frame([0x02, 0, 0, 0, 0, 1], GUEST_IP);
        claim[ETH_HEADER_LEN + 6..ETH_HEADER_LEN + 8].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(arp_sender(&claim), Some(GUEST_IP));
        shared.rx_ring.push(claim).unwrap();
        shared.rx_wake.wake();
        run(&mut pump);
        assert!(guest_end.recv(&mut buffer).is_err());
        assert_eq!(dropped(), 2);

        let mut request = arp_frame([0x02, 0, 0, 0, 0, 1], Ipv4Addr::new(172, 16, 0, 1));
        request[ETH_HEADER_LEN + 24..ETH_HEADER_LEN + 28].copy_from_slice(&GUEST_IP.octets());
        shared.rx_ring.push(request).unwrap();
        shared.rx_wake.wake();
        run(&mut pump);
        assert!(guest_end.recv(&mut buffer).is_ok());
        assert_eq!(dropped(), 2);
    }

    /// While the guest boots nothing reads the frame socket. macOS then answers ENOBUFS, which
    /// used to stop the gateway ("The computer network gateway is unavailable").
    #[test]
    fn a_full_guest_queue_drops_frames_instead_of_stopping_the_gateway() {
        use microsandbox_network::netstack::{backend::SmoltcpBackend, shared::SharedState};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;
        use std::sync::Arc;

        let shared = Arc::new(SharedState::new(1024));
        let (gateway_end, _guest_end) = UnixDatagram::pair().unwrap();
        gateway_end.set_nonblocking(true).unwrap();
        let stats = FrameStats::default();
        let mut pump = FramePump::new(
            gateway_end.as_raw_fd(),
            GUEST_IP,
            GUEST_MAC,
            1500,
            &stats,
            Box::new(SmoltcpBackend::new(shared)),
        );
        // Nothing reads the other end, so the queue fills up.
        let frame = ipv4_frame(GUEST_MAC, GUEST_IP);
        let mut filled = false;
        for _ in 0..100_000 {
            if gateway_end.send(&frame).is_err() {
                filled = true;
                break;
            }
        }
        assert!(filled, "the queue never filled");
        pump.pending.push(frame).unwrap();
        pump.flush_pending(1).unwrap();
        assert!(pump.pending.is_empty());
    }

    fn ipv4_frame(source_mac: [u8; 6], source_ip: Ipv4Addr) -> Vec<u8> {
        let mut frame = vec![0u8; ETH_HEADER_LEN + 20];
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&source_mac);
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[16..18].copy_from_slice(&20u16.to_be_bytes());
        frame[22] = 64;
        frame[23] = 1;
        frame[26..30].copy_from_slice(&source_ip.octets());
        frame[30..34].copy_from_slice(&Ipv4Addr::new(1, 1, 1, 1).octets());
        frame
    }

    fn arp_frame(source_mac: [u8; 6], source_ip: Ipv4Addr) -> Vec<u8> {
        let mut frame = vec![0u8; ETH_HEADER_LEN + 28];
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&source_mac);
        frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
        let arp = &mut frame[ETH_HEADER_LEN..];
        arp[..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&1u16.to_be_bytes());
        arp[8..14].copy_from_slice(&source_mac);
        arp[14..18].copy_from_slice(&source_ip.octets());
        arp[24..28].copy_from_slice(&Ipv4Addr::new(172, 16, 0, 1).octets());
        frame
    }
}
