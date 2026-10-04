use super::*;

extern crate std;
use std::boxed::Box;
use std::string::ToString;

use crate::dev::softraid::SR_META_BYTES;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::malloc::M_WAITOK;

/// A discipline with in-memory metadata and chunks of the given sizes (in blocks).
fn volume(sizes: &[i64]) -> &'static SrDiscipline {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
    let sd = sr_malloc::<SrDiscipline>(M_WAITOK).unwrap();
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline = unsafe { sd.as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd.sd_meta().ssdi().ssd_chunk_no.set(sizes.len() as u32);
    sd.sd_vol.sv_chunks_alloc(sizes.len(), M_WAITOK).unwrap();
    for (i, &size) in sizes.iter().enumerate() {
        // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_size.set(size);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }
    sd
}

#[test]
fn locate_walks_the_chunks() {
    let sizes = [10i64, 20, 5];
    let at = |lba: i64| concat_locate(sizes.iter().copied(), lba);
    // chunk ends in bytes: 5120, 15360, 17920
    assert_eq!(at(0), Some((0, 5120, 0)));
    assert_eq!(at(5119), Some((0, 5120, 5119)));
    // the first byte of the second chunk is its offset 0
    assert_eq!(at(5120), Some((1, 15360, 0)));
    assert_eq!(at(15359), Some((1, 15360, 10239)));
    assert_eq!(at(15360), Some((2, 17920, 0)));
    assert_eq!(at(17919), Some((2, 17920, 2559)));
    // at the end of the volume, and beyond it, there is no chunk
    assert_eq!(at(17920), None);
    assert_eq!(at(1 << 40), None);
    assert_eq!(concat_locate(core::iter::empty(), 0), None);
}

#[test]
fn create_sums_the_chunks() {
    let _g = setup_real_memory();
    let sd = volume(&[1000, 2500, 7]);
    // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
    let mut bc: BiocCreateraid = unsafe { core::mem::zeroed() };
    sr_concat_create(sd, &mut bc, 3, 7).unwrap();
    assert_eq!(sd.sd_meta().ssdi().ssd_size.get(), 3507);
    assert_eq!(sd.sd_max_ccb_per_wu.get(), SR_CONCAT_NOWU * 3);

    // assembly only sets the runtime values
    let sd = volume(&[1, 1]);
    sr_concat_assemble(sd, &mut bc, 2, None).unwrap();
    assert_eq!(sd.sd_max_ccb_per_wu.get(), SR_CONCAT_NOWU * 2);
}

#[test]
fn discipline_init_installs_hooks() {
    let _g = setup_real_memory();
    let sd = volume(&[1]);
    sr_concat_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_CONCAT);
    assert_eq!(sd.name().to_string(), "CONCAT");
    assert_eq!(
        sd.sd_capabilities.get(),
        SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_NON_COERCED
    );
    assert_eq!(sd.sd_max_wu.get(), SR_CONCAT_NOWU);
    assert!(sd.sd_create.get().is_some());
    assert!(sd.sd_assemble.get().is_some());
    assert!(sd.sd_scsi_rw.get().is_some());
}
