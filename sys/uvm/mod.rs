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
/* </LICENSES> */

/* <CODE> */
//! Virtual memory: OpenBSD `sys/uvm/*.c`.
//!
//! Physical page management, kernel memory, maps, faults, the pager interface. Headers become
//! modules too (`uvm_page.h` → `uvm_page.rs`, the same file as `uvm_page.c`).

#[allow(clippy::module_inception)] // OpenBSD's name: uvm/uvm.h
pub mod uvm;
pub mod uvm_addr;
pub mod uvm_amap;
pub mod uvm_anon;
pub mod uvm_aobj;
pub mod uvm_extern;
pub mod uvm_fault;
pub mod uvm_glue;
pub mod uvm_init;
pub mod uvm_io;
pub mod uvm_km;
pub mod uvm_map;
pub mod uvm_meter;
pub mod uvm_mmap;
pub mod uvm_object;
pub mod uvm_page;
pub mod uvm_pager;
pub mod uvm_param;
pub mod uvm_pdaemon;
pub mod uvm_percpu;
pub mod uvm_pmap;
pub mod uvm_pmemrange;
pub mod uvm_swap;
pub mod uvm_unix;
pub mod uvm_vnode;
pub mod uvmexp;
/* </CODE> */
