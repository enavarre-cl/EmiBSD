use super::*;

use core::cell::{Cell, RefCell};

/// A fake chip: 14 registers, register A's UIP bit set for the first `uip_reads` reads of it,
/// and the seconds register ticking over once, between the first sweep's two reads of it.
struct FakeRtc {
    regs: RefCell<[u32; 14]>,
    uip_reads: Cell<u32>,
    sec_ticks: Cell<u32>,
    log: RefCell<std::vec::Vec<(u32, u32)>>,
}

impl FakeRtc {
    fn new(regs: [u32; 14]) -> Self {
        Self {
            regs: RefCell::new(regs),
            uip_reads: Cell::new(0),
            sec_ticks: Cell::new(0),
            log: RefCell::new(std::vec::Vec::new()),
        }
    }

    fn read(&self, reg: u32) -> u32 {
        let mut v = self.regs.borrow()[reg as usize];
        if reg == MC_REGA && self.uip_reads.get() > 0 {
            self.uip_reads.set(self.uip_reads.get() - 1);
            v |= MC_REGA_UIP;
        }
        if reg == MC_SEC && self.sec_ticks.get() > 0 {
            // The seconds changed since the sweep read them: the sweep must go round again.
            self.sec_ticks.set(self.sec_ticks.get() - 1);
            self.regs.borrow_mut()[MC_SEC as usize] += 1;
        }
        v
    }

    fn write(&self, reg: u32, datum: u32) {
        self.log.borrow_mut().push((reg, datum));
        self.regs.borrow_mut()[reg as usize] = datum;
    }
}

#[test]
fn gettod_waits_for_uip_and_retries_a_torn_read() {
    let rtc = FakeRtc::new([
        0x56, 0, 0x34, 0, 0x12, 0, 0x07, 0x03, 0x10, 0x26, 0, 0, 0, 0,
    ]);
    rtc.uip_reads.set(3);
    rtc.sec_ticks.set(1);

    let mut regs: McTodregs = [0; MC_NTODREGS];
    mc146818_gettod(&mut regs, |r| rtc.read(r));

    // The second sweep saw seconds 0x57 twice (the first sweep saw 0x56 then 0x57).
    assert_eq!(rtc.uip_reads.get(), 0);
    assert_eq!(rtc.sec_ticks.get(), 0);
    assert_eq!(regs[MC_SEC as usize], 0x57);
    assert_eq!(regs[MC_MIN as usize], 0x34);
    assert_eq!(regs[MC_HOUR as usize], 0x12);
    assert_eq!(regs[MC_DOM as usize], 0x03);
    assert_eq!(regs[MC_MONTH as usize], 0x10);
    assert_eq!(regs[MC_YEAR as usize], 0x26);
}

#[test]
fn puttod_brackets_the_write_with_set() {
    let rtc = FakeRtc::new([0; 14]);
    rtc.regs.borrow_mut()[MC_REGB as usize] = MC_REGB_24HR;

    let regs: McTodregs = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    mc146818_puttod(&regs, |r| rtc.read(r), |r, d| rtc.write(r, d));

    let log = rtc.log.borrow();
    assert_eq!(log.first(), Some(&(MC_REGB, MC_REGB_24HR | MC_REGB_SET)));
    assert_eq!(log.last(), Some(&(MC_REGB, MC_REGB_24HR)));
    assert_eq!(log.len(), 2 + MC_NTODREGS);
    for (i, want) in regs.iter().enumerate() {
        assert_eq!(log[1 + i], (i as u32, *want));
    }
    assert_eq!(rtc.regs.borrow()[MC_REGB as usize], MC_REGB_24HR);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/ic/mc146818reg.h");
    let ours: &[(&str, i64)] = &[
        ("MC_SEC", MC_SEC as i64),
        ("MC_ASEC", MC_ASEC as i64),
        ("MC_MIN", MC_MIN as i64),
        ("MC_AMIN", MC_AMIN as i64),
        ("MC_HOUR", MC_HOUR as i64),
        ("MC_AHOUR", MC_AHOUR as i64),
        ("MC_DOW", MC_DOW as i64),
        ("MC_DOM", MC_DOM as i64),
        ("MC_MONTH", MC_MONTH as i64),
        ("MC_YEAR", MC_YEAR as i64),
        ("MC_REGA", MC_REGA as i64),
        ("MC_REGA_RSMASK", MC_REGA_RSMASK as i64),
        ("MC_REGA_DVMASK", MC_REGA_DVMASK as i64),
        ("MC_REGA_UIP", MC_REGA_UIP as i64),
        ("MC_REGB", MC_REGB as i64),
        ("MC_REGB_DSE", MC_REGB_DSE as i64),
        ("MC_REGB_24HR", MC_REGB_24HR as i64),
        ("MC_REGB_BINARY", MC_REGB_BINARY as i64),
        ("MC_REGB_SQWE", MC_REGB_SQWE as i64),
        ("MC_REGB_UIE", MC_REGB_UIE as i64),
        ("MC_REGB_AIE", MC_REGB_AIE as i64),
        ("MC_REGB_PIE", MC_REGB_PIE as i64),
        ("MC_REGB_SET", MC_REGB_SET as i64),
        ("MC_REGC", MC_REGC as i64),
        ("MC_REGC_UF", MC_REGC_UF as i64),
        ("MC_REGC_AF", MC_REGC_AF as i64),
        ("MC_REGC_PF", MC_REGC_PF as i64),
        ("MC_REGC_IRQF", MC_REGC_IRQF as i64),
        ("MC_REGD", MC_REGD as i64),
        ("MC_REGD_VRT", MC_REGD_VRT as i64),
        ("MC_NREGS", MC_NREGS as i64),
        ("MC_NTODREGS", MC_NTODREGS as i64),
        ("MC_NVRAM_START", MC_NVRAM_START as i64),
        ("MC_NVRAM_SIZE", MC_NVRAM_SIZE as i64),
        ("MC_RATE_NONE", MC_RATE_NONE as i64),
        ("MC_RATE_1", MC_RATE_1 as i64),
        ("MC_RATE_2", MC_RATE_2 as i64),
        ("MC_RATE_8192_Hz", MC_RATE_8192_HZ as i64),
        ("MC_RATE_4096_Hz", MC_RATE_4096_HZ as i64),
        ("MC_RATE_2048_Hz", MC_RATE_2048_HZ as i64),
        ("MC_RATE_1024_Hz", MC_RATE_1024_HZ as i64),
        ("MC_RATE_512_Hz", MC_RATE_512_HZ as i64),
        ("MC_RATE_256_Hz", MC_RATE_256_HZ as i64),
        ("MC_RATE_128_Hz", MC_RATE_128_HZ as i64),
        ("MC_RATE_64_Hz", MC_RATE_64_HZ as i64),
        ("MC_RATE_32_Hz", MC_RATE_32_HZ as i64),
        ("MC_RATE_16_Hz", MC_RATE_16_HZ as i64),
        ("MC_RATE_8_Hz", MC_RATE_8_HZ as i64),
        ("MC_RATE_4_Hz", MC_RATE_4_HZ as i64),
        ("MC_RATE_2_Hz", MC_RATE_2_HZ as i64),
        ("MC_BASE_4_MHz", MC_BASE_4_MHZ as i64),
        ("MC_BASE_1_MHz", MC_BASE_1_MHZ as i64),
        ("MC_BASE_32_KHz", MC_BASE_32_KHZ as i64),
        ("MC_BASE_NONE", MC_BASE_NONE as i64),
        ("MC_BASE_RESET", MC_BASE_RESET as i64),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}
