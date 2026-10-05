//! The network stack end to end: a fake interface driver whose other end is an in-memory
//! ARP responder and TFTP server, and libsa's `netif`, `ether`, `arp`, `netudp` and `tftp`
//! talking to it, directly and through `open()`/`read()`/`lseek()`.

extern crate std;

use core::ffi::c_void;
use core::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::vec;

use super::*;
use crate::arp::arp_num;
use crate::globals::GATEIP;
use crate::hdr::in_::InAddr;
use crate::netif::{Netif, NetifDif, NetifDriver, NetifStats, netif_close, netif_open};
use crate::stand::{Devsw, FsOps, SaConf, sa_conf_register};
use crate::testutil;

const CLIENT_EA: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const SERVER_EA: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x02];
const CLIENT_IP: [u8; 4] = [10, 0, 2, 15];
const SERVER_IP: [u8; 4] = [10, 0, 2, 2];
/// The server's transfer port (its TID).
const SERVER_TID: u16 = 3000;

/// The other end of the wire.
struct Server {
    /// Frames on their way to the client.
    rx: VecDeque<Vec<u8>>,
    /// Requests the server answers: file name and contents.
    files: Vec<(&'static [u8], Vec<u8>)>,
    /// The file being transferred.
    file: Vec<u8>,
    /// The client's port.
    client_port: u16,
    /// Read requests seen.
    rrqs: usize,
    /// ARP requests for the server's address answered.
    arps: usize,
    /// ARP replies the client sent to the server's request.
    arp_replies: usize,
    /// ACKs seen, by block.
    acks: Vec<u16>,
    /// ERROR packets the client sent (it stopped a transfer).
    client_errors: usize,
    /// Before this block, ask the client who it is (ARP).
    arp_before_block: Option<u16>,
    /// Send the DATA blocks without a UDP checksum.
    no_udp_sum: bool,
    /// Answer read requests with this TFTP error code instead.
    error_code: Option<u16>,
}

static SERVER: Mutex<Server> = Mutex::new(Server {
    rx: VecDeque::new(),
    files: Vec::new(),
    file: Vec::new(),
    client_port: 0,
    rrqs: 0,
    arps: 0,
    arp_replies: 0,
    acks: Vec::new(),
    client_errors: 0,
    arp_before_block: None,
    no_udp_sum: false,
    error_code: None,
});

/// The fake clock: a second per call.
static CLOCK: AtomicI64 = AtomicI64::new(1000);

fn server() -> std::sync::MutexGuard<'static, Server> {
    SERVER.lock().unwrap_or_else(|e| e.into_inner())
}

/// The Internet checksum, written independently of `in_cksum`: the bytes to store.
fn cksum(b: &[u8]) -> [u8; 2] {
    let mut sum: u32 = 0;
    for w in b.chunks(2) {
        sum += u32::from(w[0]) << 8 | u32::from(*w.get(1).unwrap_or(&0));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    (!(sum as u16)).to_be_bytes()
}

fn ether(dst: [u8; 6], etype: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&dst);
    f.extend_from_slice(&SERVER_EA);
    f.extend_from_slice(&etype.to_be_bytes());
    f.extend_from_slice(payload);
    f
}

fn arp(op: u16, sha: [u8; 6], spa: [u8; 4], tha: [u8; 6], tpa: [u8; 4]) -> Vec<u8> {
    let mut a = vec![0, 1, 0x08, 0x00, 6, 4];
    a.extend_from_slice(&op.to_be_bytes());
    a.extend_from_slice(&sha);
    a.extend_from_slice(&spa);
    a.extend_from_slice(&tha);
    a.extend_from_slice(&tpa);
    a
}

/// A UDP datagram from the server to the client, in an Ethernet frame.
fn udp_frame(sport: u16, dport: u16, payload: &[u8], with_sum: bool) -> Vec<u8> {
    let ulen = (8 + payload.len()) as u16;
    let mut ip = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 17, 0, 0];
    ip[2..4].copy_from_slice(&(20 + ulen).to_be_bytes());
    ip.extend_from_slice(&SERVER_IP);
    ip.extend_from_slice(&CLIENT_IP);
    let s = cksum(&ip);
    ip[10..12].copy_from_slice(&s);

    let mut udp = Vec::new();
    udp.extend_from_slice(&sport.to_be_bytes());
    udp.extend_from_slice(&dport.to_be_bytes());
    udp.extend_from_slice(&ulen.to_be_bytes());
    udp.extend_from_slice(&[0, 0]);
    udp.extend_from_slice(payload);
    if with_sum {
        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&SERVER_IP);
        pseudo.extend_from_slice(&CLIENT_IP);
        pseudo.extend_from_slice(&[0, 17]);
        pseudo.extend_from_slice(&ulen.to_be_bytes());
        pseudo.extend_from_slice(&udp);
        let s = cksum(&pseudo);
        udp[6..8].copy_from_slice(&s);
    }
    ip.extend_from_slice(&udp);
    ether(CLIENT_EA, 0x0800, &ip)
}

impl Server {
    fn nblocks(&self) -> u16 {
        (self.file.len() / 512 + 1) as u16
    }

    fn send_block(&mut self, n: u16) {
        if self.arp_before_block == Some(n) {
            let req = arp(1, SERVER_EA, SERVER_IP, [0; 6], CLIENT_IP);
            self.rx.push_back(ether([0xff; 6], 0x0806, &req));
        }
        let from = (usize::from(n) - 1) * 512;
        let to = (from + 512).min(self.file.len());
        let mut p = vec![0, 3];
        p.extend_from_slice(&n.to_be_bytes());
        p.extend_from_slice(&self.file[from..to]);
        let f = udp_frame(SERVER_TID, self.client_port, &p, !self.no_udp_sum);
        self.rx.push_back(f);
    }

    /// The server's side of a frame the client sent.
    fn input(&mut self, f: &[u8]) {
        assert_eq!(&f[6..12], &CLIENT_EA, "the client's source address");
        match u16::from_be_bytes([f[12], f[13]]) {
            0x0806 => {
                let a = &f[14..];
                let op = u16::from_be_bytes([a[6], a[7]]);
                if op == 1 && a[24..28] == SERVER_IP {
                    assert_eq!(&f[0..6], &[0xff; 6], "requests are broadcast");
                    assert_eq!(&a[8..14], &CLIENT_EA);
                    assert_eq!(&a[14..18], &CLIENT_IP);
                    self.arps += 1;
                    let rep = arp(2, SERVER_EA, SERVER_IP, CLIENT_EA, CLIENT_IP);
                    self.rx.push_back(ether(CLIENT_EA, 0x0806, &rep));
                } else if op == 2 {
                    assert_eq!(&f[0..6], &SERVER_EA, "the reply goes to the asker");
                    assert_eq!(&a[8..14], &CLIENT_EA);
                    assert_eq!(&a[14..18], &CLIENT_IP);
                    assert_eq!(&a[18..24], &SERVER_EA);
                    assert_eq!(&a[24..28], &SERVER_IP);
                    assert_eq!(f.len(), 14 + 46, "padded to 46 bytes");
                    self.arp_replies += 1;
                }
            }
            0x0800 => {
                assert_eq!(&f[0..6], &SERVER_EA);
                let ip = &f[14..];
                assert_eq!(cksum(&ip[..20]), [0, 0], "the IP checksum");
                assert_eq!(ip[0], 0x45);
                assert_eq!(ip[9], 17);
                assert_eq!(ip[8], 4, "ip_ttl is IP_TTL");
                assert_eq!(&ip[12..16], &CLIENT_IP);
                assert_eq!(&ip[16..20], &SERVER_IP);
                let iplen = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
                assert_eq!(iplen, ip.len());
                let udp = &ip[20..iplen];
                let ulen = u16::from_be_bytes([udp[4], udp[5]]);
                assert_eq!(usize::from(ulen), udp.len());
                let mut pseudo = Vec::new();
                pseudo.extend_from_slice(&CLIENT_IP);
                pseudo.extend_from_slice(&SERVER_IP);
                pseudo.extend_from_slice(&[0, 17]);
                pseudo.extend_from_slice(&ulen.to_be_bytes());
                pseudo.extend_from_slice(udp);
                assert_eq!(cksum(&pseudo), [0, 0], "the UDP checksum");
                let sport = u16::from_be_bytes([udp[0], udp[1]]);
                let dport = u16::from_be_bytes([udp[2], udp[3]]);
                let t = &udp[8..];
                let op = u16::from_be_bytes([t[0], t[1]]);
                if dport == 69 {
                    assert_eq!(op, 1, "RRQ");
                    let name_end = 2 + t[2..].iter().position(|&c| c == 0).unwrap();
                    let name = &t[2..name_end];
                    assert_eq!(&t[name_end + 1..], b"octet\0");
                    self.rrqs += 1;
                    self.client_port = sport;
                    if let Some(code) = self.error_code {
                        let mut p = vec![0, 5];
                        p.extend_from_slice(&code.to_be_bytes());
                        p.extend_from_slice(b"no\0");
                        let f = udp_frame(SERVER_TID, sport, &p, true);
                        self.rx.push_back(f);
                        return;
                    }
                    match self.files.iter().find(|(n, _)| *n == name) {
                        Some((_, data)) => {
                            self.file = data.clone();
                            self.send_block(1);
                        }
                        None => {
                            let mut p = vec![0, 5, 0, 1];
                            p.extend_from_slice(b"File not found\0");
                            let f = udp_frame(SERVER_TID, sport, &p, true);
                            self.rx.push_back(f);
                        }
                    }
                } else {
                    assert_eq!(dport, SERVER_TID, "the client answers the server's TID");
                    assert_eq!(sport, self.client_port);
                    match op {
                        4 => {
                            let n = u16::from_be_bytes([t[2], t[3]]);
                            self.acks.push(n);
                            if n < self.nblocks() {
                                self.send_block(n + 1);
                            }
                        }
                        5 => self.client_errors += 1,
                        _ => panic!("unexpected TFTP opcode {op}"),
                    }
                }
            }
            t => panic!("unexpected ether type {t:#x}"),
        }
    }
}

fn fake_match(_nif: &mut Netif, _hint: &[u8]) -> i32 {
    1
}

fn fake_probe(_nif: &mut Netif, hint: &[u8]) -> i32 {
    if hint.starts_with(b"fake") { 0 } else { -1 }
}

fn fake_init(desc: &mut IoDesc, _hint: &[u8]) {
    desc.myea = CLIENT_EA;
    desc.myip = InAddr {
        s_addr: u32::from_ne_bytes(CLIENT_IP),
    };
    desc.xid = 1;
}

fn fake_get(_desc: &mut IoDesc, pkt: &mut [u8], _timo: Time) -> Result<usize, Errno> {
    let Some(f) = server().rx.pop_front() else {
        return Err(Errno::EIO);
    };
    let n = f.len().min(pkt.len());
    pkt[..n].copy_from_slice(&f[..n]);
    Ok(n)
}

fn fake_put(_desc: &mut IoDesc, pkt: &[u8]) -> Result<usize, Errno> {
    server().input(pkt);
    Ok(pkt.len())
}

fn fake_end(_nif: &mut Netif) {}

static FAKE_STATS: NetifStats = NetifStats::new();
static FAKE_IFS: [NetifDif; 1] = [NetifDif::new(0, 1, &FAKE_STATS, core::ptr::null_mut())];
static FAKE_DRIVER: NetifDriver = NetifDriver {
    netif_bname: "fake",
    netif_match: fake_match,
    netif_probe: fake_probe,
    netif_init: fake_init,
    netif_get: fake_get,
    netif_put: fake_put,
    netif_end: fake_end,
    netif_ifs: &FAKE_IFS,
};
static DRIVERS: [&NetifDriver; 1] = [&FAKE_DRIVER];

/// The socket the `tftp` device opened (efiboot's `tftpdev_sock`).
static TFTPDEV_SOCK: AtomicUsize = AtomicUsize::new(usize::MAX);

fn tftpdev_open(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let Some(rest) = file.strip_prefix(b"tftp:") else {
        return Err(Errno::ENXIO);
    };
    let sock = netif_open(b"fake")?;
    TFTPDEV_SOCK.store(sock, Ordering::Relaxed);
    f.f_devdata = TFTPDEV_SOCK.as_ptr().cast::<c_void>();
    *file = rest;
    Ok(())
}

fn tftpdev_close(_f: &mut OpenFile) -> Result<(), Errno> {
    netif_close(TFTPDEV_SOCK.load(Ordering::Relaxed))
}

fn tftpdev_strategy(
    _devdata: *mut c_void,
    _rw: i32,
    _blk: crate::hdr::types::Daddr,
    _buf: &mut [u8],
    _rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

fn tftpdev_ioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

static DEVSW: [Devsw; 1] = [Devsw {
    dv_name: "tftp",
    dv_strategy: tftpdev_strategy,
    dv_open: tftpdev_open,
    dv_close: tftpdev_close,
    dv_ioctl: tftpdev_ioctl,
}];

static FILE_SYSTEM: [FsOps; 1] = [FsOps {
    open: tftp_open,
    close: tftp_close,
    read: tftp_read,
    write: tftp_write,
    seek: tftp_seek,
    stat: tftp_stat,
    readdir: tftp_readdir,
    fchmod: None,
}];

fn devopen<'a>(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno> {
    let mut file = fname;
    (DEVSW[0].dv_open)(f, &mut file)?;
    f.f_dev = Some(&DEVSW[0]);
    Ok(file)
}

static NET_CONF: SaConf = SaConf {
    file_system: &FILE_SYSTEM,
    devsw: &DEVSW,
    constab: &testutil::CONSTAB,
    devopen,
    rtt: || panic!("_rtt"),
    loadaddr: |a, offset| a.wrapping_add(offset),
    netif_drivers: &DRIVERS,
    getsecs: || CLOCK.fetch_add(1, Ordering::Relaxed),
};

/// A file of `len` bytes that tells its offsets apart.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
}

/// Takes libsa's globals, registers the network configuration and a fresh server serving
/// `files`, and empties the ARP cache.
fn net_setup(files: Vec<(&'static [u8], Vec<u8>)>) -> std::sync::MutexGuard<'static, ()> {
    let guard = testutil::setup();
    sa_conf_register(&NET_CONF);
    SERVIP.store(u32::from_ne_bytes(SERVER_IP), Ordering::Relaxed);
    // As efiboot's `efi_pxeprobe`: the gateway is the server.
    GATEIP.store(u32::from_ne_bytes(SERVER_IP), Ordering::Relaxed);
    arp_num.store(1, Ordering::Relaxed);
    let mut s = server();
    s.rx.clear();
    s.files = files;
    s.file.clear();
    s.rrqs = 0;
    s.arps = 0;
    s.arp_replies = 0;
    s.acks.clear();
    s.client_errors = 0;
    s.arp_before_block = None;
    s.no_udp_sum = false;
    s.error_code = None;
    drop(s);
    guard
}

/// An open file on the socket `sock` (what the `tftp` device's open leaves).
fn open_file(sock: &'static AtomicUsize) -> OpenFile {
    let mut f = OpenFile::new();
    f.f_devdata = sock.as_ptr().cast::<c_void>();
    f
}

#[test]
fn tftp_reads_a_file_end_to_end() {
    let data = pattern(1300);
    let _g = net_setup(vec![(b"bsd", data.clone())]);
    static SOCK: AtomicUsize = AtomicUsize::new(0);
    SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
    let mut f = open_file(&SOCK);

    tftp_open(b"bsd\0", &mut f).unwrap();
    {
        let s = server();
        assert_eq!(s.arps, 1, "the client asked who has the server");
        assert_eq!(s.rrqs, 1);
    }

    let mut out = Vec::new();
    let mut buf = [0u8; 300];
    loop {
        let mut resid = 0;
        tftp_read(&mut f, &mut buf, &mut resid).unwrap();
        out.extend_from_slice(&buf[..buf.len() - resid]);
        if resid != 0 {
            break;
        }
    }
    assert_eq!(out, data);

    let mut sb = Stat::default();
    tftp_stat(&mut f, &mut sb).unwrap();
    assert_eq!((sb.st_mode, sb.st_size), (0o444, -1));
    assert_eq!(tftp_write(&mut f, &[], &mut 0), Err(Errno::EROFS));
    assert_eq!(tftp_readdir(&mut f, None), Err(Errno::EROFS));
    assert_eq!(tftp_seek(&mut f, 0, 2), Err(Errno::EOFFSET));

    tftp_close(&mut f).unwrap();
    assert_eq!(
        server().acks,
        [1, 2, 3],
        "the last block is acknowledged at close"
    );
    netif_close(SOCK.load(Ordering::Relaxed)).unwrap();
    assert_eq!(netif_close(SOCK.load(Ordering::Relaxed)), Err(Errno::EBADF));
}

#[test]
fn tftp_through_open_read_and_lseek() {
    use crate::close::oclose;
    use crate::lseek::olseek;
    use crate::open::oopen;
    use crate::read::oread;

    // A multiple of SEGSIZE: the last block is empty.
    let data = pattern(1024);
    let _g = net_setup(vec![(b"/bsd.rd", data.clone())]);

    let fd = oopen(b"tftp:/bsd.rd", 0).unwrap();
    let mut buf = vec![0u8; 700];
    assert_eq!(oread(fd, &mut buf).unwrap(), 700);
    assert_eq!(buf, data[..700]);

    // Seek backwards: the client stops the transfer and asks again.
    assert_eq!(olseek(fd, 100, SEEK_SET).unwrap(), 100);
    let mut all = vec![0u8; 2000];
    assert_eq!(oread(fd, &mut all).unwrap(), 924);
    assert_eq!(all[..924], data[100..]);
    {
        let s = server();
        assert_eq!(s.rrqs, 2);
        assert_eq!(s.client_errors, 1, "the abandoned transfer was told so");
    }
    assert_eq!(oread(fd, &mut all).unwrap(), 0);
    oclose(fd).unwrap();
    assert_eq!(*server().acks.last().unwrap(), 3);
}

#[test]
fn tftp_answers_arp_and_reads_unchecksummed_blocks() {
    let data = pattern(2000);
    let _g = net_setup(vec![(b"bsd", data.clone())]);
    {
        let mut s = server();
        s.arp_before_block = Some(2);
        s.no_udp_sum = true;
    }
    static SOCK: AtomicUsize = AtomicUsize::new(0);
    SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
    let mut f = open_file(&SOCK);
    tftp_open(b"bsd", &mut f).unwrap();
    let mut buf = vec![0u8; 2000];
    let mut resid = 0;
    tftp_read(&mut f, &mut buf, &mut resid).unwrap();
    assert_eq!((resid, &buf[..]), (0, &data[..]));
    assert_eq!(
        server().arp_replies,
        1,
        "readudp answered the server's ARP request"
    );
    tftp_close(&mut f).unwrap();
    netif_close(SOCK.load(Ordering::Relaxed)).unwrap();
}

#[test]
fn tftp_errors() {
    let _g = net_setup(vec![]);
    static SOCK: AtomicUsize = AtomicUsize::new(0);
    SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
    let mut f = open_file(&SOCK);

    assert_eq!(tftp_open(b"nonexistent", &mut f), Err(Errno::ENOENT));
    server().error_code = Some(9);
    assert_eq!(tftp_open(b"x", &mut f), Err(Errno::EIO));
    assert!(testutil::output().contains("illegal tftp error 9"));
    server().error_code = Some(6);
    assert_eq!(tftp_open(b"x", &mut f), Err(Errno::EEXIST));
    assert!(f.f_fsdata.is_none());

    let long = [b'a'; 200];
    assert_eq!(
        tftp_open(&long, &mut f),
        Err(Errno::ENOENT),
        "too long a path"
    );
    netif_close(SOCK.load(Ordering::Relaxed)).unwrap();

    // A hint no driver probes.
    assert_eq!(netif_open(b"nope"), Err(Errno::EINVAL));
    assert!(testutil::output().contains("netboot: couldn't probe fake0"));
}

#[test]
fn sendrecv_times_out() {
    let _g = net_setup(vec![]);
    let mut d = IoDesc::new();
    static SENT: AtomicUsize = AtomicUsize::new(0);
    SENT.store(0, Ordering::Relaxed);
    let mut sbuf = [0u8; 4];
    let mut rbuf = [0u8; 4];
    let r = sendrecv(
        &mut d,
        |_, pkt, off| {
            SENT.fetch_add(1, Ordering::Relaxed);
            Ok(pkt.len() - off)
        },
        &mut sbuf,
        0,
        |_, _, _, _| {
            crate::dev::set_errno(Errno(0));
            Err(Errno(0))
        },
        &mut rbuf,
        0,
    );
    assert_eq!(r, Err(Errno::ETIMEDOUT));
    assert_eq!(errno(), Errno::ETIMEDOUT);
    // Timeouts of 2, 4, 8 and 16 seconds, then MAXTMO.
    assert_eq!(SENT.load(Ordering::Relaxed), 4);
}
