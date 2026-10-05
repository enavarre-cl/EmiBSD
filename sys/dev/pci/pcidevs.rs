/*
 * THIS FILE AUTOMATICALLY GENERATED.  DO NOT EDIT.
 *
 * generated from:
 *	OpenBSD: pcidevs,v 1.2147 2026/08/14 03:32:01 jsg Exp
 */
/*	$NetBSD: pcidevs,v 1.30 1997/06/24 06:20:24 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Christopher G. Demetriou
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<dev/pci/pcidevs.h>`: PCI vendor and product IDs, the subset the ported code names.
//!
//! Upstream: sys/dev/pci/pcidevs.h @ 3ce1f3f79392
//!
//! OpenBSD generates this header (and `pcidevs_data.h`, the names `PCIVERBOSE` prints) from
//! `sys/dev/pci/pcidevs` with `devlist2h.awk`: about 350 vendors and 9600 products, 640 KB
//! of defines and 800 KB of name tables. Here the IDs are added by hand as the code that uses
//! them is ported, with the generated names and values.
//!
//! ## Deviations
//! - Partial: only the IDs some ported file names are present (`pci.c`'s `pci_set_powerstate`,
//!   `pci_quirks.c`, `virtio_pci.c`, `nvme_pci.c`, `ahci_pci.c`, `xhci_pci.c`, `auich.c`,
//!   `azalia.c`, `azalia_codec.c` and `siop_pci_common.c`). The whole header, and `pcidevs_data.h` for
//!   `PCIVERBOSE`, wait for a generator in `tools/xtask` in the manner of `gen-syscalls`
//!   (`docs/ARCHITECTURE.md`).
//! - The IDs are `u32`, the type `pci_vendor`/`pci_product` return.

/// `PCI_VENDOR_OPENBSD`: OpenBSD.
pub const PCI_VENDOR_OPENBSD: u32 = 0x0b5d;
/// `PCI_VENDOR_CIRRUS`: Cirrus Logic.
pub const PCI_VENDOR_CIRRUS: u32 = 0x1013;
/// `PCI_VENDOR_AMD`: AMD.
pub const PCI_VENDOR_AMD: u32 = 0x1022;
/// `PCI_VENDOR_APPLE`: Apple.
pub const PCI_VENDOR_APPLE: u32 = 0x106b;
/// `PCI_VENDOR_SIS`: SiS.
pub const PCI_VENDOR_SIS: u32 = 0x1039;
/// `PCI_VENDOR_NVIDIA`: NVIDIA.
pub const PCI_VENDOR_NVIDIA: u32 = 0x10de;
/// `PCI_VENDOR_QUMRANET`: Qumranet.
pub const PCI_VENDOR_QUMRANET: u32 = 0x1af4;
/// `PCI_VENDOR_FRESCO`: Fresco Logic.
pub const PCI_VENDOR_FRESCO: u32 = 0x1b73;
/// `PCI_VENDOR_INTEL`: Intel.
pub const PCI_VENDOR_INTEL: u32 = 0x8086;
/// `PCI_VENDOR_INVALID`: INVALID VENDOR ID.
pub const PCI_VENDOR_INVALID: u32 = 0xffff;

/// `PCI_PRODUCT_AMD_17_1X_XHCI_1`: 17h/1xh xHCI.
pub const PCI_PRODUCT_AMD_17_1X_XHCI_1: u32 = 0x15e0;
/// `PCI_PRODUCT_AMD_17_1X_XHCI_2`: 17h/1xh xHCI.
pub const PCI_PRODUCT_AMD_17_1X_XHCI_2: u32 = 0x15e1;
/// `PCI_PRODUCT_AMD_17_6X_XHCI`: 17h/6xh xHCI.
pub const PCI_PRODUCT_AMD_17_6X_XHCI: u32 = 0x1639;

/// `PCI_PRODUCT_APPLE_NVME1`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME1: u32 = 0x2001;
/// `PCI_PRODUCT_APPLE_NVME2`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME2: u32 = 0x2003;
/// `PCI_PRODUCT_APPLE_NVME3`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME3: u32 = 0x2005;

/// `PCI_PRODUCT_CIRRUS_CL_PD6729`: CL-PD6729.
pub const PCI_PRODUCT_CIRRUS_CL_PD6729: u32 = 0x1100;

/// `PCI_PRODUCT_FRESCO_FL1000`: FL1000 xHCI.
pub const PCI_PRODUCT_FRESCO_FL1000: u32 = 0x1000;
/// `PCI_PRODUCT_FRESCO_FL1400`: FL1400 xHCI.
pub const PCI_PRODUCT_FRESCO_FL1400: u32 = 0x1400;

/// `PCI_PRODUCT_INTEL_82371FB_ISA`: 82371FB ISA.
pub const PCI_PRODUCT_INTEL_82371FB_ISA: u32 = 0x122e;

/// `PCI_PRODUCT_OPENBSD_CONTROL`: VMM Control.
pub const PCI_PRODUCT_OPENBSD_CONTROL: u32 = 0x0777;

// auich(4)'s controllers.
/// `PCI_PRODUCT_AMD_PBC768_ACA`: 768 AC97.
pub const PCI_PRODUCT_AMD_PBC768_ACA: u32 = 0x7445;
/// `PCI_PRODUCT_AMD_8111_ACA`: 8111 AC97.
pub const PCI_PRODUCT_AMD_8111_ACA: u32 = 0x746d;
/// `PCI_PRODUCT_INTEL_82801AA_ACA`: 82801AA AC97.
pub const PCI_PRODUCT_INTEL_82801AA_ACA: u32 = 0x2415;
/// `PCI_PRODUCT_INTEL_82801AB_ACA`: 82801AB AC97.
pub const PCI_PRODUCT_INTEL_82801AB_ACA: u32 = 0x2425;
/// `PCI_PRODUCT_INTEL_82801BA_ACA`: 82801BA AC97.
pub const PCI_PRODUCT_INTEL_82801BA_ACA: u32 = 0x2445;
/// `PCI_PRODUCT_INTEL_82801CA_ACA`: 82801CA/CAM AC97.
pub const PCI_PRODUCT_INTEL_82801CA_ACA: u32 = 0x2485;
/// `PCI_PRODUCT_INTEL_82801DB_ACA`: 82801DB AC97.
pub const PCI_PRODUCT_INTEL_82801DB_ACA: u32 = 0x24c5;
/// `PCI_PRODUCT_INTEL_82801EB_ACA`: 82801EB/ER AC97.
pub const PCI_PRODUCT_INTEL_82801EB_ACA: u32 = 0x24d5;
/// `PCI_PRODUCT_INTEL_6300ESB_ACA`: 6300ESB AC97.
pub const PCI_PRODUCT_INTEL_6300ESB_ACA: u32 = 0x25a6;
/// `PCI_PRODUCT_INTEL_82801FB_ACA`: 82801FB AC97.
pub const PCI_PRODUCT_INTEL_82801FB_ACA: u32 = 0x266e;
/// `PCI_PRODUCT_INTEL_6321ESB_ACA`: 6321ESB AC97.
pub const PCI_PRODUCT_INTEL_6321ESB_ACA: u32 = 0x2698;
/// `PCI_PRODUCT_INTEL_82801GB_ACA`: 82801GB AC97.
pub const PCI_PRODUCT_INTEL_82801GB_ACA: u32 = 0x27de;
/// `PCI_PRODUCT_INTEL_82440MX_ACA`: 82440MX AC97.
pub const PCI_PRODUCT_INTEL_82440MX_ACA: u32 = 0x7195;
/// `PCI_PRODUCT_NVIDIA_MCP04_AC97`: MCP04 AC97.
pub const PCI_PRODUCT_NVIDIA_MCP04_AC97: u32 = 0x003a;
/// `PCI_PRODUCT_NVIDIA_NFORCE4_AC`: nForce4 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE4_AC: u32 = 0x0059;
/// `PCI_PRODUCT_NVIDIA_NFORCE2_ACA`: nForce2 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE2_ACA: u32 = 0x006a;
/// `PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA`: nForce2 400 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA: u32 = 0x008a;
/// `PCI_PRODUCT_NVIDIA_NFORCE3_ACA`: nForce3 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE3_ACA: u32 = 0x00da;
/// `PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA`: nForce3 250 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA: u32 = 0x00ea;
/// `PCI_PRODUCT_NVIDIA_NFORCE_ACA`: nForce AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE_ACA: u32 = 0x01b1;
/// `PCI_PRODUCT_NVIDIA_MCP51_ACA`: MCP51 AC97.
pub const PCI_PRODUCT_NVIDIA_MCP51_ACA: u32 = 0x026b;
/// `PCI_PRODUCT_SIS_7012_ACA`: 7012 AC97.
pub const PCI_PRODUCT_SIS_7012_ACA: u32 = 0x7012;

// azalia(4): the controllers `azalia.c` names, and the codec subsystem vendors
// `azalia_codec.c` checks.

/// `PCI_VENDOR_DELL`: Dell.
pub const PCI_VENDOR_DELL: u32 = 0x1028;
/// `PCI_VENDOR_HP`: Hewlett-Packard.
pub const PCI_VENDOR_HP: u32 = 0x103c;
/// `PCI_PRODUCT_AMD_15_6X_AUDIO`: 15h HD Audio.
pub const PCI_PRODUCT_AMD_15_6X_AUDIO: u32 = 0x157a;
/// `PCI_PRODUCT_AMD_17_1X_HDA`: 17h/1xh HD Audio.
pub const PCI_PRODUCT_AMD_17_1X_HDA: u32 = 0x15e3;
/// `PCI_PRODUCT_AMD_17_3X_HDA`: 17h HD Audio.
pub const PCI_PRODUCT_AMD_17_3X_HDA: u32 = 0x1487;
/// `PCI_PRODUCT_AMD_17_HDA`: 17h HD Audio.
pub const PCI_PRODUCT_AMD_17_HDA: u32 = 0x1457;
/// `PCI_PRODUCT_AMD_HUDSON2_HDA`: Hudson-2 HD Audio.
pub const PCI_PRODUCT_AMD_HUDSON2_HDA: u32 = 0x780d;
/// `PCI_PRODUCT_ATI_SB450_HDA`: SB450 HD Audio.
pub const PCI_PRODUCT_ATI_SB450_HDA: u32 = 0x437b;
/// `PCI_PRODUCT_ATI_SBX00_HDA`: SBx00 HD Audio.
pub const PCI_PRODUCT_ATI_SBX00_HDA: u32 = 0x4383;
/// `PCI_PRODUCT_INTEL_100SERIES_HDA`: 100 Series HD Audio.
pub const PCI_PRODUCT_INTEL_100SERIES_HDA: u32 = 0xa170;
/// `PCI_PRODUCT_INTEL_100SERIES_H_HDA`: 100 Series HD Audio.
pub const PCI_PRODUCT_INTEL_100SERIES_H_HDA: u32 = 0xa171;
/// `PCI_PRODUCT_INTEL_100SERIES_LP_HDA`: 100 Series HD Audio.
pub const PCI_PRODUCT_INTEL_100SERIES_LP_HDA: u32 = 0x9d70;
/// `PCI_PRODUCT_INTEL_200SERIES_HDA`: 200 Series HD Audio.
pub const PCI_PRODUCT_INTEL_200SERIES_HDA: u32 = 0xa2f0;
/// `PCI_PRODUCT_INTEL_200SERIES_U_HDA`: 200 Series HD Audio.
pub const PCI_PRODUCT_INTEL_200SERIES_U_HDA: u32 = 0x9d71;
/// `PCI_PRODUCT_INTEL_300SERIES_CAVS`: 300 Series cAVS.
pub const PCI_PRODUCT_INTEL_300SERIES_CAVS: u32 = 0xa348;
/// `PCI_PRODUCT_INTEL_300SERIES_U_HDA`: 300 Series HD Audio.
pub const PCI_PRODUCT_INTEL_300SERIES_U_HDA: u32 = 0x9dc8;
/// `PCI_PRODUCT_INTEL_3400_HDA`: 3400 HD Audio.
pub const PCI_PRODUCT_INTEL_3400_HDA: u32 = 0x3b56;
/// `PCI_PRODUCT_INTEL_400SERIES_CAVS`: 400 Series cAVS.
pub const PCI_PRODUCT_INTEL_400SERIES_CAVS: u32 = 0x06c8;
/// `PCI_PRODUCT_INTEL_400SERIES_LP_HDA`: 400 Series HD Audio.
pub const PCI_PRODUCT_INTEL_400SERIES_LP_HDA: u32 = 0x02c8;
/// `PCI_PRODUCT_INTEL_495SERIES_LP_HDA`: 495 Series HD Audio.
pub const PCI_PRODUCT_INTEL_495SERIES_LP_HDA: u32 = 0x34c8;
/// `PCI_PRODUCT_INTEL_500SERIES_HDA`: 500 Series HD Audio.
pub const PCI_PRODUCT_INTEL_500SERIES_HDA: u32 = 0x43c8;
/// `PCI_PRODUCT_INTEL_500SERIES_HDA_2`: 500 Series HD Audio.
pub const PCI_PRODUCT_INTEL_500SERIES_HDA_2: u32 = 0xf0c8;
/// `PCI_PRODUCT_INTEL_500SERIES_LP_HDA`: 500 Series HD Audio.
pub const PCI_PRODUCT_INTEL_500SERIES_LP_HDA: u32 = 0xa0c8;
/// `PCI_PRODUCT_INTEL_600SERIES_HDA`: 600 Series HD Audio.
pub const PCI_PRODUCT_INTEL_600SERIES_HDA: u32 = 0x7ad0;
/// `PCI_PRODUCT_INTEL_600SERIES_LP_HDA`: 600 Series HD Audio.
pub const PCI_PRODUCT_INTEL_600SERIES_LP_HDA: u32 = 0x51c8;
/// `PCI_PRODUCT_INTEL_6321ESB_HDA`: 6321ESB HD Audio.
pub const PCI_PRODUCT_INTEL_6321ESB_HDA: u32 = 0x269a;
/// `PCI_PRODUCT_INTEL_6SERIES_HDA`: 6 Series HD Audio.
pub const PCI_PRODUCT_INTEL_6SERIES_HDA: u32 = 0x1c20;
/// `PCI_PRODUCT_INTEL_700SERIES_HDA`: 700 Series HD Audio.
pub const PCI_PRODUCT_INTEL_700SERIES_HDA: u32 = 0x7a50;
/// `PCI_PRODUCT_INTEL_700SERIES_LP_HDA`: 700 Series HD Audio.
pub const PCI_PRODUCT_INTEL_700SERIES_LP_HDA: u32 = 0x51ca;
/// `PCI_PRODUCT_INTEL_7SERIES_HDA`: 7 Series HD Audio.
pub const PCI_PRODUCT_INTEL_7SERIES_HDA: u32 = 0x1e20;
/// `PCI_PRODUCT_INTEL_800SERIES_HDA`: 800 Series HD Audio.
pub const PCI_PRODUCT_INTEL_800SERIES_HDA: u32 = 0x7f50;
/// `PCI_PRODUCT_INTEL_82801FB_HDA`: 82801FB HD Audio.
pub const PCI_PRODUCT_INTEL_82801FB_HDA: u32 = 0x2668;
/// `PCI_PRODUCT_INTEL_82801GB_HDA`: 82801GB HD Audio.
pub const PCI_PRODUCT_INTEL_82801GB_HDA: u32 = 0x27d8;
/// `PCI_PRODUCT_INTEL_82801H_HDA`: 82801H HD Audio.
pub const PCI_PRODUCT_INTEL_82801H_HDA: u32 = 0x284b;
/// `PCI_PRODUCT_INTEL_82801I_HDA`: 82801I HD Audio.
pub const PCI_PRODUCT_INTEL_82801I_HDA: u32 = 0x293e;
/// `PCI_PRODUCT_INTEL_82801JD_HDA`: 82801JD HD Audio.
pub const PCI_PRODUCT_INTEL_82801JD_HDA: u32 = 0x3a6e;
/// `PCI_PRODUCT_INTEL_82801JI_HDA`: 82801JI HD Audio.
pub const PCI_PRODUCT_INTEL_82801JI_HDA: u32 = 0x3a3e;
/// `PCI_PRODUCT_INTEL_8SERIES_HDA`: 8 Series HD Audio.
pub const PCI_PRODUCT_INTEL_8SERIES_HDA: u32 = 0x8c20;
/// `PCI_PRODUCT_INTEL_8SERIES_LP_HDA`: 8 Series HD Audio.
pub const PCI_PRODUCT_INTEL_8SERIES_LP_HDA: u32 = 0x9c20;
/// `PCI_PRODUCT_INTEL_9SERIES_HDA`: 9 Series HD Audio.
pub const PCI_PRODUCT_INTEL_9SERIES_HDA: u32 = 0x8ca0;
/// `PCI_PRODUCT_INTEL_9SERIES_LP_HDA`: 9 Series HD Audio.
pub const PCI_PRODUCT_INTEL_9SERIES_LP_HDA: u32 = 0x9ca0;
/// `PCI_PRODUCT_INTEL_ADL_N_HDA`: ADL-N HD Audio.
pub const PCI_PRODUCT_INTEL_ADL_N_HDA: u32 = 0x54c8;
/// `PCI_PRODUCT_INTEL_APOLLOLAKE_HDA`: Apollo Lake HD Audio.
pub const PCI_PRODUCT_INTEL_APOLLOLAKE_HDA: u32 = 0x5a98;
/// `PCI_PRODUCT_INTEL_ARL_U_HDA`: Core Ultra HD Audio.
pub const PCI_PRODUCT_INTEL_ARL_U_HDA: u32 = 0x7728;
/// `PCI_PRODUCT_INTEL_BAYTRAIL_HDA`: Bay Trail HD Audio.
pub const PCI_PRODUCT_INTEL_BAYTRAIL_HDA: u32 = 0x0f04;
/// `PCI_PRODUCT_INTEL_BSW_HDA`: Braswell HD Audio.
pub const PCI_PRODUCT_INTEL_BSW_HDA: u32 = 0x2284;
/// `PCI_PRODUCT_INTEL_C600_HDA`: C600 HD Audio.
pub const PCI_PRODUCT_INTEL_C600_HDA: u32 = 0x1d20;
/// `PCI_PRODUCT_INTEL_C610_HDA_1`: C610 HD Audio.
pub const PCI_PRODUCT_INTEL_C610_HDA_1: u32 = 0x8d20;
/// `PCI_PRODUCT_INTEL_C610_HDA_2`: C610 HD Audio.
pub const PCI_PRODUCT_INTEL_C610_HDA_2: u32 = 0x8d21;
/// `PCI_PRODUCT_INTEL_C620_HDA_1`: C620 HD Audio.
pub const PCI_PRODUCT_INTEL_C620_HDA_1: u32 = 0xa1f0;
/// `PCI_PRODUCT_INTEL_C620_HDA_2`: C620 HD Audio.
pub const PCI_PRODUCT_INTEL_C620_HDA_2: u32 = 0xa270;
/// `PCI_PRODUCT_INTEL_EHL_HDA`: Elkhart Lake HD Audio.
pub const PCI_PRODUCT_INTEL_EHL_HDA: u32 = 0x4b58;
/// `PCI_PRODUCT_INTEL_GLK_HDA`: Gemini Lake HD Audio.
pub const PCI_PRODUCT_INTEL_GLK_HDA: u32 = 0x3198;
/// `PCI_PRODUCT_INTEL_JSL_HDA`: Jasper Lake HD Audio.
pub const PCI_PRODUCT_INTEL_JSL_HDA: u32 = 0x4dc8;
/// `PCI_PRODUCT_INTEL_LNL_HDA`: Core Ultra HD Audio.
pub const PCI_PRODUCT_INTEL_LNL_HDA: u32 = 0xa828;
/// `PCI_PRODUCT_INTEL_MTL_HDA`: Core Ultra HD Audio.
pub const PCI_PRODUCT_INTEL_MTL_HDA: u32 = 0x7e28;
/// `PCI_PRODUCT_INTEL_PTL_HDA`: Core Ultra HD Audio.
pub const PCI_PRODUCT_INTEL_PTL_HDA: u32 = 0xe428;
/// `PCI_PRODUCT_INTEL_PTL_H_HDA`: Core Ultra HD Audio.
pub const PCI_PRODUCT_INTEL_PTL_H_HDA: u32 = 0xe328;
/// `PCI_PRODUCT_INTEL_QS57_HDA`: QS57 HD Audio.
pub const PCI_PRODUCT_INTEL_QS57_HDA: u32 = 0x3b57;
/// `PCI_PRODUCT_NVIDIA_MCP51_HDA`: MCP51 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP51_HDA: u32 = 0x026c;
/// `PCI_PRODUCT_NVIDIA_MCP55_HDA`: MCP55 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP55_HDA: u32 = 0x0371;
/// `PCI_PRODUCT_NVIDIA_MCP61_HDA_1`: MCP61 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP61_HDA_1: u32 = 0x03e4;
/// `PCI_PRODUCT_NVIDIA_MCP61_HDA_2`: MCP61 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP61_HDA_2: u32 = 0x03f0;
/// `PCI_PRODUCT_NVIDIA_MCP65_HDA_1`: MCP65 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP65_HDA_1: u32 = 0x044a;
/// `PCI_PRODUCT_NVIDIA_MCP65_HDA_2`: MCP65 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP65_HDA_2: u32 = 0x044b;
/// `PCI_PRODUCT_NVIDIA_MCP67_HDA_1`: MCP67 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP67_HDA_1: u32 = 0x055c;
/// `PCI_PRODUCT_NVIDIA_MCP67_HDA_2`: MCP67 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP67_HDA_2: u32 = 0x055d;
/// `PCI_PRODUCT_NVIDIA_MCP73_HDA_1`: MCP73 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP73_HDA_1: u32 = 0x07fc;
/// `PCI_PRODUCT_NVIDIA_MCP73_HDA_2`: MCP73 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP73_HDA_2: u32 = 0x07fd;
/// `PCI_PRODUCT_NVIDIA_MCP77_HDA_1`: MCP77 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP77_HDA_1: u32 = 0x0774;
/// `PCI_PRODUCT_NVIDIA_MCP77_HDA_2`: MCP77 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP77_HDA_2: u32 = 0x0775;
/// `PCI_PRODUCT_NVIDIA_MCP77_HDA_3`: MCP77 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP77_HDA_3: u32 = 0x0776;
/// `PCI_PRODUCT_NVIDIA_MCP77_HDA_4`: MCP77 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP77_HDA_4: u32 = 0x0777;
/// `PCI_PRODUCT_NVIDIA_MCP79_HDA_1`: MCP79 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP79_HDA_1: u32 = 0x0ac0;
/// `PCI_PRODUCT_NVIDIA_MCP79_HDA_2`: MCP79 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP79_HDA_2: u32 = 0x0ac1;
/// `PCI_PRODUCT_NVIDIA_MCP79_HDA_3`: MCP79 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP79_HDA_3: u32 = 0x0ac2;
/// `PCI_PRODUCT_NVIDIA_MCP79_HDA_4`: MCP79 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP79_HDA_4: u32 = 0x0ac3;
/// `PCI_PRODUCT_NVIDIA_MCP89_HDA_1`: MCP89 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP89_HDA_1: u32 = 0x0d94;
/// `PCI_PRODUCT_NVIDIA_MCP89_HDA_2`: MCP89 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP89_HDA_2: u32 = 0x0d95;
/// `PCI_PRODUCT_NVIDIA_MCP89_HDA_3`: MCP89 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP89_HDA_3: u32 = 0x0d96;
/// `PCI_PRODUCT_NVIDIA_MCP89_HDA_4`: MCP89 HD Audio.
pub const PCI_PRODUCT_NVIDIA_MCP89_HDA_4: u32 = 0x0d97;
// ahci_pci.c

/// `PCI_VENDOR_ATI`: ATI.
pub const PCI_VENDOR_ATI: u32 = 0x1002;
/// `PCI_VENDOR_VIATECH`: VIA.
pub const PCI_VENDOR_VIATECH: u32 = 0x1106;
/// `PCI_VENDOR_SAMSUNG2`: Samsung.
pub const PCI_VENDOR_SAMSUNG2: u32 = 0x144d;
/// `PCI_VENDOR_ASMEDIA`: ASMedia.
pub const PCI_VENDOR_ASMEDIA: u32 = 0x1b21;
/// `PCI_VENDOR_ZHAOXIN`: Zhaoxin.
pub const PCI_VENDOR_ZHAOXIN: u32 = 0x1d17;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_1`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_1: u32 = 0x7800;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_2`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_2: u32 = 0x7801;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_3`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_3: u32 = 0x7802;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_4`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_4: u32 = 0x7803;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_5`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_5: u32 = 0x7804;
/// `PCI_PRODUCT_AMD_HUDSON2_SATA_6`: Hudson-2 SATA.
pub const PCI_PRODUCT_AMD_HUDSON2_SATA_6: u32 = 0x7805;
/// `PCI_PRODUCT_ASMEDIA_ASM1061_SATA`: ASM1061 SATA.
pub const PCI_PRODUCT_ASMEDIA_ASM1061_SATA: u32 = 0x0611;
/// `PCI_PRODUCT_ATI_SB600_SATA`: SB600 SATA.
pub const PCI_PRODUCT_ATI_SB600_SATA: u32 = 0x4380;
/// `PCI_PRODUCT_ATI_SBX00_SATA_1`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_1: u32 = 0x4390;
/// `PCI_PRODUCT_ATI_SBX00_SATA_2`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_2: u32 = 0x4391;
/// `PCI_PRODUCT_ATI_SBX00_SATA_3`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_3: u32 = 0x4392;
/// `PCI_PRODUCT_ATI_SBX00_SATA_4`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_4: u32 = 0x4393;
/// `PCI_PRODUCT_ATI_SBX00_SATA_5`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_5: u32 = 0x4394;
/// `PCI_PRODUCT_ATI_SBX00_SATA_6`: SBx00 SATA.
pub const PCI_PRODUCT_ATI_SBX00_SATA_6: u32 = 0x4395;
/// `PCI_PRODUCT_INTEL_6SERIES_AHCI_1`: 6 Series AHCI.
pub const PCI_PRODUCT_INTEL_6SERIES_AHCI_1: u32 = 0x1c02;
/// `PCI_PRODUCT_INTEL_6SERIES_AHCI_2`: 6 Series AHCI.
pub const PCI_PRODUCT_INTEL_6SERIES_AHCI_2: u32 = 0x1c03;
/// `PCI_PRODUCT_INTEL_6321ESB_AHCI`: 6321ESB AHCI.
pub const PCI_PRODUCT_INTEL_6321ESB_AHCI: u32 = 0x2681;
/// `PCI_PRODUCT_INTEL_82801GR_AHCI`: 82801GR AHCI.
pub const PCI_PRODUCT_INTEL_82801GR_AHCI: u32 = 0x27c1;
/// `PCI_PRODUCT_INTEL_82801GBM_AHCI`: 82801GBM AHCI.
pub const PCI_PRODUCT_INTEL_82801GBM_AHCI: u32 = 0x27c5;
/// `PCI_PRODUCT_INTEL_82801H_AHCI_6P`: 82801H AHCI.
pub const PCI_PRODUCT_INTEL_82801H_AHCI_6P: u32 = 0x2821;
/// `PCI_PRODUCT_INTEL_82801H_AHCI_4P`: 82801H AHCI.
pub const PCI_PRODUCT_INTEL_82801H_AHCI_4P: u32 = 0x2824;
/// `PCI_PRODUCT_INTEL_82801HBM_AHCI`: 82801HBM AHCI.
pub const PCI_PRODUCT_INTEL_82801HBM_AHCI: u32 = 0x2829;
/// `PCI_PRODUCT_INTEL_82801I_AHCI_1`: 82801I AHCI.
pub const PCI_PRODUCT_INTEL_82801I_AHCI_1: u32 = 0x2922;
/// `PCI_PRODUCT_INTEL_82801I_AHCI_2`: 82801I AHCI.
pub const PCI_PRODUCT_INTEL_82801I_AHCI_2: u32 = 0x2923;
/// `PCI_PRODUCT_INTEL_82801I_AHCI_3`: 82801I AHCI.
pub const PCI_PRODUCT_INTEL_82801I_AHCI_3: u32 = 0x2929;
/// `PCI_PRODUCT_INTEL_82801JD_AHCI`: 82801JD AHCI.
pub const PCI_PRODUCT_INTEL_82801JD_AHCI: u32 = 0x3a02;
/// `PCI_PRODUCT_INTEL_82801JI_AHCI`: 82801JI AHCI.
pub const PCI_PRODUCT_INTEL_82801JI_AHCI: u32 = 0x3a22;
/// `PCI_PRODUCT_INTEL_3400_AHCI_1`: 3400 AHCI.
pub const PCI_PRODUCT_INTEL_3400_AHCI_1: u32 = 0x3b22;
/// `PCI_PRODUCT_INTEL_3400_AHCI_2`: 3400 AHCI.
pub const PCI_PRODUCT_INTEL_3400_AHCI_2: u32 = 0x3b23;
/// `PCI_PRODUCT_INTEL_3400_AHCI_3`: 3400 AHCI.
pub const PCI_PRODUCT_INTEL_3400_AHCI_3: u32 = 0x3b29;
/// `PCI_PRODUCT_INTEL_3400_AHCI_4`: 3400 AHCI.
pub const PCI_PRODUCT_INTEL_3400_AHCI_4: u32 = 0x3b2f;
/// `PCI_PRODUCT_INTEL_EP80579_AHCI`: EP80579 AHCI.
pub const PCI_PRODUCT_INTEL_EP80579_AHCI: u32 = 0x5029;
/// `PCI_PRODUCT_SAMSUNG2_S4LN053X01`: S4LN053X01.
pub const PCI_PRODUCT_SAMSUNG2_S4LN053X01: u32 = 0x1600;
/// `PCI_PRODUCT_SAMSUNG2_XP941`: XP941.
pub const PCI_PRODUCT_SAMSUNG2_XP941: u32 = 0xa800;
/// `PCI_PRODUCT_SAMSUNG2_SM951_AHCI`: SM951 AHCI.
pub const PCI_PRODUCT_SAMSUNG2_SM951_AHCI: u32 = 0xa801;
/// `PCI_PRODUCT_VIATECH_VT8251_SATA`: VT8251 SATA.
pub const PCI_PRODUCT_VIATECH_VT8251_SATA: u32 = 0x3349;
/// `PCI_PRODUCT_ZHAOXIN_STORX_AHCI`: StorX AHCI.
pub const PCI_PRODUCT_ZHAOXIN_STORX_AHCI: u32 = 0x9083;

// siop(4): the Symbios Logic SCSI processors `siop_pci_common.c` knows.

/// `PCI_VENDOR_SYMBIOS`: Symbios Logic.
pub const PCI_VENDOR_SYMBIOS: u32 = 0x1000;
/// `PCI_PRODUCT_SYMBIOS_810`: 53c810.
pub const PCI_PRODUCT_SYMBIOS_810: u32 = 0x0001;
/// `PCI_PRODUCT_SYMBIOS_820`: 53c820.
pub const PCI_PRODUCT_SYMBIOS_820: u32 = 0x0002;
/// `PCI_PRODUCT_SYMBIOS_825`: 53c825.
pub const PCI_PRODUCT_SYMBIOS_825: u32 = 0x0003;
/// `PCI_PRODUCT_SYMBIOS_815`: 53c815.
pub const PCI_PRODUCT_SYMBIOS_815: u32 = 0x0004;
/// `PCI_PRODUCT_SYMBIOS_860`: 53c860.
pub const PCI_PRODUCT_SYMBIOS_860: u32 = 0x0006;
/// `PCI_PRODUCT_SYMBIOS_1510D`: 53c1510D.
pub const PCI_PRODUCT_SYMBIOS_1510D: u32 = 0x000a;
/// `PCI_PRODUCT_SYMBIOS_896`: 53c896.
pub const PCI_PRODUCT_SYMBIOS_896: u32 = 0x000b;
/// `PCI_PRODUCT_SYMBIOS_895`: 53c895.
pub const PCI_PRODUCT_SYMBIOS_895: u32 = 0x000c;
/// `PCI_PRODUCT_SYMBIOS_885`: 53c885.
pub const PCI_PRODUCT_SYMBIOS_885: u32 = 0x000d;
/// `PCI_PRODUCT_SYMBIOS_875`: 53c875.
pub const PCI_PRODUCT_SYMBIOS_875: u32 = 0x000f;
/// `PCI_PRODUCT_SYMBIOS_895A`: 53c895A.
pub const PCI_PRODUCT_SYMBIOS_895A: u32 = 0x0012;
/// `PCI_PRODUCT_SYMBIOS_1010`: 53c1010-33.
pub const PCI_PRODUCT_SYMBIOS_1010: u32 = 0x0020;
/// `PCI_PRODUCT_SYMBIOS_1010_2`: 53c1010-66.
pub const PCI_PRODUCT_SYMBIOS_1010_2: u32 = 0x0021;
/// `PCI_PRODUCT_SYMBIOS_875J`: 53c875J.
pub const PCI_PRODUCT_SYMBIOS_875J: u32 = 0x008f;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn ids_match_the_generated_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pcidevs.h");
        for (name, value) in [
            ("PCI_VENDOR_OPENBSD", PCI_VENDOR_OPENBSD),
            ("PCI_VENDOR_CIRRUS", PCI_VENDOR_CIRRUS),
            ("PCI_VENDOR_QUMRANET", PCI_VENDOR_QUMRANET),
            ("PCI_PRODUCT_OPENBSD_CONTROL", PCI_PRODUCT_OPENBSD_CONTROL),
            ("PCI_VENDOR_APPLE", PCI_VENDOR_APPLE),
            ("PCI_PRODUCT_APPLE_NVME1", PCI_PRODUCT_APPLE_NVME1),
            ("PCI_PRODUCT_APPLE_NVME2", PCI_PRODUCT_APPLE_NVME2),
            ("PCI_PRODUCT_APPLE_NVME3", PCI_PRODUCT_APPLE_NVME3),
            ("PCI_VENDOR_AMD", PCI_VENDOR_AMD),
            ("PCI_VENDOR_INTEL", PCI_VENDOR_INTEL),
            ("PCI_VENDOR_INVALID", PCI_VENDOR_INVALID),
            ("PCI_VENDOR_SIS", PCI_VENDOR_SIS),
            ("PCI_VENDOR_NVIDIA", PCI_VENDOR_NVIDIA),
            ("PCI_PRODUCT_AMD_PBC768_ACA", PCI_PRODUCT_AMD_PBC768_ACA),
            ("PCI_PRODUCT_AMD_8111_ACA", PCI_PRODUCT_AMD_8111_ACA),
            (
                "PCI_PRODUCT_INTEL_82801AA_ACA",
                PCI_PRODUCT_INTEL_82801AA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801AB_ACA",
                PCI_PRODUCT_INTEL_82801AB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801BA_ACA",
                PCI_PRODUCT_INTEL_82801BA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801CA_ACA",
                PCI_PRODUCT_INTEL_82801CA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801DB_ACA",
                PCI_PRODUCT_INTEL_82801DB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801EB_ACA",
                PCI_PRODUCT_INTEL_82801EB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_6300ESB_ACA",
                PCI_PRODUCT_INTEL_6300ESB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801FB_ACA",
                PCI_PRODUCT_INTEL_82801FB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_6321ESB_ACA",
                PCI_PRODUCT_INTEL_6321ESB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801GB_ACA",
                PCI_PRODUCT_INTEL_82801GB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82440MX_ACA",
                PCI_PRODUCT_INTEL_82440MX_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP04_AC97",
                PCI_PRODUCT_NVIDIA_MCP04_AC97,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE4_AC",
                PCI_PRODUCT_NVIDIA_NFORCE4_AC,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE2_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE2_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE3_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE3_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE_ACA,
            ),
            ("PCI_PRODUCT_NVIDIA_MCP51_ACA", PCI_PRODUCT_NVIDIA_MCP51_ACA),
            ("PCI_PRODUCT_SIS_7012_ACA", PCI_PRODUCT_SIS_7012_ACA),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_1", PCI_PRODUCT_AMD_17_1X_XHCI_1),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_2", PCI_PRODUCT_AMD_17_1X_XHCI_2),
            ("PCI_PRODUCT_AMD_17_6X_XHCI", PCI_PRODUCT_AMD_17_6X_XHCI),
            ("PCI_PRODUCT_CIRRUS_CL_PD6729", PCI_PRODUCT_CIRRUS_CL_PD6729),
            ("PCI_VENDOR_FRESCO", PCI_VENDOR_FRESCO),
            ("PCI_PRODUCT_FRESCO_FL1000", PCI_PRODUCT_FRESCO_FL1000),
            ("PCI_PRODUCT_FRESCO_FL1400", PCI_PRODUCT_FRESCO_FL1400),
            (
                "PCI_PRODUCT_INTEL_82371FB_ISA",
                PCI_PRODUCT_INTEL_82371FB_ISA,
            ),
            ("PCI_VENDOR_DELL", PCI_VENDOR_DELL),
            ("PCI_VENDOR_HP", PCI_VENDOR_HP),
            ("PCI_VENDOR_SYMBIOS", PCI_VENDOR_SYMBIOS),
            ("PCI_PRODUCT_SYMBIOS_810", PCI_PRODUCT_SYMBIOS_810),
            ("PCI_PRODUCT_SYMBIOS_820", PCI_PRODUCT_SYMBIOS_820),
            ("PCI_PRODUCT_SYMBIOS_825", PCI_PRODUCT_SYMBIOS_825),
            ("PCI_PRODUCT_SYMBIOS_815", PCI_PRODUCT_SYMBIOS_815),
            ("PCI_PRODUCT_SYMBIOS_860", PCI_PRODUCT_SYMBIOS_860),
            ("PCI_PRODUCT_SYMBIOS_1510D", PCI_PRODUCT_SYMBIOS_1510D),
            ("PCI_PRODUCT_SYMBIOS_896", PCI_PRODUCT_SYMBIOS_896),
            ("PCI_PRODUCT_SYMBIOS_895", PCI_PRODUCT_SYMBIOS_895),
            ("PCI_PRODUCT_SYMBIOS_885", PCI_PRODUCT_SYMBIOS_885),
            ("PCI_PRODUCT_SYMBIOS_875", PCI_PRODUCT_SYMBIOS_875),
            ("PCI_PRODUCT_SYMBIOS_895A", PCI_PRODUCT_SYMBIOS_895A),
            ("PCI_PRODUCT_SYMBIOS_1010", PCI_PRODUCT_SYMBIOS_1010),
            ("PCI_PRODUCT_SYMBIOS_1010_2", PCI_PRODUCT_SYMBIOS_1010_2),
            ("PCI_PRODUCT_SYMBIOS_875J", PCI_PRODUCT_SYMBIOS_875J),
            ("PCI_PRODUCT_AMD_15_6X_AUDIO", PCI_PRODUCT_AMD_15_6X_AUDIO),
            ("PCI_PRODUCT_AMD_17_1X_HDA", PCI_PRODUCT_AMD_17_1X_HDA),
            ("PCI_PRODUCT_AMD_17_3X_HDA", PCI_PRODUCT_AMD_17_3X_HDA),
            ("PCI_PRODUCT_AMD_17_HDA", PCI_PRODUCT_AMD_17_HDA),
            ("PCI_PRODUCT_AMD_HUDSON2_HDA", PCI_PRODUCT_AMD_HUDSON2_HDA),
            ("PCI_PRODUCT_ATI_SB450_HDA", PCI_PRODUCT_ATI_SB450_HDA),
            ("PCI_PRODUCT_ATI_SBX00_HDA", PCI_PRODUCT_ATI_SBX00_HDA),
            (
                "PCI_PRODUCT_INTEL_100SERIES_HDA",
                PCI_PRODUCT_INTEL_100SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_100SERIES_H_HDA",
                PCI_PRODUCT_INTEL_100SERIES_H_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_100SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_100SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_200SERIES_HDA",
                PCI_PRODUCT_INTEL_200SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_200SERIES_U_HDA",
                PCI_PRODUCT_INTEL_200SERIES_U_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_300SERIES_CAVS",
                PCI_PRODUCT_INTEL_300SERIES_CAVS,
            ),
            (
                "PCI_PRODUCT_INTEL_300SERIES_U_HDA",
                PCI_PRODUCT_INTEL_300SERIES_U_HDA,
            ),
            ("PCI_PRODUCT_INTEL_3400_HDA", PCI_PRODUCT_INTEL_3400_HDA),
            (
                "PCI_PRODUCT_INTEL_400SERIES_CAVS",
                PCI_PRODUCT_INTEL_400SERIES_CAVS,
            ),
            (
                "PCI_PRODUCT_INTEL_400SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_400SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_495SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_495SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_500SERIES_HDA",
                PCI_PRODUCT_INTEL_500SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_500SERIES_HDA_2",
                PCI_PRODUCT_INTEL_500SERIES_HDA_2,
            ),
            (
                "PCI_PRODUCT_INTEL_500SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_500SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_600SERIES_HDA",
                PCI_PRODUCT_INTEL_600SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_600SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_600SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_6321ESB_HDA",
                PCI_PRODUCT_INTEL_6321ESB_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_6SERIES_HDA",
                PCI_PRODUCT_INTEL_6SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_700SERIES_HDA",
                PCI_PRODUCT_INTEL_700SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_700SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_700SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_7SERIES_HDA",
                PCI_PRODUCT_INTEL_7SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_800SERIES_HDA",
                PCI_PRODUCT_INTEL_800SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801FB_HDA",
                PCI_PRODUCT_INTEL_82801FB_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801GB_HDA",
                PCI_PRODUCT_INTEL_82801GB_HDA,
            ),
            ("PCI_PRODUCT_INTEL_82801H_HDA", PCI_PRODUCT_INTEL_82801H_HDA),
            ("PCI_PRODUCT_INTEL_82801I_HDA", PCI_PRODUCT_INTEL_82801I_HDA),
            (
                "PCI_PRODUCT_INTEL_82801JD_HDA",
                PCI_PRODUCT_INTEL_82801JD_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801JI_HDA",
                PCI_PRODUCT_INTEL_82801JI_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_8SERIES_HDA",
                PCI_PRODUCT_INTEL_8SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_8SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_8SERIES_LP_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_9SERIES_HDA",
                PCI_PRODUCT_INTEL_9SERIES_HDA,
            ),
            (
                "PCI_PRODUCT_INTEL_9SERIES_LP_HDA",
                PCI_PRODUCT_INTEL_9SERIES_LP_HDA,
            ),
            ("PCI_PRODUCT_INTEL_ADL_N_HDA", PCI_PRODUCT_INTEL_ADL_N_HDA),
            (
                "PCI_PRODUCT_INTEL_APOLLOLAKE_HDA",
                PCI_PRODUCT_INTEL_APOLLOLAKE_HDA,
            ),
            ("PCI_PRODUCT_INTEL_ARL_U_HDA", PCI_PRODUCT_INTEL_ARL_U_HDA),
            (
                "PCI_PRODUCT_INTEL_BAYTRAIL_HDA",
                PCI_PRODUCT_INTEL_BAYTRAIL_HDA,
            ),
            ("PCI_PRODUCT_INTEL_BSW_HDA", PCI_PRODUCT_INTEL_BSW_HDA),
            ("PCI_PRODUCT_INTEL_C600_HDA", PCI_PRODUCT_INTEL_C600_HDA),
            ("PCI_PRODUCT_INTEL_C610_HDA_1", PCI_PRODUCT_INTEL_C610_HDA_1),
            ("PCI_PRODUCT_INTEL_C610_HDA_2", PCI_PRODUCT_INTEL_C610_HDA_2),
            ("PCI_PRODUCT_INTEL_C620_HDA_1", PCI_PRODUCT_INTEL_C620_HDA_1),
            ("PCI_PRODUCT_INTEL_C620_HDA_2", PCI_PRODUCT_INTEL_C620_HDA_2),
            ("PCI_PRODUCT_INTEL_EHL_HDA", PCI_PRODUCT_INTEL_EHL_HDA),
            ("PCI_PRODUCT_INTEL_GLK_HDA", PCI_PRODUCT_INTEL_GLK_HDA),
            ("PCI_PRODUCT_INTEL_JSL_HDA", PCI_PRODUCT_INTEL_JSL_HDA),
            ("PCI_PRODUCT_INTEL_LNL_HDA", PCI_PRODUCT_INTEL_LNL_HDA),
            ("PCI_PRODUCT_INTEL_MTL_HDA", PCI_PRODUCT_INTEL_MTL_HDA),
            ("PCI_PRODUCT_INTEL_PTL_HDA", PCI_PRODUCT_INTEL_PTL_HDA),
            ("PCI_PRODUCT_INTEL_PTL_H_HDA", PCI_PRODUCT_INTEL_PTL_H_HDA),
            ("PCI_PRODUCT_INTEL_QS57_HDA", PCI_PRODUCT_INTEL_QS57_HDA),
            ("PCI_PRODUCT_NVIDIA_MCP51_HDA", PCI_PRODUCT_NVIDIA_MCP51_HDA),
            ("PCI_PRODUCT_NVIDIA_MCP55_HDA", PCI_PRODUCT_NVIDIA_MCP55_HDA),
            (
                "PCI_PRODUCT_NVIDIA_MCP61_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP61_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP61_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP61_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP65_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP65_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP65_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP65_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP67_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP67_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP67_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP67_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP73_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP73_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP73_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP73_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP77_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP77_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP77_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP77_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP77_HDA_3",
                PCI_PRODUCT_NVIDIA_MCP77_HDA_3,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP77_HDA_4",
                PCI_PRODUCT_NVIDIA_MCP77_HDA_4,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP79_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP79_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP79_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP79_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP79_HDA_3",
                PCI_PRODUCT_NVIDIA_MCP79_HDA_3,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP79_HDA_4",
                PCI_PRODUCT_NVIDIA_MCP79_HDA_4,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP89_HDA_1",
                PCI_PRODUCT_NVIDIA_MCP89_HDA_1,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP89_HDA_2",
                PCI_PRODUCT_NVIDIA_MCP89_HDA_2,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP89_HDA_3",
                PCI_PRODUCT_NVIDIA_MCP89_HDA_3,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP89_HDA_4",
                PCI_PRODUCT_NVIDIA_MCP89_HDA_4,
            ),
            ("PCI_VENDOR_ATI", PCI_VENDOR_ATI),
            ("PCI_VENDOR_VIATECH", PCI_VENDOR_VIATECH),
            ("PCI_VENDOR_SAMSUNG2", PCI_VENDOR_SAMSUNG2),
            ("PCI_VENDOR_ASMEDIA", PCI_VENDOR_ASMEDIA),
            ("PCI_VENDOR_ZHAOXIN", PCI_VENDOR_ZHAOXIN),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_1",
                PCI_PRODUCT_AMD_HUDSON2_SATA_1,
            ),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_2",
                PCI_PRODUCT_AMD_HUDSON2_SATA_2,
            ),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_3",
                PCI_PRODUCT_AMD_HUDSON2_SATA_3,
            ),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_4",
                PCI_PRODUCT_AMD_HUDSON2_SATA_4,
            ),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_5",
                PCI_PRODUCT_AMD_HUDSON2_SATA_5,
            ),
            (
                "PCI_PRODUCT_AMD_HUDSON2_SATA_6",
                PCI_PRODUCT_AMD_HUDSON2_SATA_6,
            ),
            (
                "PCI_PRODUCT_ASMEDIA_ASM1061_SATA",
                PCI_PRODUCT_ASMEDIA_ASM1061_SATA,
            ),
            ("PCI_PRODUCT_ATI_SB600_SATA", PCI_PRODUCT_ATI_SB600_SATA),
            ("PCI_PRODUCT_ATI_SBX00_SATA_1", PCI_PRODUCT_ATI_SBX00_SATA_1),
            ("PCI_PRODUCT_ATI_SBX00_SATA_2", PCI_PRODUCT_ATI_SBX00_SATA_2),
            ("PCI_PRODUCT_ATI_SBX00_SATA_3", PCI_PRODUCT_ATI_SBX00_SATA_3),
            ("PCI_PRODUCT_ATI_SBX00_SATA_4", PCI_PRODUCT_ATI_SBX00_SATA_4),
            ("PCI_PRODUCT_ATI_SBX00_SATA_5", PCI_PRODUCT_ATI_SBX00_SATA_5),
            ("PCI_PRODUCT_ATI_SBX00_SATA_6", PCI_PRODUCT_ATI_SBX00_SATA_6),
            (
                "PCI_PRODUCT_INTEL_6SERIES_AHCI_1",
                PCI_PRODUCT_INTEL_6SERIES_AHCI_1,
            ),
            (
                "PCI_PRODUCT_INTEL_6SERIES_AHCI_2",
                PCI_PRODUCT_INTEL_6SERIES_AHCI_2,
            ),
            (
                "PCI_PRODUCT_INTEL_6321ESB_AHCI",
                PCI_PRODUCT_INTEL_6321ESB_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_82801GR_AHCI",
                PCI_PRODUCT_INTEL_82801GR_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_82801GBM_AHCI",
                PCI_PRODUCT_INTEL_82801GBM_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_82801H_AHCI_6P",
                PCI_PRODUCT_INTEL_82801H_AHCI_6P,
            ),
            (
                "PCI_PRODUCT_INTEL_82801H_AHCI_4P",
                PCI_PRODUCT_INTEL_82801H_AHCI_4P,
            ),
            (
                "PCI_PRODUCT_INTEL_82801HBM_AHCI",
                PCI_PRODUCT_INTEL_82801HBM_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_82801I_AHCI_1",
                PCI_PRODUCT_INTEL_82801I_AHCI_1,
            ),
            (
                "PCI_PRODUCT_INTEL_82801I_AHCI_2",
                PCI_PRODUCT_INTEL_82801I_AHCI_2,
            ),
            (
                "PCI_PRODUCT_INTEL_82801I_AHCI_3",
                PCI_PRODUCT_INTEL_82801I_AHCI_3,
            ),
            (
                "PCI_PRODUCT_INTEL_82801JD_AHCI",
                PCI_PRODUCT_INTEL_82801JD_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_82801JI_AHCI",
                PCI_PRODUCT_INTEL_82801JI_AHCI,
            ),
            (
                "PCI_PRODUCT_INTEL_3400_AHCI_1",
                PCI_PRODUCT_INTEL_3400_AHCI_1,
            ),
            (
                "PCI_PRODUCT_INTEL_3400_AHCI_2",
                PCI_PRODUCT_INTEL_3400_AHCI_2,
            ),
            (
                "PCI_PRODUCT_INTEL_3400_AHCI_3",
                PCI_PRODUCT_INTEL_3400_AHCI_3,
            ),
            (
                "PCI_PRODUCT_INTEL_3400_AHCI_4",
                PCI_PRODUCT_INTEL_3400_AHCI_4,
            ),
            (
                "PCI_PRODUCT_INTEL_EP80579_AHCI",
                PCI_PRODUCT_INTEL_EP80579_AHCI,
            ),
            (
                "PCI_PRODUCT_SAMSUNG2_S4LN053X01",
                PCI_PRODUCT_SAMSUNG2_S4LN053X01,
            ),
            ("PCI_PRODUCT_SAMSUNG2_XP941", PCI_PRODUCT_SAMSUNG2_XP941),
            (
                "PCI_PRODUCT_SAMSUNG2_SM951_AHCI",
                PCI_PRODUCT_SAMSUNG2_SM951_AHCI,
            ),
            (
                "PCI_PRODUCT_VIATECH_VT8251_SATA",
                PCI_PRODUCT_VIATECH_VT8251_SATA,
            ),
            (
                "PCI_PRODUCT_ZHAOXIN_STORX_AHCI",
                PCI_PRODUCT_ZHAOXIN_STORX_AHCI,
            ),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
