use super::*;

extern crate std;
use std::vec;
use std::vec::Vec;

use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::malloc::M_WAITOK;
use crate::sys::param::DEV_BSIZE;

/// The strip size of the in-memory volumes (small, to keep them small).
const STRIP: i64 = 4096;
/// Its shift.
const BITS: i64 = 12;

/// A deterministic byte stream (an LCG), for test data.
fn bytes(seed: u32, n: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (x >> 16) as u8
        })
        .collect()
}

/// Reference GF(2^8) multiplication modulo x^8 + x^4 + x^3 + x^2 + 1 (0x11D), bit by bit,
/// independent of the tables under test.
fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        let hi = a & 0x80 != 0;
        a <<= 1;
        if hi {
            a ^= 0x1D;
        }
        b >>= 1;
    }
    p
}

/// Reference `2^i` in GF(2^8).
fn gpow(i: usize) -> u8 {
    (0..i).fold(1u8, |x, _| gmul(x, 2))
}

/// Which work unit of a strip an I/O is queued on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum W {
    /// `wu`: the transfer's own.
    Wu,
    /// `wu_r`: the reads of a write, which run first.
    WuR,
}

/// One queued `sr_raid6_addio`.
#[derive(Clone, Copy, Debug)]
struct Op {
    wu: W,
    chunk: i64,
    blkno: Daddr,
    len: i64,
    data: usize,
    read: bool,
    freebuf: bool,
    pbuf: Option<usize>,
    qbuf: Option<usize>,
    gn: u8,
}

/// A RAID 6 volume in memory: the chunks' bytes and states, an arena of buffers (a
/// [`Raid6Io::Buf`] is an index into it; `None` once freed) and the queued I/O.
struct Mem {
    chunks: Vec<Vec<u8>>,
    status: Vec<i32>,
    bufs: Vec<Option<Vec<u8>>>,
    ops: Vec<Op>,
    /// Fail the `addio` call with this index (counting down), to test the error paths.
    fail_in: Option<usize>,
}

impl Mem {
    /// `n` online, zeroed chunks of `rows` strips (a consistent volume: all parities 0).
    fn new(n: usize, rows: usize) -> Self {
        Mem {
            chunks: vec![vec![0; rows * STRIP as usize]; n],
            status: vec![BIOC_SDONLINE; n],
            bufs: Vec::new(),
            ops: Vec::new(),
            fail_in: None,
        }
    }

    fn n(&self) -> i64 {
        self.chunks.len() as i64
    }

    fn rows(&self) -> usize {
        self.chunks[0].len() / STRIP as usize
    }

    /// The volume's data bytes.
    fn size(&self) -> usize {
        self.rows() * STRIP as usize * (self.chunks.len() - 2)
    }

    fn add(&mut self, b: Vec<u8>) -> usize {
        self.bufs.push(Some(b));
        self.bufs.len() - 1
    }

    fn take(&mut self, i: usize) -> Vec<u8> {
        self.bufs[i].take().expect("buffer already freed")
    }

    fn buf(&mut self, i: usize) -> &mut Vec<u8> {
        self.bufs[i].as_mut().expect("use of a freed buffer")
    }

    fn live(&self) -> usize {
        self.bufs.iter().filter(|b| b.is_some()).count()
    }

    /// Runs the queued I/O as the kernel does: the read work unit first, then the
    /// transfer's own (which it collides with).
    fn run(&mut self) {
        let ops = core::mem::take(&mut self.ops);
        for w in [W::WuR, W::Wu] {
            for op in ops.iter().filter(|o| o.wu == w) {
                self.exec(op);
            }
        }
    }

    fn exec(&mut self, op: &Op) {
        let c = op.chunk as usize;
        let off = op.blkno as usize * DEV_BSIZE;
        let len = op.len as usize;
        if op.read {
            assert!(
                matches!(self.status[c], BIOC_SDONLINE | BIOC_SDSCRUB),
                "read of chunk {c} in state {}",
                self.status[c]
            );
            let got = self.chunks[c][off..off + len].to_vec();
            self.buf(op.data)[..len].copy_from_slice(&got);
            if let Some(p) = op.pbuf {
                sr_raid6_xorp(self.buf(p), &got, len);
            }
            if let Some(q) = op.qbuf {
                sr_raid6_xorq(self.buf(q), &got, len, op.gn);
            }
        } else {
            assert!(
                matches!(self.status[c], BIOC_SDONLINE | BIOC_SDSCRUB),
                "write of chunk {c} in state {}",
                self.status[c]
            );
            let src = self.buf(op.data)[..len].to_vec();
            self.chunks[c][off..off + len].copy_from_slice(&src);
        }
        if op.freebuf {
            self.bufs[op.data] = None;
        }
    }

    /// Drops the queued I/O unrun (a work unit put back after an error), freeing the blocks
    /// the ccbs own as a leak-free core would.
    fn discard(&mut self) {
        for op in core::mem::take(&mut self.ops) {
            if op.freebuf {
                self.bufs[op.data] = None;
            }
        }
    }

    /// The bytes of strip-row `row` of chunk `c`.
    fn strip(&self, c: i64, row: usize) -> &[u8] {
        &self.chunks[c as usize][row * STRIP as usize..(row + 1) * STRIP as usize]
    }

    /// Every row's P is the XOR of its data strips and its Q the GF(256) sum of `g^i` times
    /// the data strip of chunk `i`, computed with the reference multiplication.
    fn assert_pq(&self) {
        let nd = self.n() - 2;
        for row in 0..self.rows() {
            let st = Raid6Strip::new((row as i64 * nd) << BITS, STRIP, STRIP, BITS, nd);
            let mut p = vec![0u8; STRIP as usize];
            let mut q = vec![0u8; STRIP as usize];
            for c in 0..self.n() {
                if c == st.pchunk || c == st.qchunk {
                    continue;
                }
                let g = gpow(c as usize);
                for (j, &d) in self.strip(c, row).iter().enumerate() {
                    p[j] ^= d;
                    q[j] ^= gmul(g, d);
                }
            }
            assert!(p == self.strip(st.pchunk, row), "row {row}: P is wrong");
            assert!(q == self.strip(st.qchunk, row), "row {row}: Q is wrong");
        }
    }
}

impl Raid6Io for Mem {
    type Wu = W;
    type Buf = usize;

    fn chunk_status(&self, chunk: i64) -> i32 {
        self.status[chunk as usize]
    }

    fn addio(
        &mut self,
        wu: W,
        chunk: i64,
        blkno: Daddr,
        len: i64,
        data: Option<usize>,
        xsflags: i32,
        mut ccbflags: i32,
        pbuf: Option<usize>,
        qbuf: Option<usize>,
        gn: u8,
    ) -> Result<(), Errno> {
        if let Some(n) = self.fail_in {
            if n == 0 {
                // as sr_ccb_rw failing: a block handed over with FREEBUF goes back
                if let Some(d) = data
                    && ccbflags & SR_CCBF_FREEBUF != 0
                {
                    self.bufs[d] = None;
                }
                return Err(Errno::EIO);
            }
            self.fail_in = Some(n - 1);
        }
        let data = match data {
            Some(d) => d,
            None => {
                ccbflags |= SR_CCBF_FREEBUF;
                self.add(vec![0; len as usize])
            }
        };
        if qbuf.is_some() {
            gf_premul(gn)?;
        }
        self.ops.push(Op {
            wu,
            chunk,
            blkno,
            len,
            data,
            read: xsflags & SCSI_DATA_IN != 0,
            freebuf: ccbflags & SR_CCBF_FREEBUF != 0,
            pbuf,
            qbuf,
            gn,
        });
        Ok(())
    }

    fn block_get(&mut self, len: i64) -> Option<usize> {
        Some(self.add(vec![0; len as usize]))
    }

    fn block_put(&mut self, buf: usize, _len: i64) {
        assert!(self.bufs[buf].take().is_some(), "double free");
    }

    fn zero(&mut self, buf: usize, len: i64) {
        self.buf(buf)[..len as usize].fill(0);
    }

    fn xorp(&mut self, p: usize, d: usize, len: i64) {
        assert_ne!(p, d);
        let src = self.buf(d)[..len as usize].to_vec();
        sr_raid6_xorp(self.buf(p), &src, len as usize);
    }

    fn xorq(&mut self, q: usize, d: usize, len: i64, gn: u8) {
        assert_ne!(q, d);
        let src = self.buf(d)[..len as usize].to_vec();
        sr_raid6_xorq(self.buf(q), &src, len as usize, gn);
    }
}

/// A read or write of `data.len()` bytes at byte `offset` of the volume, strip by strip as
/// `sr_raid6_rw` does it, each strip's I/O run before the next. Stops at the first strip
/// that fails.
fn vol_io(m: &mut Mem, write: bool, offset: usize, data: &mut [u8]) -> Result<(), Errno> {
    let nd = m.n() - 2;
    let mut lbaoffs = offset as i64;
    let mut datalen = data.len() as i64;
    let mut done = 0usize;
    while datalen != 0 {
        let st = Raid6Strip::new(lbaoffs, datalen, STRIP, BITS, nd);
        let len = st.length as usize;
        let piece = m.add(data[done..done + len].to_vec());
        let (wu_r, flags) = if write {
            (Some(W::WuR), SCSI_DATA_OUT)
        } else {
            (None, SCSI_DATA_IN)
        };
        if let Err(e) = sr_raid6_strip(m, W::Wu, wu_r, flags, &st, nd, piece) {
            m.discard();
            m.take(piece);
            return Err(e);
        }
        m.run();
        let got = m.take(piece);
        if !write {
            data[done..done + len].copy_from_slice(&got);
        }
        lbaoffs += st.length;
        datalen -= st.length;
        done += len;
    }
    Ok(())
}

/// Reads the whole volume and compares it with `expect`.
fn assert_reads(m: &mut Mem, expect: &[u8]) {
    let mut got = vec![0u8; expect.len()];
    vol_io(m, false, 0, &mut got).unwrap();
    assert!(got == expect, "volume reads back wrong");
    assert_eq!(m.live(), 0, "leaked blocks");
}

#[test]
fn gf_tables_match_the_field() {
    // gf_init: g^i, a doubled table, zero beyond
    for i in 0..=510 {
        assert_eq!(GF.gf_pow[i], gpow(i % 255), "gf_pow[{i}]");
    }
    assert!(GF.gf_pow[511..].iter().all(|&x| x == 0));
    assert_eq!(GF.gf_log[0], 512);
    assert_eq!(GF.gf_log[1], 255);
    for i in 1..255 {
        assert_eq!(GF.gf_log[usize::from(gpow(i))], i as i32);
    }

    for a in 1..=255u8 {
        assert_eq!(gf_mul(a, gf_inv(a)), 1, "inverse of {a}");
        assert_eq!(gmul(a, gf_inv(a)), 1, "inverse of {a}");
        assert_eq!(gf_mul(a, 0), 0);
        assert_eq!(gf_mul(0, a), 0);
        for b in 1..=255u8 {
            assert_eq!(gf_mul(a, b), gmul(a, b), "{a} * {b}");
        }
    }
}

#[test]
fn premul_tables_and_xorq() {
    let _g = setup_real_memory();
    for gn in [1u8, 2, 3, 0x1d, 0x8e, 0xff] {
        gf_premul(gn).unwrap();
        gf_premul(gn).unwrap(); // once only
        let map = gf_map(gn);
        for x in 0..=255u8 {
            assert_eq!(map[usize::from(x)], gmul(gn, x), "{gn} * {x}");
        }
    }

    // q ^= gn * d, in whole 32-bit words as the C
    let d = bytes(5, 7);
    let mut q = vec![0x55u8; 7];
    sr_raid6_xorq(&mut q, &d, 7, 0x8e);
    for j in 0..4 {
        assert_eq!(q[j], 0x55 ^ gmul(0x8e, d[j]));
    }
    assert_eq!(&q[4..], &[0x55; 3]);
    let mut p = vec![0x0fu8; 6];
    sr_raid6_xorp(&mut p, &[0xff; 6], 6);
    assert_eq!(p, [0xf0, 0xf0, 0xf0, 0xf0, 0x0f, 0x0f]);
}

#[test]
fn strip_map_rotates_p_and_q() {
    for n in 4..=8i64 {
        let nd = n - 2;
        for row in 0..3 * n {
            let mut seen = vec![false; n as usize];
            let mut last = -1;
            for k in 0..nd {
                let strip_no = row * nd + k;
                let st = Raid6Strip::new(strip_no << BITS, STRIP, STRIP, BITS, nd);
                assert_eq!(st.strip_no, strip_no);
                assert_eq!(st.lba, row * (STRIP / DEV_BSIZE as i64));
                // Q starts on the last chunk and moves left one chunk per row, P just left
                // of it (wrapping to the last chunk)
                assert_eq!(st.qchunk, (n - 1) - row % n);
                assert_eq!(st.pchunk, (st.qchunk + n - 1) % n);
                // data fills the other chunks left to right
                assert!(st.chunk > last && st.chunk < n);
                assert!(st.chunk != st.pchunk && st.chunk != st.qchunk);
                last = st.chunk;
                seen[st.chunk as usize] = true;
                if k == 0 {
                    seen[st.pchunk as usize] = true;
                    seen[st.qchunk as usize] = true;
                }
            }
            assert!(seen.iter().all(|&s| s), "n {n} row {row}");
        }
    }
    // part of a strip
    let st = Raid6Strip::new((3 << BITS) + 512, 10_000, STRIP, BITS, 2);
    assert_eq!(
        (st.length, st.lba),
        (STRIP - 512, (STRIP + 512) / DEV_BSIZE as i64)
    );
}

#[test]
fn sizes_states_and_init() {
    let strip = MAXPHYS as u32;
    let blocks = i64::from(strip) / DEV_BSIZE as i64;
    assert_eq!(raid6_volume_size(10 * blocks + 7, strip, 4), 20 * blocks);
    assert_eq!(raid6_volume_size(10 * blocks, strip, 6), 40 * blocks);

    let st = |online: i64, rebuild: i64, offline: i64| {
        let mut s = [0i64; SR_MAX_STATES];
        s[BIOC_SDONLINE as usize] = online;
        s[BIOC_SDREBUILD as usize] = rebuild;
        s[BIOC_SDOFFLINE as usize] = offline;
        raid6_vol_state(&s, online + rebuild + offline)
    };
    assert_eq!(st(5, 0, 0), Some(BIOC_SVONLINE));
    assert_eq!(st(4, 0, 1), Some(BIOC_SVDEGRADED));
    assert_eq!(st(3, 0, 2), Some(BIOC_SVDEGRADED));
    assert_eq!(st(3, 1, 1), Some(BIOC_SVREBUILD));
    assert_eq!(st(2, 0, 3), Some(BIOC_SVOFFLINE));
    assert!(raid6_chunk_state_ok(BIOC_SDONLINE, BIOC_SDOFFLINE));
    assert!(raid6_chunk_state_ok(BIOC_SDOFFLINE, BIOC_SDREBUILD));
    assert!(!raid6_chunk_state_ok(BIOC_SDOFFLINE, BIOC_SDONLINE));
    assert!(!raid6_chunk_state_ok(BIOC_SDHOTSPARE, BIOC_SDONLINE));
    assert!(raid6_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVDEGRADED));
    assert!(!raid6_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVONLINE));
    assert!(!raid6_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVOFFLINE));

    let _g = setup_real_memory();
    // SAFETY: zeroed (`SrZeroed`) and leaked.
    let sd: &'static SrDiscipline =
        unsafe { sr_malloc::<SrDiscipline>(M_WAITOK).unwrap().as_ref() };
    // SAFETY: a zeroed softc (a `Softc`: all-zero bytes are a valid value), leaked.
    let sc: &'static SrSoftc = std::boxed::Box::leak(std::boxed::Box::new(unsafe {
        core::mem::zeroed::<SrSoftc>()
    }));
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(crate::dev::softraid::SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    let n = 5;
    sd.sd_meta().ssdi().ssd_chunk_no.set(n);
    sd.sd_vol.sv_chunks_alloc(n as usize, M_WAITOK).unwrap();
    for i in 0..n as usize {
        // SAFETY: zeroed (`SrZeroed`) and leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_meta.scm_status.set(BIOC_SDONLINE as u32);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }

    sr_raid6_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_RAID6);
    assert_eq!(&sd.sd_name.get()[..6], b"RAID 6");
    assert_eq!(sr_raid6_openings(sd), SR_RAID6_NOWU as i32 / 2);
    assert!(sd.sd_capabilities.get() & SR_CAP_REBUILD == 0);
    sd.sd_meta().ssdi().ssd_strip_size.set(strip);
    sr_raid6_init(sd).unwrap();
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 10);
    sd.sd_meta().ssdi().ssd_strip_size.set(1000);
    assert_eq!(sr_raid6_init(sd), Err(Errno::EINVAL));

    sd.sd_vol_status.set(BIOC_SVONLINE);
    for (c, want) in [
        (1, BIOC_SVDEGRADED),
        (3, BIOC_SVDEGRADED),
        (4, BIOC_SVOFFLINE),
    ] {
        sd.sd_vol
            .sv_chunk(c)
            .src_meta
            .scm_status
            .set(BIOC_SDOFFLINE as u32);
        sr_raid6_set_vol_state(sd);
        assert_eq!(sd.sd_vol_status.get(), want);
    }
}

#[test]
fn writes_compute_p_and_q() {
    let _g = setup_real_memory();
    for n in 4..=7 {
        let mut m = Mem::new(n, 2 * n);
        let mut expect = bytes(n as u32, m.size());
        vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
        m.assert_pq();
        // read-modify-write of pieces of strips (whole sectors) keeps both parities
        for (k, (off, len)) in [(512, 9216), (4096 * 5 - 512, 1024), (1536, 512)]
            .iter()
            .enumerate()
        {
            let mut piece = bytes(1000 + k as u32, *len);
            vol_io(&mut m, true, *off, &mut piece).unwrap();
            expect[*off..*off + *len].copy_from_slice(&piece);
        }
        assert_eq!(m.live(), 0, "leaked blocks");
        m.assert_pq();
        assert_reads(&mut m, &expect);
    }
}

#[test]
fn one_or_two_missing_chunks_are_recovered() {
    // Over the rows, two missing chunks are every pair of roles: two data strips (Dx+Dy, from
    // P and Q), data and P (from Q), data and Q (from P), P and Q (plain reads).
    let _g = setup_real_memory();
    for n in 4..=6 {
        let mut m = Mem::new(n, 2 * n);
        let expect = bytes(17 * n as u32, m.size());
        vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
        for a in 0..n {
            m.status[a] = BIOC_SDOFFLINE;
            assert_reads(&mut m, &expect);
            for b in a + 1..n {
                m.status[b] = BIOC_SDREBUILD;
                assert_reads(&mut m, &expect);
                m.status[b] = BIOC_SDONLINE;
            }
            m.status[a] = BIOC_SDONLINE;
        }
    }
}

#[test]
fn degraded_writes() {
    // With chunk k offline, a strip write fails when k holds the strip, P or Q, and else
    // updates P and Q from the old data, so the volume stays consistent and readable.
    let _g = setup_real_memory();
    for n in 4..=6 {
        for k in 0..n {
            let mut m = Mem::new(n, 2 * n);
            let mut expect = bytes(5 * n as u32 + k as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            m.status[k] = BIOC_SDOFFLINE;
            let nd = n as i64 - 2;
            let new = bytes(200 + k as u32, m.size());
            for s in 0..m.size() / STRIP as usize {
                let st = Raid6Strip::new((s as i64) << BITS, STRIP, STRIP, BITS, nd);
                let hit = [st.chunk, st.pchunk, st.qchunk].contains(&(k as i64));
                let range = s * STRIP as usize..(s + 1) * STRIP as usize;
                let mut piece = new[range.clone()].to_vec();
                let r = vol_io(&mut m, true, range.start, &mut piece);
                assert_eq!(r.is_err(), hit, "n {n} k {k} strip {s}");
                if !hit {
                    expect[range].copy_from_slice(&piece);
                }
            }
            assert_eq!(m.live(), 0, "leaked blocks");
            assert_reads(&mut m, &expect);
            m.status[k] = BIOC_SDONLINE;
            m.assert_pq();
            assert_reads(&mut m, &expect);
        }
    }
}

#[test]
fn failed_queueing_frees_p_and_q() {
    // Fail each of the six addio calls of a strip write in turn: the write fails and no
    // block is left behind.
    let _g = setup_real_memory();
    for k in 0..8 {
        let mut m = Mem::new(5, 3);
        m.fail_in = Some(k);
        let mut piece = bytes(k as u32, STRIP as usize);
        let r = vol_io(&mut m, true, STRIP as usize, &mut piece);
        assert_eq!(r.is_ok(), k >= 6, "addio {k} failed");
        assert_eq!(m.live(), 0, "leaked blocks (addio {k} failed)");
    }
}
