/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 2011,2020 Theo de Raadt.
 * Copyright (c) 2008 Damien Miller.
 * Copyright (c) 1996, 1997, 2000-2002 Michael Shalayeff.
 * Copyright (c) 2013 Markus Friedl.
 * Copyright Theodore Ts'o, 1994, 1995, 1996, 1997, 1998, 1999.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, and the entire permission notice in its entirety,
 *    including the disclaimer of warranties.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior
 *    written permission.
 *
 * ALTERNATIVELY, this product may be distributed under the terms of
 * the GNU Public License, in which case the provisions of the GPL are
 * required INSTEAD OF the above restrictions.  (This clause is
 * necessary due to a potential bad interaction between the GPL and
 * the restrictions contained in a BSD-style copyright.)
 *
 * THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESS OR IMPLIED
 * WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 * OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The kernel's random number generator and `/dev/random`: `dev/rnd.c`.
//!
//! Upstream: sys/dev/rnd.c @ 3ce1f3f79392
//!
//! The boot loader pre-fills the kernel's `.openbsd.randomdata` section ([`ENTROPY_POOL0`],
//! [`RS_BUF0`], and `init_main.c`'s `__guard_local`) with seed material: boot(8) mixes the
//! previous boot's `/etc/random.seed` with the machine's and the firmware's random sources.
//! The first `arc4random` call keys the ChaCha20 generator from that seed, so the kernel may
//! use it from early bootstrap on.
//!
//! Device drivers and the clock submit samples with [`enqueue_randomness`]: each lands in
//! one slot of the entropy input ring ([`RND_EVENT_SPACE`]) together with
//! `cpu_rnd_messybits()`, the timing noise of the moment. A timeout ([`RND_TIMEOUT`]) pulls
//! a mix of the newest and the oldest slots into [`ENTROPY_POOL`] through a twisted GFSR
//! ([`add_entropy_words`]). From time to time ([`rnd_reinit`], every ten minutes, and when
//! the generator has handed out 1.6 MB) the pool is whitened by SHA-512
//! ([`extract_entropy`]), mixed with the time, and XORed into the ChaCha20 state: the new
//! key and IV. While the kernel is `cold` every sample is dequeued and stirred in at once.
//! [`random_start`] (from `main`) wipes the boot seed, mixes in the message buffer and
//! a slice of the kernel text, stirs, and starts the periodic reseed.
//!
//! `/dev/random` (`cdevsw` 45, `cdev_random_init`): reads are served from the generator
//! (from a private ChaCha20 instance keyed from it past [`RND_MAIN_MAX_BYTES`]); writes are
//! mixed into the pool and reseed the generator. `getentropy(2)` is [`sys_getentropy`].
//!
//! ## Deviations
//! - The pool, the input ring, `extract_entropy`'s copy of the pool, `rnd_cold`,
//!   `rnd_slowextract` and `add_entropy_words`'s two statics are shared without a lock, as
//!   in C, which tolerates the races by design ("INTENTIONALLY not protected by any lock").
//!   Rust has no benign data race, so their words are atomics accessed `Relaxed` (plain
//!   loads and stores, the C's code); the C's two `atomic_*_int_nv` counters are `fetch_add`.
//!   `add_entropy_words`'s `entropy_add_ptr` and `entropy_input_rotate` live beside the pool
//!   words in one [`EntropyPool`], so the host tests can run the mixing on a pool of their
//!   own; the kernel has the one [`ENTROPY_POOL`].
//! - `extract_entropy` hashes its copy of the pool one SHA-512 block at a time (the words in
//!   memory order, as the C's single `SHA512Update` over the array reads them): the same
//!   digest.
//! - The ChaCha20 state (`rs`, `rs_buf`, `rs_have`, `rs_count` and `_rs_stir_if_needed`'s
//!   `rs_initialized`) is one [`RsState`] in a `StaticCell` under `rndlock`. The C reaches
//!   `rs_count` and `_rs_seed` without the lock in `suspend_randomness`/`resume_randomness`
//!   (one CPU runs at suspend and resume); here they take `rndlock`, which changes nothing
//!   there and keeps the access sound. `_rs_stir(do_lock)` takes the state itself when the
//!   caller holds the lock (`Some`) and the lock when it does not (`None`).
//! - The host tests run on several threads with the uniprocessor `mtx_enter`, which
//!   excludes nothing between them; under `cfg(test)` `rndlock` also takes a host spin flag.
//! - The boot seed ([`ENTROPY_POOL0`], [`RS_BUF0`]) is an immutable static the loader writes
//!   before the kernel runs and `_rs_clearseed` wipes through another mapping: it is read
//!   volatile only, so the compiler never assumes its initial zeroes. On the host (no
//!   `.openbsd.randomdata` section) it stays zero.
//! - `_rs_clearseed` panics ("_rs_clearseed") when the seed's page has no mapping; the C
//!   ignores `pmap_extract`'s failure, which cannot happen for the kernel image.
//! - `arc4random_ctx_new` returns `None` and `randomread`/`randomwrite` fail with `ENOMEM`
//!   if `malloc(M_WAITOK)` returns nothing, which the C's `M_WAITOK` never does (as
//!   `exec_subr.rs` does with its own `malloc`). The context is an owning
//!   [`Arc4randomCtx`] handle that `arc4random_ctx_free` consumes.
//! - `suspend_randomness` fills the pool through the generator in 64-byte pieces under one
//!   `rndlock` hold and one `_rs_stir_if_needed`: the C's one `arc4random_buf` of the pool,
//!   byte for byte. `subr_suspend.c` is not ported, so nothing calls it or
//!   `resume_randomness` yet.
//! - `__guard_local` is `init_main.rs`'s `GUARD_LOCAL` (`!NO_PROPOLICE`: the variable
//!   exists; Rust code has no stack protector reading it). Booted by Limine, which fills no
//!   `PT_OPENBSD_RANDOMIZE` segment, it stays 0 and `random_start` prints the C's "warning: no
//!   entropy supplied by boot loader"; the boot glue reports that gap (`sys/stand/mod.rs`).

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use libkern::{StaticCell, explicit_bzero};

use crate::crypto::chacha_private::ChachaCtx;
use crate::crypto::sha2::{SHA512_BLOCK_LENGTH, SHA512_DIGEST_LENGTH, Sha512Ctx};
use crate::kassert;
use crate::kern::init_main::GUARD_LOCAL;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_task::{SYSTQ, task_add};
use crate::kern::kern_tc::{getnanotime, nanotime};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec};
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_log::msgbufp;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::copy::copyout;
use crate::machine::cpu::{cpu_rnd_messybits, etext};
use crate::machine::intr::IPL_HIGH;
use crate::machine::pmap::{pmap_extract, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_update};
use crate::sys::errno::Errno;
use crate::sys::event::{EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, Filterops, Knote};
use crate::sys::filio::FIOASYNC;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::msgbuf::MSG_MAGIC;
use crate::sys::mutex::Mutex;
use crate::sys::param::PAGE_SIZE;
use crate::sys::proc::Proc;
use crate::sys::syscallargs::SysGetentropyArgs;
use crate::sys::syslimits::GETENTROPY_MAX;
use crate::sys::systm::{COLD, SysArgs, sysargs};
use crate::sys::task::Task;
use crate::sys::time::Timespec;
use crate::sys::timeout::{Timeout, timeout_pending};
use crate::sys::types::{Dev, Register, Vaddr, Vsize};
use crate::sys::uio::Uio;
use crate::uvm::uvm_extern::{KmemDynMode, Voff};
use crate::uvm::uvm_km::{KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_param::trunc_page;

// Stirring polynomial over GF(2), used in add_entropy_words(). The polynomial terms are
// chosen to be evenly spaced (minimum RMS distance from evenly spaced), except for the last
// tap, which is 1 to get the twisting happening as fast as possible. The resultant polynomial
// is 2^POOLWORDS + 2^POOL_TAP1 + 2^POOL_TAP2 + 2^POOL_TAP3 + 2^POOL_TAP4 + 1.

/// `POOLWORDS`: the entropy pool's size in 32-bit words.
pub const POOLWORDS: usize = 2048;
/// `POOLBYTES`: the entropy pool's size in bytes.
pub const POOLBYTES: usize = POOLWORDS * 4;
/// `POOLMASK`.
const POOLMASK: u32 = (POOLWORDS - 1) as u32;
/// `POOL_TAP1`.
const POOL_TAP1: u32 = 1638;
/// `POOL_TAP2`.
const POOL_TAP2: u32 = 1231;
/// `POOL_TAP3`.
const POOL_TAP3: u32 = 819;
/// `POOL_TAP4`.
const POOL_TAP4: u32 = 411;

/// `QEVLEN`: the entropy input ring's slots (a power of 2).
pub const QEVLEN: u32 = 128;
/// `QEVCONSUME`: how many events to consume at a time.
pub const QEVCONSUME: u32 = 8;

/// `KEYSZ`: the ChaCha20 key's bytes.
const KEYSZ: usize = 32;
/// `IVSZ`: the ChaCha20 IV's bytes.
const IVSZ: usize = 8;
/// `BLOCKSZ`: a ChaCha20 block.
const BLOCKSZ: usize = 64;
/// `RSBUFSZ`: the keystream buffer.
const RSBUFSZ: usize = 16 * BLOCKSZ;
/// `EBUFSIZE`: what `extract_entropy` returns, a key and an IV.
const EBUFSIZE: usize = KEYSZ + IVSZ;

/// `twist_table` (`add_entropy_words`): derived from IEEE 802.3 CRC-32.
const TWIST_TABLE: [u32; 8] = [
    0x0000_0000,
    0x3b6e_20c8,
    0x76dc_4190,
    0x4db2_6158,
    0xedb8_8320,
    0xd6d6_a3e8,
    0x9b64_c2b0,
    0xa00a_e278,
];

/// `RND_MAIN_MAX_BYTES`: the most bytes a read takes from the main ChaCha20 generator;
/// larger reads are served by a ChaCha20 instance of their own, keyed from it.
pub const RND_MAIN_MAX_BYTES: usize = 2048;

// SHA-512's output covers what extract_entropy returns ("need more bigger hash output").
const _: () = assert!(SHA512_DIGEST_LENGTH >= EBUFSIZE);

/// `struct rand_event`: one slot of the entropy input ring.
pub struct RandEvent {
    /// `re_time`: the sum of the `cpu_rnd_messybits()` of the samples that landed here.
    pub re_time: AtomicU32,
    /// `re_val`: the sum of the samples that landed here.
    pub re_val: AtomicU32,
}

impl RandEvent {
    /// An empty slot.
    const fn new() -> Self {
        Self {
            re_time: AtomicU32::new(0),
            re_val: AtomicU32::new(0),
        }
    }
}

/// `entropy_pool[]` with `add_entropy_words`'s `entropy_add_ptr` and `entropy_input_rotate`
/// (see the module's deviations).
pub struct EntropyPool {
    /// `entropy_pool[]`.
    words: [AtomicU32; POOLWORDS],
    /// `entropy_add_ptr`: where the next word goes (counting down).
    add_ptr: AtomicU32,
    /// `entropy_input_rotate`: the rotation of the next input word.
    input_rotate: AtomicU8,
}

impl EntropyPool {
    /// An all-zero pool.
    const fn new() -> Self {
        Self {
            words: [const { AtomicU32::new(0) }; POOLWORDS],
            add_ptr: AtomicU32::new(0),
            input_rotate: AtomicU8::new(0),
        }
    }

    /// `add_entropy_words(buf, n)` on this pool: merges `buf` into the pool, using the
    /// polynomial to spread the bits.
    fn add_entropy_words(&self, buf: impl IntoIterator<Item = u32>) {
        for word in buf {
            let rotate = u32::from(self.input_rotate.load(Ordering::Relaxed));
            let mut w = word.rotate_left(rotate);
            let i = self.add_ptr.load(Ordering::Relaxed).wrapping_sub(1) & POOLMASK;
            self.add_ptr.store(i, Ordering::Relaxed);
            // Normally, we add 7 bits of rotation to the pool. At the beginning of the pool,
            // add an extra 7 bits rotation, so that successive passes spread the input bits
            // across the pool evenly.
            let next = (rotate + if i != 0 { 7 } else { 14 }) & 31;
            self.input_rotate.store(next as u8, Ordering::Relaxed);

            // XOR pool contents corresponding to polynomial terms.
            let at = |off: u32| self.words[((i + off) & POOLMASK) as usize].load(Ordering::Relaxed);
            w ^= at(POOL_TAP1) ^ at(POOL_TAP2) ^ at(POOL_TAP3) ^ at(POOL_TAP4) ^ at(1) ^ at(0);

            self.words[i as usize]
                .store((w >> 3) ^ TWIST_TABLE[(w & 7) as usize], Ordering::Relaxed);
        }
    }

    /// Word `i` of the pool (for the tests).
    #[cfg(test)]
    fn word(&self, i: usize) -> u32 {
        self.words[i].load(Ordering::Relaxed)
    }
}

/// The ChaCha20 generator's state, under `rndlock` (see the module's deviations).
struct RsState {
    /// `rs`: the ChaCha20 context of the random keystream.
    rs: ChachaCtx,
    /// `rs_buf`: keystream blocks (the seed from boot at first).
    rs_buf: [u8; RSBUFSZ],
    /// `rs_have`: valid bytes at the end of `rs_buf`.
    rs_have: usize,
    /// `rs_count`: bytes until the next reseed.
    rs_count: usize,
    /// `rs_initialized` (`_rs_stir_if_needed`): the boot seed has been taken.
    rs_initialized: bool,
}

impl RsState {
    /// The state before the first `arc4random`.
    const fn new() -> Self {
        Self {
            rs: ChachaCtx { input: [0; 16] },
            rs_buf: [0; RSBUFSZ],
            rs_have: 0,
            rs_count: 0,
            rs_initialized: false,
        }
    }

    /// `_rs_seed(buf, n)`: rekeys with `buf` mixed in and drops the buffered keystream.
    fn _rs_seed(&mut self, buf: &[u8]) {
        self._rs_rekey(Some(buf));

        // invalidate rs_buf
        self.rs_have = 0;
        self.rs_buf.fill(0);

        self.rs_count = 1_600_000;
    }

    /// `_rs_rekey(dat, datlen)`: refills `rs_buf` with keystream, XORs in the optional
    /// `dat`, and takes the buffer's first bytes as the next key and IV at once
    /// (backtracking resistance).
    fn _rs_rekey(&mut self, dat: Option<&[u8]>) {
        // KEYSTREAM_ONLY: fill rs_buf with the keystream.
        self.rs.keystream_bytes(&mut self.rs_buf);
        // mix in optional user provided data
        if let Some(dat) = dat {
            let m = dat.len().min(KEYSZ + IVSZ);
            for (b, d) in self.rs_buf[..m].iter_mut().zip(dat) {
                *b ^= d;
            }
        }
        // immediately reinit for backtracking resistance
        _rs_init(&mut self.rs, &self.rs_buf[..KEYSZ + IVSZ]);
        self.rs_buf[..KEYSZ + IVSZ].fill(0);
        self.rs_have = RSBUFSZ - KEYSZ - IVSZ;
    }

    /// The copy loop of `_rs_random_buf`: hands out the buffered keystream (wiping what it
    /// gives), rekeying whenever the buffer runs dry.
    fn rs_fill(&mut self, buf: &mut [u8]) {
        let mut done = 0;
        while done < buf.len() {
            if self.rs_have > 0 {
                let m = (buf.len() - done).min(self.rs_have);
                let start = RSBUFSZ - self.rs_have;
                buf[done..done + m].copy_from_slice(&self.rs_buf[start..start + m]);
                self.rs_buf[start..start + m].fill(0);
                done += m;
                self.rs_have -= m;
            }
            if self.rs_have == 0 {
                self._rs_rekey(None);
            }
        }
    }
}

/// `struct arc4random_ctx`: a ChaCha20 generator of the caller's own, `malloc`ed (`M_TEMP`)
/// by [`arc4random_ctx_new`] and released by [`arc4random_ctx_free`].
pub struct Arc4randomCtx(NonNull<ChachaCtx>);

/// `rnd_event_space[]`: the entropy input ring.
pub static RND_EVENT_SPACE: [RandEvent; QEVLEN as usize] =
    [const { RandEvent::new() }; QEVLEN as usize];
/// `rnd_event_cons`: the ring's consumer count.
pub static RND_EVENT_CONS: AtomicU32 = AtomicU32::new(0);
/// `rnd_event_prod`: the ring's producer count.
pub static RND_EVENT_PROD: AtomicU32 = AtomicU32::new(0);
/// `rnd_cold`: every sample is dequeued and stirred in at once (until `cold` is over).
static RND_COLD: AtomicBool = AtomicBool::new(true);
/// `rnd_slowextract`: the dequeue timeout's delay, in tens of milliseconds.
static RND_SLOWEXTRACT: AtomicU32 = AtomicU32::new(1);

/// `entropy_pool[]` (and `add_entropy_words`'s pointer and rotation).
pub static ENTROPY_POOL: EntropyPool = EntropyPool::new();
/// `entropy_pool0[]`: the pool's boot seed, filled by the boot loader.
#[cfg_attr(target_os = "none", unsafe(link_section = ".openbsd.randomdata"))]
static ENTROPY_POOL0: [u32; POOLWORDS] = [0; POOLWORDS];

/// `rnd_timeout`: runs `dequeue_randomness`.
pub static RND_TIMEOUT: Timeout = Timeout::new(dequeue_randomness, ptr::null_mut());

/// `extract_entropy`'s `extract_pool[]`: its copy of the pool (static, as in C: 8 KB is
/// too much for a kernel stack).
static EXTRACT_POOL: [AtomicU32; POOLWORDS] = [const { AtomicU32::new(0) }; POOLWORDS];

/// `rndlock`: the ChaCha20 generator's lock.
pub static RNDLOCK: Mutex = Mutex::new(IPL_HIGH);
/// `rndreinit_timeout`: runs `rnd_reinit`.
static RNDREINIT_TIMEOUT: Timeout = Timeout::new(rnd_reinit, ptr::null_mut());
/// `rnd_task`: runs `rnd_init` on `systq`.
static RND_TASK: Task = Task::new(rnd_init, ptr::null_mut());

/// `rs`, `rs_buf`, `rs_have`, `rs_count`: the generator, under [`RNDLOCK`].
static RS: StaticCell<RsState> = StaticCell::new(RsState::new());
/// The host tests' stand-in for what `rndlock` does between CPUs (see the module's
/// deviations).
#[cfg(test)]
static HOST_RNDLOCK: AtomicBool = AtomicBool::new(false);
/// `rs_buf0[]`: the generator's boot seed (keystream blocks), filled by the boot loader.
#[cfg_attr(target_os = "none", unsafe(link_section = ".openbsd.randomdata"))]
static RS_BUF0: [u8; RSBUFSZ] = [0; RSBUFSZ];

/// `randomread_filtops`.
pub static RANDOMREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_randomdetach),
    f_event: Some(filt_randomread),
    f_modify: None,
    f_process: None,
};

/// `randomwrite_filtops`.
pub static RANDOMWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_randomdetach),
    f_event: Some(filt_randomwrite),
    f_modify: None,
    f_process: None,
};

/// `mtx_enter(&rndlock)` (and, in the host tests, the host's flag: see the module's
/// deviations).
fn rndlock_enter() {
    #[cfg(test)]
    while HOST_RNDLOCK.swap(true, Ordering::Acquire) {
        core::hint::spin_loop();
    }
    mtx_enter(&RNDLOCK);
}

/// `mtx_leave(&rndlock)`.
fn rndlock_leave() {
    mtx_leave(&RNDLOCK);
    #[cfg(test)]
    HOST_RNDLOCK.store(false, Ordering::Release);
}

/// The generator's state.
///
/// # Safety
///
/// The caller holds [`RNDLOCK`] and no other reference to the state is live.
unsafe fn rs_state() -> &'static mut RsState {
    // SAFETY: the caller holds rndlock, which serialises every access to the state.
    unsafe { RS.get_mut() }
}

/// `add_event_data`: mixes `val` and the timing of the moment into the next slot of the
/// entropy input ring, whose index it returns (for the tests; the C's returns nothing).
fn add_event_data(val: u32) -> u32 {
    let e = RND_EVENT_PROD.fetch_add(1, Ordering::Relaxed) & (QEVLEN - 1);
    let rep = &RND_EVENT_SPACE[e as usize];
    let t = rep.re_time.load(Ordering::Relaxed);
    rep.re_time
        .store(t.wrapping_add(cpu_rnd_messybits()), Ordering::Relaxed);
    let v = rep.re_val.load(Ordering::Relaxed);
    rep.re_val.store(v.wrapping_add(val), Ordering::Relaxed);
    e
}

/// `enqueue_randomness`: submits a sample to the entropy input ring, from interrupt context
/// or not. While cold it is mixed in and stirred at once; afterwards a timeout dequeues the
/// ring, sooner the fuller it is.
pub fn enqueue_randomness(val: u32) {
    let _ = add_event_data(val);

    if RND_COLD.load(Ordering::Relaxed) {
        dequeue_randomness(ptr::null_mut());
        rnd_init(ptr::null_mut());
        if !COLD.load(Ordering::Relaxed) {
            RND_COLD.store(false, Ordering::Relaxed);
        }
    } else if !timeout_pending(&RND_TIMEOUT)
        && RND_EVENT_PROD
            .load(Ordering::Relaxed)
            .wrapping_sub(RND_EVENT_CONS.load(Ordering::Relaxed))
            > QEVCONSUME
    {
        let slow = (RND_SLOWEXTRACT.load(Ordering::Relaxed) * 2).min(5000);
        RND_SLOWEXTRACT.store(slow, Ordering::Relaxed);
        timeout_add_msec(&RND_TIMEOUT, u64::from(slow) * 10);
    }
}

/// `add_entropy_words(buf, n)`: merges entropy ring information into the pool, using a
/// polynomial to spread the bits.
pub fn add_entropy_words(buf: &[u32]) {
    ENTROPY_POOL.add_entropy_words(buf.iter().copied());
}

/// `dequeue_randomness`: pulls entropy out of the queue and merges it into the pool with the
/// CRC: a mix of fresh entries from the producer end of the queue and entries from the
/// consumer end, which are likely to have collected more damage. `rnd_timeout`'s function.
pub fn dequeue_randomness(_v: *mut c_void) {
    let slot = |e: u32| {
        let ev = &RND_EVENT_SPACE[(e & (QEVLEN - 1)) as usize];
        [
            ev.re_time.load(Ordering::Relaxed),
            ev.re_val.load(Ordering::Relaxed),
        ]
    };

    // Some very new damage
    let startp = RND_EVENT_PROD
        .load(Ordering::Relaxed)
        .wrapping_sub(QEVCONSUME);
    for i in 0..QEVCONSUME {
        ENTROPY_POOL.add_entropy_words(slot(startp.wrapping_add(i)));
    }
    // and some probably more damaged
    let startc = RND_EVENT_CONS.fetch_add(QEVCONSUME, Ordering::Relaxed);
    for i in 0..QEVCONSUME {
        ENTROPY_POOL.add_entropy_words(slot(startc.wrapping_add(i)));
    }
}

/// `extract_entropy`: grabs the pool and slams it through SHA-512: a key and an IV for the
/// generator. The pool is then modified so the next hash differs.
pub fn extract_entropy(buf: &mut [u8; EBUFSIZE]) {

    // INTENTIONALLY not protected by any lock. Races during the copy result in acceptable
    // input data; races during the hashing would create nasty data dependencies. We do not
    // rely on this as a benefit, but if it happens, cool.
    for (d, s) in EXTRACT_POOL.iter().zip(&ENTROPY_POOL.words) {
        d.store(s.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    // Hash the pool to get the output.
    let mut shactx = Sha512Ctx::new();
    let mut block = [0u8; SHA512_BLOCK_LENGTH];
    for words in EXTRACT_POOL.chunks(SHA512_BLOCK_LENGTH / 4) {
        for (b, w) in block.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *b = w.load(Ordering::Relaxed).to_ne_bytes();
        }
        shactx.update(&block);
    }
    explicit_bzero(&mut block);
    let mut digest: [u8; SHA512_DIGEST_LENGTH] = shactx.finalize();

    // Copy data to destination buffer.
    buf.copy_from_slice(&digest[..EBUFSIZE]);

    // Modify pool so next hash will produce different results.
    let _ = add_event_data(EXTRACT_POOL[0].load(Ordering::Relaxed));
    dequeue_randomness(ptr::null_mut());

    // Wipe data from memory.
    for w in &EXTRACT_POOL {
        w.store(0, Ordering::Relaxed);
    }
    explicit_bzero(&mut digest);
}

/// The bytes of `ts` in memory order (`struct timespec`: `tv_sec`, then `tv_nsec`).
fn timespec_bytes(ts: &Timespec) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&ts.tv_sec.to_ne_bytes());
    b[8..].copy_from_slice(&ts.tv_nsec.to_ne_bytes());
    b
}

/// `suspend_randomness`: before a suspend, mixes in the time, forces a reseed at the next
/// use and refills the pool from the generator.
pub fn suspend_randomness() {
    let ts = getnanotime();
    enqueue_randomness(ts.tv_sec as u32);
    enqueue_randomness(ts.tv_nsec as u32);

    dequeue_randomness(ptr::null_mut());

    rndlock_enter();
    // SAFETY: rndlock is held.
    let rs = unsafe { rs_state() };
    rs.rs_count = 0;
    // arc4random_buf(entropy_pool, sizeof(entropy_pool)), 64 bytes at a time.
    _rs_stir_if_needed(rs, POOLBYTES);
    let mut chunk = [0u8; BLOCKSZ];
    for words in ENTROPY_POOL.words.chunks(BLOCKSZ / 4) {
        rs.rs_fill(&mut chunk);
        for (w, b) in words.iter().zip(chunk.as_chunks::<4>().0) {
            w.store(u32::from_ne_bytes(*b), Ordering::Relaxed);
        }
    }
    rndlock_leave();
    explicit_bzero(&mut chunk);
}

/// `resume_randomness`: after a resume, reseeds the generator with the loader's fresh
/// `buf` (when given) and mixes in the time.
pub fn resume_randomness(buf: Option<&[u8]>) {
    if let Some(buf) = buf
        && !buf.is_empty()
    {
        rndlock_enter();
        // SAFETY: rndlock is held.
        unsafe { rs_state() }._rs_seed(buf);
        rndlock_leave();
    }
    let ts = getnanotime();
    enqueue_randomness(ts.tv_sec as u32);
    enqueue_randomness(ts.tv_nsec as u32);

    dequeue_randomness(ptr::null_mut());
    rndlock_enter();
    // SAFETY: rndlock is held.
    unsafe { rs_state() }.rs_count = 0;
    rndlock_leave();
}

/// `_rs_init(buf, n)`: keys `rs` from the first `KEYSZ` bytes of `buf` and takes the next
/// `IVSZ` as the IV.
fn _rs_init(rs: &mut ChachaCtx, buf: &[u8]) {
    kassert!(buf.len() >= KEYSZ + IVSZ);
    if let (Some(key), Some(iv)) = (
        buf[..KEYSZ].first_chunk::<KEYSZ>(),
        buf[KEYSZ..KEYSZ + IVSZ].first_chunk::<IVSZ>(),
    ) {
        *rs = ChachaCtx::new(key);
        rs.ivsetup(iv, None);
    }
}

/// `_rs_stir(do_lock)`: reseeds the generator from SHA-512 of the pool and the time (early
/// in boot this is the best we can do: some architectures do not collect entropy well
/// then, but may have clock information, better than nothing). `locked` is the state when
/// the caller holds `rndlock` (`do_lock == 0`); `None` takes the lock here.
fn _rs_stir(locked: Option<&mut RsState>) {
    let mut buf = [0u8; EBUFSIZE];

    extract_entropy(&mut buf);

    let ts = nanotime();
    for (b, t) in buf.iter_mut().zip(timespec_bytes(&ts)) {
        *b ^= t;
    }

    match locked {
        Some(rs) => rs._rs_seed(&buf),
        None => {
            rndlock_enter();
            // SAFETY: rndlock is held.
            unsafe { rs_state() }._rs_seed(&buf);
            rndlock_leave();
        }
    }
    explicit_bzero(&mut buf);

    // encourage fast-dequeue again
    RND_SLOWEXTRACT.store(1, Ordering::Relaxed);
}

/// `_rs_stir_if_needed(len)`: on the first use, keys the generator from the boot seed (and
/// seeds the pool); afterwards reseeds once `rs_count` bytes have been handed out.
fn _rs_stir_if_needed(rs: &mut RsState, len: usize) {
    if !rs.rs_initialized {
        for (i, w) in ENTROPY_POOL.words.iter().enumerate() {
            // SAFETY: an in-bounds read of a static the boot loader may have written.
            let seed = unsafe { ptr::read_volatile(ENTROPY_POOL0.as_ptr().add(i)) };
            w.store(seed, Ordering::Relaxed);
        }
        for (i, b) in rs.rs_buf.iter_mut().enumerate() {
            // SAFETY: as above.
            *b = unsafe { ptr::read_volatile(RS_BUF0.as_ptr().add(i)) };
        }
        // seeds cannot be cleaned yet, random_start() will do so
        _rs_init(&mut rs.rs, &rs.rs_buf[..KEYSZ + IVSZ]);
        rs.rs_count = 1024 * 1024 * 1024; // until main() runs
        rs.rs_initialized = true;
    } else if rs.rs_count <= len {
        _rs_stir(Some(rs));
    } else {
        rs.rs_count -= len;
    }
}

/// `_rs_clearseed(p, s)`: wipes `s` bytes of the read-only boot seed at `p`, through a
/// writable mapping of each of its pages made for the purpose.
fn _rs_clearseed(p: *const u8, mut s: usize) {
    let mut va = trunc_page(p as usize);
    let mut off = p as usize - va;

    while s > 0 {
        let Some(pa) = pmap_extract(pmap_kernel(), Vaddr::new(va)) else {
            panic(format_args!("_rs_clearseed"));
        };

        let kd_avoidalias = KmemDynMode {
            kd_prefer: pa.as_usize() as Voff,
            kd_slowdown: false,
            kd_waitok: true,
            kd_trylock: false,
        };
        let Some(rwva) = km_alloc(PAGE_SIZE, &KV_ANY, &KP_NONE, &kd_avoidalias) else {
            panic(format_args!("_rs_clearseed"));
        };
        let rw = Vaddr::new(rwva.as_ptr() as usize);

        // SAFETY: `rwva` is the page of kernel virtual space km_alloc just gave us, with no
        // physical page behind it (kp_none); `pa` is the seed's own page.
        unsafe { pmap_kenter_pa(rw, pa, PROT_READ | PROT_WRITE) };
        pmap_update(pmap_kernel());

        let len = s.min(PAGE_SIZE - off);
        // SAFETY: `rw` maps the seed's page read-write; `off + len` stays inside it. The
        // seed is never read through its own (read-only) address after random_start.
        explicit_bzero(unsafe { core::slice::from_raw_parts_mut(rwva.as_ptr().add(off), len) });

        // SAFETY: the page was entered just above and nothing uses it afterwards.
        unsafe { pmap_kremove(rw, Vsize::new(PAGE_SIZE)) };
        km_free(rwva, PAGE_SIZE, &KV_ANY, &KP_NONE);

        va += PAGE_SIZE;
        s -= len;
        off = 0;
    }
}

/// `_rs_random_buf`: fills `buf` from the generator.
fn _rs_random_buf(rs: &mut RsState, buf: &mut [u8]) {
    _rs_stir_if_needed(rs, buf.len());
    rs.rs_fill(buf);
}

/// `_rs_random_u32`: one word from the generator.
fn _rs_random_u32(rs: &mut RsState) -> u32 {
    _rs_stir_if_needed(rs, size_of::<u32>());
    if rs.rs_have < size_of::<u32>() {
        rs._rs_rekey(None);
    }
    let start = RSBUFSZ - rs.rs_have;
    let b = &mut rs.rs_buf[start..start + size_of::<u32>()];
    let val = u32::from_ne_bytes([b[0], b[1], b[2], b[3]]);
    b.fill(0);
    rs.rs_have -= size_of::<u32>();
    val
}

/// `arc4random`: one word of randomness from the ChaCha20 generator.
pub fn arc4random() -> u32 {
    rndlock_enter();
    // SAFETY: rndlock is held.
    let ret = _rs_random_u32(unsafe { rs_state() });
    rndlock_leave();
    ret
}

/// `arc4random_buf`: fills a buffer of arbitrary length with ChaCha20-derived randomness.
pub fn arc4random_buf(buf: &mut [u8]) {
    rndlock_enter();
    // SAFETY: rndlock is held.
    _rs_random_buf(unsafe { rs_state() }, buf);
    rndlock_leave();
}

/// `arc4random_ctx_new`: a new ChaCha20 generator for the caller, keyed from the main one.
pub fn arc4random_ctx_new() -> Option<Arc4randomCtx> {
    let mut keybuf = [0u8; KEYSZ + IVSZ];

    let ctx = malloc(size_of::<ChachaCtx>(), M_TEMP, M_WAITOK)?.cast::<ChachaCtx>();
    // SAFETY: a fresh allocation of `sizeof(chacha_ctx)` bytes, aligned for any kernel type
    // (malloc's buckets), ours alone.
    let c = unsafe {
        ctx.as_ptr().write(ChachaCtx::default());
        &mut *ctx.as_ptr()
    };
    arc4random_buf(&mut keybuf);
    if let (Some(key), Some(iv)) = (
        keybuf[..KEYSZ].first_chunk::<KEYSZ>(),
        keybuf[KEYSZ..].first_chunk::<IVSZ>(),
    ) {
        *c = ChachaCtx::new(key);
        c.ivsetup(iv, None);
    }
    explicit_bzero(&mut keybuf);
    Some(Arc4randomCtx(ctx))
}

/// `arc4random_ctx_free`: wipes and frees a context from [`arc4random_ctx_new`].
pub fn arc4random_ctx_free(ctx: Arc4randomCtx) {
    let p = ctx.0;
    // SAFETY: `p` is the context's own allocation, `sizeof(chacha_ctx)` bytes, which
    // nothing else references (the handle is consumed here).
    explicit_bzero(unsafe {
        core::slice::from_raw_parts_mut(p.as_ptr().cast::<u8>(), size_of::<ChachaCtx>())
    });
    free(p.cast::<u8>(), M_TEMP, size_of::<ChachaCtx>());
}

/// `arc4random_ctx_buf`: fills `buf` from the caller's own generator.
pub fn arc4random_ctx_buf(ctx: &mut Arc4randomCtx, buf: &mut [u8]) {
    // SAFETY: the handle owns the context (arc4random_ctx_new); `&mut` excludes other users.
    let c = unsafe { ctx.0.as_mut() };
    // KEYSTREAM_ONLY: chacha_encrypt_bytes(ctx, buf, buf, n) writes the keystream.
    c.keystream_bytes(buf);
}

/// `arc4random_uniform`: a uniformly distributed random number less than `upper_bound`,
/// avoiding "modulo bias".
///
/// Uniformity is achieved by generating new random numbers until the one returned is
/// outside the range `[0, 2**32 % upper_bound)`. This guarantees the selected random number
/// will be inside `[2**32 % upper_bound, 2**32)`, which maps back to `[0, upper_bound)`
/// after reduction modulo `upper_bound`.
pub fn arc4random_uniform(upper_bound: u32) -> u32 {
    if upper_bound < 2 {
        return 0;
    }

    // 2**32 % x == (2**32 - x) % x
    let min = upper_bound.wrapping_neg() % upper_bound;

    // This could theoretically loop forever but each retry has p > 0.5 (worst case, usually
    // far better) of selecting a number inside the range we need, so it should rarely need
    // to re-roll.
    let r = loop {
        let r = arc4random();
        if r >= min {
            break r;
        }
    };

    r % upper_bound
}

/// `rnd_init`: reseeds the generator (`rnd_task`'s function).
pub fn rnd_init(_null: *mut c_void) {
    _rs_stir(None);
}

/// `rnd_reinit`: called by timeout to mark the generator for stirring: queues `rnd_task`
/// and rearms itself in 10 minutes (per dm@'s suggestion).
pub fn rnd_reinit(_v: *mut c_void) {
    task_add(SYSTQ, &RND_TASK);
    timeout_add_sec(&RNDREINIT_TIMEOUT, 10 * 60);
}

/// `random_start`: starts the periodic services of the random subsystem, which pull entropy
/// forward, hash it, and reseed the generator as needed. `main` calls it once, with
/// `boothowto & RB_GOODRANDOM`.
pub fn random_start(goodseed: bool) {
    // !NO_PROPOLICE
    // SAFETY: a read of a static the boot loader may have written; volatile so the compiler
    // never assumes its initial value.
    if unsafe { ptr::read_volatile(&GUARD_LOCAL) } == 0 {
        printf(format_args!(
            "warning: no entropy supplied by boot loader\n"
        ));
    }

    _rs_clearseed(ENTROPY_POOL0.as_ptr().cast(), size_of::<[u32; POOLWORDS]>());
    _rs_clearseed(RS_BUF0.as_ptr(), RSBUFSZ);

    // Message buffer may contain data from previous boot
    if let Some(mbp) = msgbufp()
        && mbp.magic() == MSG_MAGIC
    {
        let n = mbp.bufs() as usize / size_of::<u32>();
        ENTROPY_POOL.add_entropy_words(
            mbp.bufc()
                .as_chunks::<4>()
                .0
                .iter()
                .take(n)
                .map(|c| u32::from_ne_bytes(c.each_ref().map(Cell::get))),
        );
    }
    let text = (etext() - 32 * 1024 * size_of::<u32>()) as *const u32;
    // SAFETY: the 8 KB that start 128 KB before `etext` are kernel text, mapped readable
    // for the kernel's lifetime and never written.
    add_entropy_words(unsafe { core::slice::from_raw_parts(text, 8192 / size_of::<u32>()) });

    dequeue_randomness(ptr::null_mut());
    rnd_init(ptr::null_mut());
    rnd_reinit(ptr::null_mut());

    if goodseed {
        printf(format_args!("random: good seed from bootblocks\n"));
    } else {
        // XXX kernel should work harder here
        printf(format_args!(
            "random: boothowto does not indicate good seed\n"
        ));
    }
}

/// `randomopen`.
pub fn randomopen(_dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `randomclose`.
pub fn randomclose(_dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// A `malloc(POOLBYTES, M_TEMP, M_WAITOK)` buffer as a byte slice.
fn poolbytes_buf() -> Option<(NonNull<u8>, &'static mut [u8])> {
    let mem = malloc(POOLBYTES, M_TEMP, M_WAITOK)?;
    // SAFETY: a fresh allocation of POOLBYTES bytes, ours until `free`.
    let buf = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), POOLBYTES) };
    Some((mem, buf))
}

/// `randomread`: random bytes from the generator, `POOLBYTES` at a time; a read of more
/// than [`RND_MAIN_MAX_BYTES`] is served by a ChaCha20 instance of its own.
pub fn randomread(_dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    let total = uio.uio_resid;

    if uio.uio_resid == 0 {
        return Ok(());
    }

    let Some((mem, buf)) = poolbytes_buf() else {
        return Err(Errno::ENOMEM);
    };
    let mut lctx = None;
    if total > RND_MAIN_MAX_BYTES {
        lctx = arc4random_ctx_new();
        if lctx.is_none() {
            free(mem, M_TEMP, POOLBYTES);
            return Err(Errno::ENOMEM);
        }
    }

    let mut ret = Ok(());
    while ret.is_ok() && uio.uio_resid > 0 {
        let n = POOLBYTES.min(uio.uio_resid);

        match lctx.as_mut() {
            Some(ctx) => arc4random_ctx_buf(ctx, &mut buf[..n]),
            None => arc4random_buf(&mut buf[..n]),
        }
        ret = uiomove(&mut buf[..n], uio);
        if ret.is_ok() && uio.uio_resid > 0 {
            r#yield();
        }
    }
    if let Some(ctx) = lctx {
        arc4random_ctx_free(ctx);
    }
    explicit_bzero(buf);
    free(mem, M_TEMP, POOLBYTES);
    ret
}

/// `randomwrite`: mixes what is written into the pool and then reseeds the generator.
pub fn randomwrite(_dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    let mut newdata = false;

    if uio.uio_resid == 0 {
        return Ok(());
    }

    let Some((mem, buf)) = poolbytes_buf() else {
        return Err(Errno::ENOMEM);
    };

    let mut ret = Ok(());
    while ret.is_ok() && uio.uio_resid > 0 {
        let mut n = POOLBYTES.min(uio.uio_resid);

        ret = uiomove(&mut buf[..n], uio);
        if ret.is_err() {
            break;
        }
        while !n.is_multiple_of(size_of::<u32>()) {
            buf[n] = 0;
            n += 1;
        }
        ENTROPY_POOL.add_entropy_words(
            buf[..n]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c)),
        );
        if uio.uio_resid > 0 {
            r#yield();
        }
        newdata = true;
    }

    if newdata {
        rnd_init(ptr::null_mut());
    }

    explicit_bzero(buf);
    free(mem, M_TEMP, POOLBYTES);
    ret
}

/// `randomkqfilter`: reading and writing are always possible.
pub fn randomkqfilter(_dev: Dev, kn: &Knote) -> Result<(), Errno> {
    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&RANDOMREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&RANDOMWRITE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `filt_randomdetach`: nothing to undo.
pub fn filt_randomdetach(_kn: &Knote) {}

/// `filt_randomread`: [`RND_MAIN_MAX_BYTES`] can always be read.
pub fn filt_randomread(kn: &Knote, _hint: i64) -> bool {
    kn.kn_data().set(RND_MAIN_MAX_BYTES as i64);
    true
}

/// `filt_randomwrite`: [`POOLBYTES`] can always be written.
pub fn filt_randomwrite(kn: &Knote, _hint: i64) -> bool {
    kn.kn_data().set(POOLBYTES as i64);
    true
}

/// `randomioctl`: `FIOASYNC` is accepted (there is no async flag in a softc, so it is a
/// no-op); anything else is `ENOTTY`.
pub fn randomioctl(
    _dev: Dev,
    cmd: u64,
    _data: &mut [u8],
    _flag: i32,
    _p: &Proc,
) -> Result<(), Errno> {
    match cmd {
        FIOASYNC => Ok(()),
        _ => Err(Errno::ENOTTY),
    }
}

/// `getentropy(2)`: at most `GETENTROPY_MAX` bytes from the kernel's generator.
pub fn sys_getentropy(_p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetentropyArgs = sysargs(v);
    let mut buf = [0u8; GETENTROPY_MAX];
    let nbyte = uap.nbyte.get();

    if nbyte > buf.len() {
        return Err(Errno::EINVAL);
    }
    arc4random_buf(&mut buf[..nbyte]);
    copyout(&buf[..nbyte], uap.buf.get() as usize)?;
    explicit_bzero(&mut buf);
    retval[0] = 0;
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// ChaCha20's keystream for the all-zero key and IV, block 0 (the published test vector
    /// of the original 64-bit-nonce construction).
    const ZERO_KEYSTREAM: [u8; 64] = [
        0x76, 0xb8, 0xe0, 0xad, 0xa0, 0xf1, 0x3d, 0x90, 0x40, 0x5d, 0x6a, 0xe5, 0x53, 0x86, 0xbd,
        0x28, 0xbd, 0xd2, 0x19, 0xb8, 0xa0, 0x8d, 0xed, 0x1a, 0xa8, 0x36, 0xef, 0xcc, 0x8b, 0x77,
        0x0d, 0xc7, 0xda, 0x41, 0x59, 0x7c, 0x51, 0x57, 0x48, 0x8d, 0x77, 0x24, 0xe0, 0x3f, 0xb8,
        0xd8, 0x4a, 0x37, 0x6a, 0x43, 0xb8, 0xf4, 0x15, 0x18, 0xa1, 0x1c, 0xc3, 0x87, 0xb6, 0x69,
        0xb2, 0xee, 0x65, 0x86,
    ];

    /// A generator keyed from an all-zero seed, as on a boot without a loader seed.
    fn zero_seeded() -> RsState {
        let mut rs = RsState::new();
        _rs_init(&mut rs.rs, &[0u8; KEYSZ + IVSZ]);
        rs
    }

    #[test]
    fn rekey_takes_the_next_key_from_the_keystream() {
        let mut rs = zero_seeded();
        rs._rs_rekey(None);

        // The first 40 bytes of the keystream became the key and the IV, and were wiped.
        let mut want = ChachaCtx::default();
        _rs_init(&mut want, &ZERO_KEYSTREAM[..KEYSZ + IVSZ]);
        assert_eq!(rs.rs.input, want.input);
        assert!(rs.rs_buf[..KEYSZ + IVSZ].iter().all(|&b| b == 0));
        assert_eq!(rs.rs_have, RSBUFSZ - KEYSZ - IVSZ);

        // What is handed out next is the keystream after them, wiped as it goes.
        let mut out = [0u8; 24];
        rs.rs_fill(&mut out);
        assert_eq!(out, ZERO_KEYSTREAM[KEYSZ + IVSZ..]);
        assert_eq!(rs.rs_have, RSBUFSZ - KEYSZ - IVSZ - 24);
        assert!(rs.rs_buf[..KEYSZ + IVSZ + 24].iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_rekeys_when_the_buffer_runs_dry() {
        let mut rs = zero_seeded();
        rs._rs_rekey(None);
        let mut big = [0u8; RSBUFSZ];
        rs.rs_fill(&mut big);
        // 984 buffered bytes, a rekey, then 40 more from the next buffer.
        assert_eq!(
            rs.rs_have,
            RSBUFSZ - KEYSZ - IVSZ - (RSBUFSZ - (RSBUFSZ - KEYSZ - IVSZ))
        );
        assert_eq!(big[..24], ZERO_KEYSTREAM[KEYSZ + IVSZ..]);
    }

    #[test]
    fn seed_mixes_the_data_into_the_next_key() {
        let mut a = zero_seeded();
        let mut b = zero_seeded();
        a._rs_seed(&[0u8; EBUFSIZE]);
        b._rs_seed(&[1u8; EBUFSIZE]);
        assert_ne!(a.rs.input, b.rs.input);
        assert_eq!(a.rs_have, 0);
        assert_eq!(a.rs_count, 1_600_000);
        assert!(a.rs_buf.iter().all(|&x| x == 0));

        // With zero data, the seed is the plain rekey.
        let mut c = zero_seeded();
        c._rs_rekey(None);
        assert_eq!(a.rs.input, c.rs.input);
    }

    #[test]
    fn random_u32_takes_native_order_words() {
        let mut rs = zero_seeded();
        rs._rs_rekey(None);
        rs.rs_initialized = true;
        rs.rs_count = usize::MAX;
        let w = _rs_random_u32(&mut rs);
        let k = &ZERO_KEYSTREAM[KEYSZ + IVSZ..KEYSZ + IVSZ + 4];
        assert_eq!(w, u32::from_ne_bytes([k[0], k[1], k[2], k[3]]));
        assert_eq!(rs.rs_count, usize::MAX - 4);
    }

    #[test]
    fn pool_mixing_follows_the_twisted_gfsr() {
        let pool = EntropyPool::new();
        // From an all-zero pool: the first word goes to the last slot, unrotated, and only
        // the twist touches it; the rotation moves on by 7.
        pool.add_entropy_words([1]);
        assert_eq!(pool.word(POOLWORDS - 1), (1 >> 3) ^ TWIST_TABLE[1]);
        assert_eq!(pool.input_rotate.load(Ordering::Relaxed), 7);
        assert_eq!(pool.add_ptr.load(Ordering::Relaxed), POOLMASK);

        // The second word is rotated by 7 and XORed with the taps, slot POOLWORDS - 1 among
        // them (i + 1).
        pool.add_entropy_words([0x8000_0001]);
        let i = POOLWORDS - 2;
        let w = 0x8000_0001u32.rotate_left(7) ^ pool.word(POOLWORDS - 1);
        assert_eq!(pool.word(i), (w >> 3) ^ TWIST_TABLE[(w & 7) as usize]);
        assert_eq!(pool.input_rotate.load(Ordering::Relaxed), 14);
    }

    #[test]
    fn pool_start_adds_the_extra_rotation() {
        let pool = EntropyPool::new();
        pool.add_entropy_words(core::iter::repeat_n(0u32, POOLWORDS - 1));
        // POOLWORDS - 1 words: the pointer is at 1, the rotation moved 7 each time.
        assert_eq!(pool.add_ptr.load(Ordering::Relaxed), 1);
        let r = ((POOLWORDS - 1) * 7 % 32) as u8;
        assert_eq!(pool.input_rotate.load(Ordering::Relaxed), r);
        // The word at slot 0 adds 14.
        pool.add_entropy_words([0]);
        assert_eq!(pool.add_ptr.load(Ordering::Relaxed), 0);
        assert_eq!(pool.input_rotate.load(Ordering::Relaxed), (r + 14) & 31);
    }

    #[test]
    fn samples_land_in_the_input_ring() {
        // What viornd(4)'s interrupt does with the device's words: one enqueue_randomness
        // per word, each added into the ring slot the producer count names.
        for word in [0x1234_5678u32, 0x9abc_def0, 0x0bad_cafe, 0xdead_beef] {
            loop {
                let want = RND_EVENT_PROD.load(Ordering::Relaxed) & (QEVLEN - 1);
                let before = RND_EVENT_SPACE[want as usize]
                    .re_val
                    .load(Ordering::Relaxed);
                let e = add_event_data(word);
                if e != want {
                    continue; // another test's sample took the slot first
                }
                let after = RND_EVENT_SPACE[e as usize].re_val.load(Ordering::Relaxed);
                assert_eq!(after.wrapping_sub(before), word);
                break;
            }
        }
    }

    #[test]
    fn enqueue_reaches_the_pool() {
        let snapshot: [u32; POOLWORDS] = core::array::from_fn(|i| ENTROPY_POOL.word(i));
        let cons = RND_EVENT_CONS.load(Ordering::Relaxed);
        enqueue_randomness(0x5eed_0001);
        dequeue_randomness(ptr::null_mut());
        assert!(RND_EVENT_CONS.load(Ordering::Relaxed).wrapping_sub(cons) >= QEVCONSUME);
        assert!((0..POOLWORDS).any(|i| ENTROPY_POOL.word(i) != snapshot[i]));
    }

    #[test]
    fn extraction_changes_the_pool_for_the_next_one() {
        let mut a = [0u8; EBUFSIZE];
        let mut b = [0u8; EBUFSIZE];
        extract_entropy(&mut a);
        extract_entropy(&mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn generator_moves_and_fills() {
        let a = arc4random();
        let b = arc4random();
        let c = arc4random();
        assert!(a != b || b != c);
        let mut buf = [0u8; 13];
        arc4random_buf(&mut buf);
        assert!(buf.iter().any(|&x| x != 0));
    }

    #[test]
    fn uniform_stays_below_the_bound() {
        assert_eq!(arc4random_uniform(0), 0);
        assert_eq!(arc4random_uniform(1), 0);
        for bound in [2u32, 3, 7, 1000, 0x8000_0001, u32::MAX] {
            for _ in 0..200 {
                assert!(arc4random_uniform(bound) < bound);
            }
        }
    }

    #[test]
    fn uniform_threshold_is_two_to_the_32_mod_bound() {
        for x in [2u32, 3, 7, 10, 1000, 0x8000_0001, u32::MAX] {
            assert_eq!(u64::from(x.wrapping_neg() % x), (1u64 << 32) % u64::from(x));
        }
    }

    #[test]
    fn filters_report_what_can_move() {
        let kn = Knote::default();
        assert!(filt_randomread(&kn, 0));
        assert_eq!(kn.kn_data().get(), RND_MAIN_MAX_BYTES as i64);
        assert!(filt_randomwrite(&kn, 0));
        assert_eq!(kn.kn_data().get(), POOLBYTES as i64);
    }
}
/* </TESTS> */
