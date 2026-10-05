//! OpenBSD `sys/dev/puc/`: the port drivers of `puc(4)`, the PCI "universal" communication card
//! driver (`dev/pci/puc.c`). Only `com(4)`'s attachment (`com_puc.c`) is here; `lpt_puc.c` waits
//! for `lpt(4)`.

pub mod com_puc;
