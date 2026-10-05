//! Host tests for `siop_common.rs`: the negotiation messages, the synchronous period range,
//! the save-data-pointer and residual arithmetic, and a target-initiated SDTR, on a command
//! whose tables live in host memory (the host's `bus_space` reads 0 and drops writes).

use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::{assert_eq, vec};

use super::*;
use crate::dev::ic::siopreg::{SCF_PERIOD, ScfPeriod};
use crate::dev::ic::siopvar_common::{SiopCommonTarget, TARF_ISWIDE};
use crate::machine::bus::{BusSpaceTag, bus_space_map};
use crate::scsi::scsi_message::MSG_EXT_SDTR_LEN;
use crate::scsi::scsiconf::{ScsiLink, ScsiXfer};

/// A zeroed `T` (an `M_ZERO` allocation), leaked.
///
/// # Safety
///
/// All-zero must be a valid `T`.
unsafe fn leak_zeroed<T>() -> &'static T {
    // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
    unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
}

/// An adapter with the 53c895A's clock and offset, its registers "mapped", and a command
/// for target 0 lun 0 whose transfer has `flags`.
fn command(flags: i32) -> &'static SiopCommonCmd {
    // SAFETY: the softc, the target, the tables and the command are `Cell`s of integers,
    // pointers, `Option`s and bus tags, and a `Device`: all valid as zero bytes.
    let (sc, target, tables, cmd) = unsafe {
        (
            leak_zeroed::<SiopCommonSoftc>(),
            leak_zeroed::<SiopCommonTarget>(),
            leak_zeroed::<SiopCommonXfer>(),
            leak_zeroed::<SiopCommonCmd>(),
        )
    };
    sc.clock_period.set(62);
    sc.maxoff.set(31);
    let (min, max) = sync_range(&SCF_PERIOD, 62);
    sc.st_minsync.set(min);
    sc.st_maxsync.set(max);
    sc.sc_rt.set(Some(BusSpaceTag::default()));
    // SAFETY: the host's bus space is a test double; nothing is mapped.
    sc.sc_rh.set(Some(
        unsafe { bus_space_map(BusSpaceTag::default(), 0, 0x100, 0) }.unwrap(),
    ));
    sc.targets[0].set(Some(core::ptr::NonNull::from(target)));

    let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    xs.sc_link.set(Some(link));
    xs.flags.set(flags);

    cmd.siop_sc.set(Some(sc));
    cmd.siop_target.set(sc.targets[0].get());
    cmd.xs.set(Some(xs));
    cmd.siop_tables.set(Some(tables));
    cmd
}

fn msg_out(cmd: &SiopCommonCmd, n: usize) -> std::vec::Vec<u8> {
    (0..n).map(|i| cmd.tables().msg_out(i)).collect()
}

#[test]
fn negotiation_messages() {
    let cmd = command(0);
    siop_sdtr_msg(cmd, 1, 10, 31);
    assert_eq!(
        msg_out(cmd, 6)[1..],
        [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 10, 31]
    );
    assert_eq!(dma_get(&cmd.tables().t_msgout).count, 6);

    siop_wdtr_msg(cmd, 0, 1);
    assert_eq!(
        msg_out(cmd, 4),
        [MSG_EXTENDED, MSG_EXT_WDTR_LEN, MSG_EXT_WDTR, 1]
    );
    assert_eq!(dma_get(&cmd.tables().t_msgout).count, 4);

    siop_ppr_msg(cmd, 0, 9, 62);
    assert_eq!(
        msg_out(cmd, 8),
        [
            MSG_EXTENDED,
            MSG_EXT_PPR_LEN,
            MSG_EXT_PPR,
            9,
            0,
            62,
            1,
            MSG_EXT_PPR_PROT_DT
        ]
    );
    assert_eq!(dma_get(&cmd.tables().t_msgout).count, 8);
}

#[test]
fn sync_range_per_clock() {
    assert_eq!(sync_range(&SCF_PERIOD, 62), (10, 25));
    assert_eq!(sync_range(&DT_SCF_PERIOD, 62), (9, 25));
    assert_eq!(sync_range(&SCF_PERIOD, 250), (25, 75));
    assert_eq!(sync_range(&SCF_PERIOD, 125), (12, 50));
    let none: [ScfPeriod; 0] = [];
    assert_eq!(sync_range(&none, 62), (255, 0));
    assert_eq!(siop_period_mhz(10), "40.0");
    assert_eq!(siop_period_mhz(11), "??");
}

#[test]
fn sdp_cuts_the_partial_table_and_moves_the_rest_down() {
    let cmd = command(SCSI_DATA_IN);
    let t = cmd.tables();
    let rows = [(4096, 0x10_0000), (4096, 0x20_0000), (2048, 0x30_0000)];
    for (i, &(count, addr)) in rows.iter().enumerate() {
        dma_set(&t.data[i], ScrTable { count, addr });
    }
    cmd.xs().resid.set(10240);
    // A phase mismatch left 1000 bytes of the second table untransferred.
    cmd.flags.set(CMDFL_RESID);
    cmd.resid.set(1000);

    siop_sdp(cmd, 1);

    assert_eq!(cmd.xs().resid.get(), 10240 - 4096 - 3096);
    assert_eq!(cmd.flags.get() & CMDFL_RESID, 0);
    assert_eq!(
        dma_get(&t.data[0]),
        ScrTable {
            count: 1000,
            addr: 0x20_0000 + 4096 - 1000
        }
    );
    assert_eq!(
        dma_get(&t.data[1]),
        ScrTable {
            count: 2048,
            addr: 0x30_0000
        }
    );
    assert_eq!(dma_get(&t.data[2]), ScrTable::default());

    // Without data, nothing moves; at SIOP_NSG, neither.
    let cmd = command(0);
    dma_set(&cmd.tables().data[0], ScrTable { count: 5, addr: 6 });
    siop_sdp(cmd, 0);
    assert_eq!(dma_get(&cmd.tables().data[0]).count, 5);
    let cmd = command(SCSI_DATA_OUT);
    dma_set(&cmd.tables().data[0], ScrTable { count: 5, addr: 6 });
    siop_sdp(cmd, SIOP_NSG);
    assert_eq!(dma_get(&cmd.tables().data[0]).count, 5);
}

#[test]
fn update_resid_counts_whole_tables() {
    let cmd = command(SCSI_DATA_OUT);
    for d in cmd.tables().data.iter().take(3) {
        dma_set(
            d,
            ScrTable {
                count: 512,
                addr: 0,
            },
        );
    }
    cmd.xs().resid.set(1536);
    siop_update_resid(cmd, 2);
    assert_eq!(cmd.xs().resid.get(), 512);
    siop_update_resid(cmd, 1);
    assert_eq!(cmd.xs().resid.get(), 0);
}

#[test]
fn target_initiated_sdtr_is_answered_at_the_closest_period() {
    let cmd = command(0);
    let sc = cmd.sc();
    let t = cmd.target();
    t.status.set(TARST_OK);
    t.id.set(0x0300_0000); // clock_div 3 in SCNTL3
    let msg_in = [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 25, 40];
    for (i, &b) in msg_in.iter().enumerate() {
        dma_set(&cmd.tables().msg_in[i], b);
    }

    assert_eq!(siop_sdtr_neg(cmd), SIOP_NEG_MSGOUT);
    // Period 25 at clock 62 is SCF 5; the offset is cut to 31; not Ultra (period >= 25).
    assert_eq!((t.period.get(), t.offset.get()), (25, 31));
    let id = sc.target(0).id.get();
    assert_eq!(
        (id >> 24) & u32::from(SCNTL3_SCF_MASK),
        5 << SCNTL3_SCF_SHIFT
    );
    assert_eq!((id >> 8) & u32::from(SXFER_MO_MASK), 31);
    assert_eq!(dma_get(&cmd.tables().id), id);
    // The answer: the SDTR with the period and offset we can do.
    assert_eq!(
        msg_out(cmd, 5),
        [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 25, 31]
    );

    // A wide answer to a target that cannot be wide is rejected by siop_iwr.
    t.flags.set(t.flags.get() & !TARF_ISWIDE);
    assert_eq!(siop_iwr(cmd), SIOP_NEG_MSGOUT);
    assert_eq!(msg_out(cmd, 1), vec![MSG_MESSAGE_REJECT]);
}
