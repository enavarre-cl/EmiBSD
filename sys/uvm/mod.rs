//! Virtual memory: OpenBSD `sys/uvm/*.c`.
//!
//! Physical page management, kernel memory, maps, faults, the pager interface. Headers become
//! modules too (`uvm_page.h` → `uvm_page.rs`, the same file as `uvm_page.c`).

#[allow(clippy::module_inception)] // OpenBSD's name: uvm/uvm.h
pub mod uvm;
pub mod uvm_anon;
pub mod uvm_extern;
pub mod uvm_glue;
pub mod uvm_init;
pub mod uvm_km;
pub mod uvm_object;
pub mod uvm_page;
pub mod uvm_param;
pub mod uvm_pdaemon;
pub mod uvm_pmap;
pub mod uvm_pmemrange;
pub mod uvmexp;
