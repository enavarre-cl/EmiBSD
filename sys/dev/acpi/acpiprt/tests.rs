//! Host tests of `acpiprt.rs`: reading and choosing a link device's interrupt from its
//! `_CRS`/`_PRS` descriptors.

use super::*;

/// An IRQ descriptor (`SR_IRQ`, three bytes of payload) for `mask` with `flags`.
fn sr_irq(mask: u16, flags: u8) -> [u8; 4] {
    let m = mask.to_le_bytes();
    [0x23, m[0], m[1], flags]
}

/// An extended interrupt descriptor (`LR_EXTIRQ`) listing `irqs` with `flags`.
fn lr_extirq(irqs: &[u32], flags: u8) -> std::vec::Vec<u8> {
    let len = (2 + 4 * irqs.len()) as u16;
    let mut b = std::vec![0x89, len.to_le_bytes()[0], len.to_le_bytes()[1], flags];
    b.push(irqs.len() as u8);
    for i in irqs {
        b.extend_from_slice(&i.to_le_bytes());
    }
    b
}

#[test]
fn getirq_reads_the_set_interrupt() {
    let mut irq = AcpiprtIrq::default();
    // IRQ 11, level, active low, shared (the PIIX links' _CRS).
    let d = sr_irq(1 << 11, SR_IRQ_SHR | SR_IRQ_POLARITY);
    acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(irq._int, 11);
    assert_ne!(irq._shr, 0);
    assert_ne!(irq._ll, 0);
    assert_eq!(irq._he, 0);

    // GSI 16, level, active high, shared (Q35's GSIx links).
    let d = lr_extirq(&[16], LR_EXTIRQ_SHR | 0x1);
    acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(
        irq,
        AcpiprtIrq {
            _int: 16,
            _shr: i32::from(LR_EXTIRQ_SHR),
            _ll: 0,
            _he: 0
        }
    );

    // No interrupt set: ffs(0) - 1.
    let d = sr_irq(0, 0);
    acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(irq._int, -1);
}

#[test]
fn chooseirq_prefers_the_generic_lines_and_the_ioapic() {
    let mut irq = AcpiprtIrq::default();
    // 5, 10 and 11 allowed: 10 and 11 weigh 7, the first of them wins.
    let d = sr_irq((1 << 5) | (1 << 10) | (1 << 11), 0);
    acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(irq._int, 10);

    // An I/O APIC input (> 15) is taken before any 8259 line.
    let d = lr_extirq(&[5, 10, 20], 0);
    acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(irq._int, 20);

    let d = lr_extirq(&[3, 9], 0);
    acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
    assert_eq!(irq._int, 9);
}
