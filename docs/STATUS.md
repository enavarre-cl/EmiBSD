# Status

Milestone: **M7a in progress** (part 0, the rwlock, part 1, the uvm object layer, and part 2,
`uvm_map.c` and `uvm_addr.c`, done; part 3, `uvm_fault.c`, next). Updated: 2026-10-03.

Done:
- M7a-2: `uvm_addr.c` (the rnd, kbootstrap, bestfit, pivot and stack/brk selectors) and
  `uvm_map.c` proper: the augmented entry tree, `uvm_map`/`uvm_mapanon`/`uvm_unmap`, the
  clippers, the free lists, `uvm_map_protect`/`inherit`/`immutable`/`advice`/`pageable`/
  `extract`/`clean`/`submap`/`mquery`, the fork copy functions (`uvmspace_fork` is real),
  `uvmspace_exec` through `uvm_unmap_remove`, `uvm_map_pie`, the static kernel entries and
  the entry pools, `VMMAP_DEBUG` as feature `vmmap_debug` (on in host tests); `uvm_km_init`
  sets up `kernel_map` with the bootstrap reservation, `uvm_km_suballoc`,
  `uvm_km_pgremove(_intrsafe)`, the kernel map's bestfit switch in `uvm_init`;
  `pmap_protect`/`pmap_wired_count` in the `machine::Pmap` trait (amd64/arm64 report
  `pmap_protect` until the pv lists). amd64's kernel window is now consistent: it starts
  at `virtual_avail` above Limine's direct map and keeps the C's 512 GiB size. Exec still
  uses the wired stand-ins (M7a-3 retires them); `uvm_fault_wire`/`unwire_locked` are
  reported stubs; `uvm_map_inentry` reports `trapsignal`; `uvm_map_protect` reports the
  `RLIMIT_DATA` check.
- M7a-1: the object layer: `uvm_anon.c` (the anon pool, `uvm_analloc`/`uvm_anfree`/
  `uvm_anon_release`/`uvm_anon_dropswap`), `uvm_amap.c` (chunks, buckets, ppref, `amap_copy`,
  `amap_cow_now`, `amap_ref`/`amap_unref`, `amap_swap_off`; one chunk pool instead of the
  sixteen small-amap pools), `uvm_aobj.c` (`uao_create` with the kernel object, `uao_get`,
  `uao_flush`, `uao_detach`, the swap-slot array and hash), `uvm_pager.h` (`struct
  uvm_pagerops`, `PGO_*`, `VM_PAGER_*`) and `uvm_pager_init`, `uvm_object.c` complete
  (`uvm_obj_init/destroy/setlock/wire/unwire/free`), `uvm_fault.h`'s `struct uvm_faultinfo`,
  `struct vm_map_entry` with the `uvm_map_addr` tree and `uvm_map_deadq`, `uvm.kernel_object`
  (created in `uvm_km_init`), `uvm_pagewait` sleeping on the owner lock,
  `uvm_page_owner_locked_p` checking it, `pmap_page_protect`/`pmap_clear_modify` in the
  `machine::Pmap` trait (amd64/arm64 report them until the pv lists). Swap stays reported
  (`uvm_swap_*`, M7); `amap_copy`'s chunking waits for `uvm_map_clip_*` (M7a-2).
- M7a-0: `kern_rwlock.c`; the vm_map lock is an rwlock.
- M6 closed on 2026-10-03 (`494a597`): init runs in user mode on wired mappings on both archs.

Next:
- M7a-3: `uvm_fault.c`, amd64 pv lists (`pmap_enter_pv`, `pmap_page_remove`,
  `pmap_page_protect`, `pmap_protect`, the R/M bits), arm64 `pmap_fault_fixup`, exec through
  `uvm_map` (the wired stand-ins retire), `uvm_mmap.c`; exit criterion: a user page fault
  served by `uvm_fault` on both archs.
- After M7: `docs/ROADMAP.md` carries, as proposals of 2026-10-03, the "What 'complete'
  means" scope section and the rows M9 security, M10 file systems, M11 SMP, M12 devices and
  vmm, M13 storage/firmware/console (everything QEMU 11.1 emulates: ahci, nvme, scsi, ACPI,
  RTC, efifb/wscons, em/re), M14 installable (was M9), M15 real hardware (was M10, only what
  QEMU does not emulate). Eight decisions are the user's before M8 starts: the M7+ order (vfs
  first, or the network as M7b with the ping criterion), widening the reference clone, C or
  Rust userland, Limine or `boot(8)`, the scope section, the new order, the vmm test rig (SVM
  under TCG on the Apple M3 Pro, or the reduced criterion) and the M15 machines.

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's, for M8/M9/M10, proposed 2026-10-03 in `docs/ROADMAP.md`):
widening the reference clone beyond `sys/`; userland as cross-compiled OpenBSD C or a Rust
port; Limine versus `boot(8)`/`efiboot` for the installable system; for real hardware, the
one arm64 board, whether networking is in, and a named reference PC.
