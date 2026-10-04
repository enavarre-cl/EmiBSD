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

/// Which work unit of a strip an I/O is queued on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum W {
    /// `wu`: the transfer's own.
    Wu,
    /// `wu_r`: the reads of a write, which run first.
    WuR,
}

/// One queued `sr_raid5_addio`.
#[derive(Clone, Copy, Debug)]
struct Op {
    wu: W,
    chunk: i64,
    blkno: Daddr,
    len: i64,
    data: usize,
    read: bool,
    freebuf: bool,
    xorbuf: Option<usize>,
}

/// A RAID 5 volume in memory: the chunks' bytes and states, an arena of buffers (a
/// [`Raid5Io::Buf`] is an index into it; `None` once freed) and the queued I/O.
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

    fn rows(&self) -> usize {
        self.chunks[0].len() / STRIP as usize
    }

    /// The volume's data bytes.
    fn size(&self) -> usize {
        self.rows() * STRIP as usize * (self.chunks.len() - 1)
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
            if let Some(x) = op.xorbuf {
                sr_raid5_xor(self.buf(x), &got, len);
            }
        } else {
            assert!(
                matches!(
                    self.status[c],
                    BIOC_SDONLINE | BIOC_SDSCRUB | BIOC_SDREBUILD
                ),
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
    fn strip(&self, c: usize, row: usize) -> &[u8] {
        &self.chunks[c][row * STRIP as usize..(row + 1) * STRIP as usize]
    }

    /// Every row's strips XOR to zero (the parity is the XOR of the data).
    fn assert_parity(&self) {
        for row in 0..self.rows() {
            let mut x = vec![0u8; STRIP as usize];
            for c in 0..self.chunks.len() {
                sr_raid5_xor(&mut x, self.strip(c, row), STRIP as usize);
            }
            assert!(x.iter().all(|&b| b == 0), "row {row} parity is wrong");
        }
    }
}

impl Raid5Io for Mem {
    type Wu = W;
    type Buf = usize;

    fn chunk_no(&self) -> i64 {
        self.chunks.len() as i64
    }

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
        xorbuf: Option<usize>,
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
        assert!(xorbuf.is_none() || data != xorbuf.unwrap_or(usize::MAX));
        self.ops.push(Op {
            wu,
            chunk,
            blkno,
            len,
            data,
            read: xsflags & SCSI_DATA_IN != 0,
            freebuf: ccbflags & SR_CCBF_FREEBUF != 0,
            xorbuf,
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

    fn copy(&mut self, dst: usize, src: usize, len: i64) {
        let s = self.buf(src)[..len as usize].to_vec();
        self.buf(dst)[..len as usize].copy_from_slice(&s);
    }
}

/// A read or write of `data.len()` bytes at byte `offset` of the volume, strip by strip as
/// `sr_raid5_rw` does it, each strip's I/O run before the next.
fn vol_io(m: &mut Mem, write: bool, offset: usize, data: &mut [u8]) -> Result<(), Errno> {
    let no_chunk = m.chunk_no() - 1;
    let mut lbaoffs = offset as i64;
    let mut datalen = data.len() as i64;
    let mut done = 0usize;
    while datalen != 0 {
        let st = Raid5Strip::new(lbaoffs, datalen, STRIP, BITS, no_chunk);
        let len = st.length as usize;
        let piece = m.add(data[done..done + len].to_vec());
        let r = if write {
            sr_raid5_write(
                m,
                W::Wu,
                W::WuR,
                st.chunk,
                st.parity,
                st.lba,
                st.length,
                piece,
                SCSI_DATA_OUT,
                0,
            )
        } else {
            match m.chunk_status(st.chunk) {
                BIOC_SDONLINE | BIOC_SDSCRUB => m.addio(
                    W::Wu,
                    st.chunk,
                    st.lba,
                    st.length,
                    Some(piece),
                    SCSI_DATA_IN,
                    0,
                    None,
                ),
                BIOC_SDOFFLINE | BIOC_SDREBUILD | BIOC_SDHOTSPARE => {
                    sr_raid5_regenerate(m, W::Wu, st.chunk, st.lba, st.length, piece)
                }
                _ => Err(Errno::EIO),
            }
        };
        if let Err(e) = r {
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
fn strip_map_is_left_asymmetric() {
    for n in 3..=8i64 {
        let no_chunk = n - 1;
        for row in 0..3 * n {
            let mut seen = vec![false; n as usize];
            let mut last = -1;
            let mut parity = None;
            for k in 0..no_chunk {
                let strip_no = row * no_chunk + k;
                let st = Raid5Strip::new(strip_no << BITS, STRIP, STRIP, BITS, no_chunk);
                assert_eq!(st.strip_no, strip_no);
                assert_eq!(st.length, STRIP);
                assert_eq!(st.lba, row * (STRIP / DEV_BSIZE as i64));
                // parity starts on the last chunk and moves left one chunk per row
                assert_eq!(st.parity, (n - 1) - row % n);
                assert_eq!(*parity.get_or_insert(st.parity), st.parity);
                // data fills the other chunks left to right
                assert!(st.chunk > last && st.chunk != st.parity && st.chunk < n);
                last = st.chunk;
                seen[st.chunk as usize] = true;
            }
            seen[parity.unwrap() as usize] = true;
            assert!(seen.iter().all(|&s| s), "n {n} row {row}");
        }
    }
}

#[test]
fn strip_map_partial_strips() {
    // 3 data chunks; a transfer starting 1024 bytes into strip 4: row 1, whose parity is on
    // chunk 3 - 1 = 2; strip 4 % 3 = 1 is left of it
    let st = Raid5Strip::new((4 << BITS) + 1024, 10_000, STRIP, BITS, 3);
    assert_eq!(st.strip_no, 4);
    assert_eq!(st.length, STRIP - 1024);
    assert_eq!(st.parity, 2);
    assert_eq!(st.chunk, 1);
    assert_eq!(st.lba, (STRIP + 1024) / DEV_BSIZE as i64);
    // strip 5 % 3 = 2 is the parity's chunk: it moves right of it
    let st = Raid5Strip::new(5 << BITS, 100, STRIP, BITS, 3);
    assert_eq!((st.chunk, st.parity, st.length), (3, 2, 100));
}

#[test]
fn volume_size_and_openings() {
    let strip = MAXPHYS as u32;
    let blocks = i64::from(strip) / DEV_BSIZE as i64;
    assert_eq!(raid5_volume_size(10 * blocks + 7, strip, 3), 20 * blocks);
    assert_eq!(raid5_volume_size(10 * blocks, strip, 5), 40 * blocks);

    let _g = setup_real_memory();
    let sd = discipline(4, &[BIOC_SDONLINE; 4]);
    sr_raid5_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_RAID5);
    assert_eq!(&sd.sd_name.get()[..6], b"RAID 5");
    assert_eq!(sd.sd_max_wu.get(), SR_RAID5_NOWU + 2);
    assert_eq!(sr_raid5_openings(sd), SR_RAID5_NOWU as i32 / 2);
    assert!(sd.sd_capabilities.get() & SR_CAP_REBUILD != 0);
    sd.sd_meta().ssdi().ssd_strip_size.set(strip);
    sr_raid5_init(sd).unwrap();
    assert_eq!(
        sd.mds().mdd_raid5.sr5_strip_bits.get(),
        strip.trailing_zeros() as i32
    );
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 4);
}

#[test]
fn xor_works_in_words() {
    let mut a = vec![0x0fu8; 7];
    sr_raid5_xor(&mut a, &[0xffu8; 7], 7);
    // 7 & !3 = 4 bytes: the C's one 32-bit word
    assert_eq!(a, [0xf0, 0xf0, 0xf0, 0xf0, 0x0f, 0x0f, 0x0f]);
}

#[test]
fn chunk_state_table() {
    let ok = [
        (BIOC_SDONLINE, BIOC_SDOFFLINE),
        (BIOC_SDONLINE, BIOC_SDSCRUB),
        (BIOC_SDOFFLINE, BIOC_SDREBUILD),
        (BIOC_SDSCRUB, BIOC_SDONLINE),
        (BIOC_SDSCRUB, BIOC_SDOFFLINE),
        (BIOC_SDREBUILD, BIOC_SDONLINE),
        (BIOC_SDREBUILD, BIOC_SDOFFLINE),
    ];
    for old in 0..SR_MAX_STATES as i32 {
        for new in 0..SR_MAX_STATES as i32 {
            if old != new {
                assert_eq!(
                    raid5_chunk_state_ok(old, new),
                    ok.contains(&(old, new)),
                    "{old} -> {new}"
                );
            }
        }
    }
}

#[test]
fn volume_state_from_chunks() {
    let st = |online: i64, scrub: i64, rebuild: i64, offline: i64| {
        let mut s = [0i64; SR_MAX_STATES];
        s[BIOC_SDONLINE as usize] = online;
        s[BIOC_SDSCRUB as usize] = scrub;
        s[BIOC_SDREBUILD as usize] = rebuild;
        s[BIOC_SDOFFLINE as usize] = offline;
        raid5_vol_state(&s, online + scrub + rebuild + offline)
    };
    assert_eq!(st(4, 0, 0, 0), Some(BIOC_SVONLINE));
    assert_eq!(st(3, 0, 0, 1), Some(BIOC_SVDEGRADED));
    assert_eq!(st(3, 0, 1, 0), Some(BIOC_SVREBUILD));
    assert_eq!(st(3, 1, 0, 0), Some(BIOC_SVSCRUB));
    assert_eq!(st(2, 0, 0, 2), Some(BIOC_SVOFFLINE));
    assert_eq!(st(2, 0, 1, 1), Some(BIOC_SVOFFLINE));

    assert!(raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVDEGRADED));
    assert!(raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVREBUILD));
    assert!(!raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVSCRUB));
    assert!(raid5_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVREBUILD));
    assert!(!raid5_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVONLINE));
    assert!(raid5_vol_state_ok(BIOC_SVREBUILD, BIOC_SVONLINE));
    assert!(raid5_vol_state_ok(BIOC_SVBUILDING, BIOC_SVBUILDING));
    assert!(!raid5_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVOFFLINE));
    assert!(!raid5_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVONLINE));
}

/// A discipline over `n` chunks in the states `status`, as the softraid tests make one.
fn discipline(n: usize, status: &[i32]) -> &'static SrDiscipline {
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
    sd.sd_meta().ssdi().ssd_chunk_no.set(n as u32);
    sd.sd_vol.sv_chunks_alloc(n, M_WAITOK).unwrap();
    for (i, &s) in status.iter().enumerate() {
        // SAFETY: zeroed (`SrZeroed`) and leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_meta.scm_status.set(s as u32);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }
    sd
}

#[test]
fn set_vol_state_follows_chunks() {
    let _g = setup_real_memory();
    let sd = discipline(4, &[BIOC_SDONLINE; 4]);
    sd.sd_vol_status.set(BIOC_SVONLINE);
    sr_raid5_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVONLINE);
    sd.sd_vol
        .sv_chunk(2)
        .src_meta
        .scm_status
        .set(BIOC_SDOFFLINE as u32);
    sr_raid5_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVDEGRADED);
    sd.sd_vol
        .sv_chunk(2)
        .src_meta
        .scm_status
        .set(BIOC_SDREBUILD as u32);
    sr_raid5_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVREBUILD);
    sd.sd_vol
        .sv_chunk(0)
        .src_meta
        .scm_status
        .set(BIOC_SDOFFLINE as u32);
    sr_raid5_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVOFFLINE);
}

#[test]
fn writes_keep_parity_and_read_back() {
    for n in 3..=6 {
        let mut m = Mem::new(n, 2 * n);
        let mut expect = vec![0u8; m.size()];
        // the whole volume, then pieces (whole sectors, as transfers are) that cross strips
        let mut all = bytes(n as u32, m.size());
        vol_io(&mut m, true, 0, &mut all).unwrap();
        expect.copy_from_slice(&all);
        for (k, (off, len)) in [(512, 9216), (4096 * 3 - 512, 1024), (1536, 512)]
            .iter()
            .enumerate()
        {
            let mut piece = bytes(1000 + k as u32, *len);
            vol_io(&mut m, true, *off, &mut piece).unwrap();
            expect[*off..*off + *len].copy_from_slice(&piece);
        }
        assert_eq!(m.live(), 0, "leaked blocks");
        m.assert_parity();
        assert_reads(&mut m, &expect);
    }
}

#[test]
fn any_one_missing_chunk_is_regenerated() {
    for n in 3..=6 {
        let mut m = Mem::new(n, 2 * n);
        let expect = bytes(7 * n as u32, m.size());
        vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
        for c in 0..n {
            for state in [BIOC_SDOFFLINE, BIOC_SDREBUILD, BIOC_SDHOTSPARE] {
                m.status[c] = state;
                assert_reads(&mut m, &expect);
            }
            m.status[c] = BIOC_SDONLINE;
        }
        // two missing chunks: a strip on either cannot be regenerated
        m.status[0] = BIOC_SDOFFLINE;
        m.status[1] = BIOC_SDOFFLINE;
        let mut got = vec![0u8; expect.len()];
        assert_eq!(vol_io(&mut m, false, 0, &mut got), Err(Errno::EIO));
        assert_eq!(m.live(), 0, "leaked blocks");
    }
}

#[test]
fn degraded_writes_cover_the_four_cases() {
    // With one chunk offline, each row is case 2 (it holds the row's parity), case 3 (it
    // holds the strip) or case 4 (another strip of the row).
    for n in 3..=6 {
        for off in 0..n {
            let mut m = Mem::new(n, 2 * n);
            let mut expect = bytes(3 * n as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            m.status[off] = BIOC_SDOFFLINE;
            let mut new = bytes(100 + off as u32, m.size());
            vol_io(&mut m, true, 0, &mut new).unwrap();
            expect.copy_from_slice(&new);
            assert_eq!(m.live(), 0, "leaked blocks");
            // the missing chunk's strips come back from the new parity
            assert_reads(&mut m, &expect);
        }
    }
}

#[test]
fn writes_onto_a_rebuilding_chunk() {
    // The rebuilding chunk is written as normal; the other strips of its rows update the
    // parity from the old data and parity (case 4), so after a full write the chunk holds
    // its data and every row's parity is right.
    for n in 3..=5 {
        for rc in 0..n {
            let mut m = Mem::new(n, 2 * n);
            m.status[rc] = BIOC_SDREBUILD;
            let expect = bytes(11 * n as u32 + rc as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            m.status[rc] = BIOC_SDONLINE;
            m.assert_parity();
            assert_reads(&mut m, &expect);
        }
    }
}

#[test]
fn rebuild_strips_regenerate_the_chunk() {
    // sr_raid5_rebuild's loop body: regenerate each strip of the chunk from the others into a
    // block, write the block to the chunk.
    let n = 5;
    let mut m = Mem::new(n, 8);
    let mut data = bytes(42, m.size());
    vol_io(&mut m, true, 0, &mut data).unwrap();
    for rc in 0..n {
        let orig = m.chunks[rc].clone();
        m.chunks[rc].fill(0xa5);
        m.status[rc] = BIOC_SDREBUILD;
        for strip_no in 0..m.rows() as i64 {
            let chunk_lba = (STRIP >> DEV_BSHIFT) * strip_no;
            let xorbuf = m.block_get(STRIP).unwrap();
            sr_raid5_regenerate(&mut m, W::WuR, rc as i64, chunk_lba, STRIP, xorbuf).unwrap();
            m.addio(
                W::Wu,
                rc as i64,
                chunk_lba,
                STRIP,
                Some(xorbuf),
                SCSI_DATA_OUT,
                SR_CCBF_FREEBUF,
                None,
            )
            .unwrap();
            m.run();
        }
        m.status[rc] = BIOC_SDONLINE;
        assert!(m.chunks[rc] == orig, "chunk {rc} not rebuilt");
        assert_eq!(m.live(), 0, "leaked blocks");
    }
}

#[test]
fn failed_queueing_frees_the_parity_block() {
    // Fail each addio of a write of strip 0 (chunk 0, parity on 3) in turn, all online
    // (case 1) and with chunk 1 offline (case 4): the write fails and no block is left
    // behind; four calls each (two reads, the parity, the data).
    for off in [None, Some(1)] {
        for k in 0..6 {
            let mut m = Mem::new(4, 4);
            if let Some(o) = off {
                m.status[o] = BIOC_SDOFFLINE;
            }
            m.fail_in = Some(k);
            let mut piece = bytes(k as u32, STRIP as usize);
            let r = vol_io(&mut m, true, 0, &mut piece);
            assert_eq!(r.is_ok(), k >= 4, "addio {k} failed");
            assert_eq!(m.live(), 0, "leaked blocks (addio {k} failed)");
        }
    }
}
