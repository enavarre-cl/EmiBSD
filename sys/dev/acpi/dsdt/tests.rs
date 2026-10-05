//! Host tests of the AML interpreter: hand-assembled AML (each block says the ASL it
//! encodes) parsed into a fresh namespace and evaluated through the external API.

use std::boxed::Box;
use std::string::String;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

use super::*;

/// The namespace is global: tests that use it take this lock (`AmlGlobal`'s host invariant).
static NS_LOCK: Mutex<()> = Mutex::new(());

/// A fresh namespace (`aml_create_defaultobjects`), held for the test.
fn fresh() -> MutexGuard<'static, ()> {
    let g = NS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    AML_INTLEN.store(64, Ordering::Relaxed);
    aml_create_defaultobjects();
    g
}

/// Parses `aml` as a definition block's byte code; the parser keeps pointers into it, so
/// it lives for the rest of the test run, as a mapped table does.
fn load(aml: Vec<u8>) -> i32 {
    let aml: &'static [u8] = Box::leak(aml.into_boxed_slice());
    acpi_parse_aml(None, None, aml)
}

/// `aml_evalinteger(\, name)`.
fn int(name: &[u8], argv: &[AmlValue]) -> i64 {
    let mut v = 0;
    let rc = aml_evalinteger(None, Some(&aml_root()), name, argv, &mut v);
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(name));
    v
}

/// `aml_evalname(\, name)` into a value of our own.
fn eval(name: &[u8], argv: &[AmlValue]) -> AmlValue {
    let res = AmlValue::new();
    let rc = aml_evalname(None, Some(&aml_root()), name, argv, Some(&res));
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(name));
    res
}

// A few encoders, so the byte strings below stay readable.

/// `PkgLength` of a body of `n` bytes (the length counts its own bytes).
fn pkglen(n: usize) -> Vec<u8> {
    if n + 1 <= 0x3F {
        vec![(n + 1) as u8]
    } else if n + 2 <= 0xFFF {
        let t = n + 2;
        vec![0x40 | (t & 0xF) as u8, (t >> 4) as u8]
    } else {
        let t = n + 3;
        vec![0x80 | (t & 0xF) as u8, (t >> 4) as u8, (t >> 12) as u8]
    }
}

/// Concatenates byte strings.
fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `op PkgLength body`.
fn pkg(op: &[u8], body: &[u8]) -> Vec<u8> {
    cat(&[op, &pkglen(body.len()), body])
}

/// `0x1234` as a `WordPrefix` constant.
fn w(x: u16) -> Vec<u8> {
    cat(&[&[0x0B], &x.to_le_bytes()])
}

/// A `DWordPrefix` constant.
fn d(x: u32) -> Vec<u8> {
    cat(&[&[0x0C], &x.to_le_bytes()])
}

/// A `BytePrefix` constant.
fn b(x: u8) -> Vec<u8> {
    vec![0x0A, x]
}

/// A string constant.
fn s(x: &str) -> Vec<u8> {
    cat(&[&[0x0D], x.as_bytes(), &[0]])
}

/// `Name(n, v)`.
fn name(n: &[u8], v: &[u8]) -> Vec<u8> {
    cat(&[&[0x08], n, v])
}

/// `Method(n, flags) { body }`.
fn method(n: &[u8], flags: u8, body: &[u8]) -> Vec<u8> {
    pkg(&[0x14], &cat(&[n, &[flags], body]))
}

/// `Return(t)`.
fn ret(t: &[u8]) -> Vec<u8> {
    cat(&[&[0xA4], t])
}

/// `Buffer(size) { bytes }`.
fn buffer(size: &[u8], bytes: &[u8]) -> Vec<u8> {
    pkg(&[0x11], &cat(&[size, bytes]))
}

/// `Package(n) { elems }`.
fn package(n: u8, elems: &[u8]) -> Vec<u8> {
    pkg(&[0x12], &cat(&[&[n], elems]))
}

/// `Device(n) { body }`.
fn device(n: &[u8], body: &[u8]) -> Vec<u8> {
    pkg(&[0x5B, 0x82], &cat(&[n, body]))
}

/// `Scope(n) { body }`.
fn scope(n: &[u8], body: &[u8]) -> Vec<u8> {
    pkg(&[0x10], &cat(&[n, body]))
}

/// `If (pred) { body }`.
fn if_(pred: &[u8], body: &[u8]) -> Vec<u8> {
    pkg(&[0xA0], &cat(&[pred, body]))
}

/// `Else { body }`.
fn else_(body: &[u8]) -> Vec<u8> {
    pkg(&[0xA1], body)
}

/// `While (pred) { body }`.
fn while_(pred: &[u8], body: &[u8]) -> Vec<u8> {
    pkg(&[0xA2], &cat(&[pred, body]))
}

const LOCAL0: u8 = 0x60;
const LOCAL1: u8 = 0x61;
const ARG0: u8 = 0x68;
const ARG1: u8 = 0x69;
const NULLNAME: u8 = 0x00;

#[test]
fn opcode_table_is_a_perfect_hash() {
    for op in &AML_TABLE {
        let found = aml_findopcode(op.opcode as i32).map(|o| o.mnem);
        assert_eq!(found, Some(op.mnem));
    }
    assert!(aml_findopcode(0x5B99).is_none());
    assert!(aml_findopcode(AMLOP_INVALID).is_none());
    assert_eq!(aml_mnem(AMLOP_STORE, None), b"Store");
    assert_eq!(aml_mnem(0x5B99, None), b"xxx");
}

#[test]
fn math_and_logic_follow_the_c() {
    assert_eq!(aml_evalexpr(u64::MAX, 2, AMLOP_ADD), 1);
    assert_eq!(aml_evalexpr(1, 65, AMLOP_SHL), 2);
    assert_eq!(aml_evalexpr(3, 3, AMLOP_LEQUAL), u64::MAX);
    assert_eq!(aml_evalexpr(3, 4, AMLOP_LGREATER), 0);
    assert_eq!(aml_evalexpr(0x80, 0, AMLOP_FINDSETLEFTBIT), 8);
    assert_eq!(aml_evalexpr(0x80, 0, AMLOP_FINDSETRIGHTBIT), 8);
    assert_eq!(aml_evalexpr(0x1234, 0, AMLOP_FROMBCD), 1234);
    assert_eq!(aml_evalexpr(1234, 0, AMLOP_TOBCD), 0x1234);
    assert_eq!(aml_evalexpr(7, 0, AMLOP_MOD), 0);
    assert_eq!(aml_hextoint(b"1fZ"), 0x1f);

    let mut dst = [0u8; 4];
    aml_bufcpy(&mut dst, 4, &[0xAB], 0, 8);
    assert_eq!(dst, [0xB0, 0x0A, 0, 0]);
}

#[test]
fn names_and_ids_decode() {
    assert_eq!(aml_getname(b"\\\x2e_SB_PCI0"), b"\\_SB_.PCI0");
    assert_eq!(aml_getname(b"\x2f\x03ABCDEFGHIJKL"), b"ABCD.EFGH.IJKL");
    assert_eq!(aml_getname(b"^^FOO_"), b"^^FOO_");
    assert_eq!(aml_getname(b"\\\0"), b"");
    assert_eq!(&aml_eisaid(0x030A_D041), b"PNP0A03");
    assert_eq!(&aml_eisaid(0x080A_D041), b"PNP0A08");
}

#[test]
fn data_objects() {
    let _g = fresh();
    // Name(INT1, 0x1234)
    // Name(STR1, "abc")
    // Name(BUF1, Buffer(4) {1, 2, 3})
    // Name(PKG1, Package(3) {One, "x", Package(1) {2}})
    // Name(ONES, Ones)
    let aml = cat(&[
        &name(b"INT1", &w(0x1234)),
        &name(b"STR1", &s("abc")),
        &name(b"BUF1", &buffer(&b(4), &[1, 2, 3])),
        &name(
            b"PKG1",
            &package(3, &cat(&[&[0x01], &s("x"), &package(1, &b(2))])),
        ),
        &name(b"ONES", &[0xFF]),
    ]);
    assert_eq!(load(aml), 0);

    assert_eq!(int(b"INT1", &[]), 0x1234);
    assert_eq!(int(b"ONES", &[]), -1);
    let s1 = eval(b"STR1", &[]);
    assert_eq!(s1.r#type(), AML_OBJTYPE_STRING);
    assert_eq!(s1.v_string(), b"abc");
    let b1 = eval(b"BUF1", &[]);
    assert_eq!(b1.v_buffer(), [1, 2, 3, 0]);
    let p = eval(b"PKG1", &[]);
    assert_eq!(p.length(), 3);
    assert_eq!(p.v_package(0).map(|v| v.v_integer()), Some(1));
    assert_eq!(p.v_package(1).map(|v| v.v_string()), Some(b"x".to_vec()));
    let inner = p.v_package(2).expect("inner package");
    assert_eq!(inner.v_package(0).map(|v| v.v_integer()), Some(2));

    // The default objects.
    assert_eq!(eval(b"_OS_", &[]).v_string(), b"Microsoft Windows NT");
    assert_eq!(int(b"_REV", &[]), 2);
    assert!(aml_searchname(Some(&aml_root()), b"\\_SB_").is_some());
    assert!(aml_global_lock().is_some());
}

#[test]
fn methods_arguments_and_control_flow() {
    let _g = fresh();
    // Method(ADD1, 2) { Return(Add(Arg0, Arg1)) }
    let add1 = method(b"ADD1", 2, &ret(&[0x72, ARG0, ARG1, NULLNAME]));
    // Method(MAXV, 2) { If (LGreater(Arg0, Arg1)) { Return(Arg0) } Else { Return(Arg1) } }
    let maxv = method(
        b"MAXV",
        2,
        &cat(&[
            &if_(&[0x94, ARG0, ARG1], &ret(&[ARG0])),
            &else_(&ret(&[ARG1])),
        ]),
    );
    // Method(SUMN, 1) {
    //     Store(Zero, Local0)
    //     Store(Zero, Local1)
    //     While (LLess(Local1, Arg0)) { Increment(Local1); Add(Local0, Local1, Local0) }
    //     Return(Local0)
    // }
    let sumn = method(
        b"SUMN",
        1,
        &cat(&[
            &[0x70, 0x00, LOCAL0],
            &[0x70, 0x00, LOCAL1],
            &while_(
                &[0x95, LOCAL1, ARG0],
                &[0x75, LOCAL1, 0x72, LOCAL0, LOCAL1, LOCAL0],
            ),
            &ret(&[LOCAL0]),
        ]),
    );
    // Method(BRKT) {
    //     Store(Zero, Local0)
    //     While (One) { Increment(Local0); If (LEqual(Local0, 5)) { Break } }
    //     Return(Local0)
    // }
    let brkt = method(
        b"BRKT",
        0,
        &cat(&[
            &[0x70, 0x00, LOCAL0],
            &while_(
                &[0x01],
                &cat(&[
                    &[0x75, LOCAL0],
                    &if_(&cat(&[&[0x93, LOCAL0], &b(5)]), &[0xA5]),
                ]),
            ),
            &ret(&[LOCAL0]),
        ]),
    );
    // Method(CALL) { Return(ADD1(MAXV(3, 9), 1)) }: a method calling methods, its arguments
    // parsed from the byte code.
    let call = method(
        b"CALL",
        0,
        &ret(&cat(&[b"ADD1", b"MAXV", &b(3), &b(9), &[0x01]])),
    );
    // Method(RIW_) { While (One) { Return(42) } }: a Return inside a While.
    let riw = method(b"RIW_", 0, &while_(&[0x01], &ret(&b(42))));
    assert_eq!(load(cat(&[&add1, &maxv, &sumn, &brkt, &call, &riw])), 0);

    assert_eq!(
        int(b"ADD1", &[AmlValue::integer(2), AmlValue::integer(3)]),
        5
    );
    assert_eq!(
        int(b"MAXV", &[AmlValue::integer(7), AmlValue::integer(4)]),
        7
    );
    assert_eq!(
        int(b"MAXV", &[AmlValue::integer(1), AmlValue::integer(4)]),
        4
    );
    assert_eq!(int(b"SUMN", &[AmlValue::integer(10)]), 55);
    assert_eq!(int(b"BRKT", &[]), 5);
    assert_eq!(int(b"CALL", &[]), 10);
    assert_eq!(int(b"RIW_", &[]), 42);
    // Locals start fresh on every call.
    assert_eq!(int(b"BRKT", &[]), 5);
}

#[test]
fn names_made_by_a_method_go_away() {
    let _g = fresh();
    // Method(TMPN) { Name(TEMP, 7); Return(Add(TEMP, 1)) }
    let tmpn = method(
        b"TMPN",
        0,
        &cat(&[
            &name(b"TEMP", &b(7)),
            &ret(&cat(&[&[0x72], b"TEMP", &[0x01, NULLNAME]])),
        ]),
    );
    assert_eq!(load(tmpn), 0);
    assert_eq!(int(b"TMPN", &[]), 8);
    let node = aml_searchname(Some(&aml_root()), b"TMPN").expect("TMPN");
    assert!(node.sons().is_empty());
    assert_eq!(int(b"TMPN", &[]), 8);
}

#[test]
fn buffer_fields_and_store() {
    let _g = fresh();
    // Name(BUF0, Buffer(8) {})
    // CreateDWordField(BUF0, 0, DW00)
    // CreateByteField(BUF0, 4, BY04)
    // CreateBitField(BUF0, 47, BI47)
    // CreateField(BUF0, 40, 4, NB40)
    // Method(SETF) { Store(0x11223344, DW00); Store(0xAB, BY04); Store(One, BI47);
    //                Store(0x5, NB40); Return(DW00) }
    let aml = cat(&[
        &name(b"BUF0", &buffer(&b(8), &[])),
        &cat(&[&[0x8A], b"BUF0", &[0x00], b"DW00"]),
        &cat(&[&[0x8C], b"BUF0", &b(4), b"BY04"]),
        &cat(&[&[0x8D], b"BUF0", &b(47), b"BI47"]),
        &cat(&[&[0x5B, 0x13], b"BUF0", &b(40), &b(4), b"NB40"]),
        &method(
            b"SETF",
            0,
            &cat(&[
                &cat(&[&[0x70], &d(0x1122_3344), b"DW00"]),
                &cat(&[&[0x70], &b(0xAB), b"BY04"]),
                &cat(&[&[0x70, 0x01], b"BI47"]),
                &cat(&[&[0x70], &b(5), b"NB40"]),
                &ret(b"DW00"),
            ]),
        ),
    ]);
    assert_eq!(load(aml), 0);
    assert_eq!(int(b"SETF", &[]), 0x1122_3344);
    assert_eq!(
        eval(b"BUF0", &[]).v_buffer(),
        [0x44, 0x33, 0x22, 0x11, 0xAB, 0x85, 0, 0]
    );
    assert_eq!(int(b"BY04", &[]), 0xAB);
    assert_eq!(int(b"BI47", &[]), 1);
}

/// The memory behind the test address space (0x80, an OEM space).
static TESTMEM: Mutex<[u8; 16]> = Mutex::new([0; 16]);

/// The handler of space 0x80: `TESTMEM` at 0x100..0x110.
fn testspace(_cookie: *mut c_void, iodir: i32, address: u64, size: i32, value: &mut u64) -> i32 {
    let mut m = TESTMEM.lock().unwrap_or_else(|e| e.into_inner());
    let off = (address - 0x100) as usize;
    for i in 0..size as usize {
        if iodir == ACPI_IOREAD {
            let b = u64::from(m[off + i]);
            if i == 0 {
                *value = 0;
            }
            *value |= b << (8 * i);
        } else {
            m[off + i] = (*value >> (8 * i)) as u8;
        }
    }
    0
}

#[test]
fn operation_region_fields() {
    let _g = fresh();
    *TESTMEM.lock().unwrap_or_else(|e| e.into_inner()) =
        [0x5A, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    // OperationRegion(TREG, 0x80, 0x100, 0x10)
    // Field(TREG, ByteAcc, NoLock, Preserve) { FLD0, 8, FLD1, 16, , 4, NIB0, 4 }
    // Method(WRF1, 1) { Store(Arg0, FLD1); Store(0xF, NIB0) }
    // Method(_REG, 2) { Store(Arg0, REGS) }
    // Name(REGS, Zero)
    let aml = cat(&[
        &cat(&[&[0x5B, 0x80], b"TREG", &[0x80], &w(0x100), &b(0x10)]),
        &pkg(
            &[0x5B, 0x81],
            &cat(&[
                b"TREG",
                &[0x01],
                b"FLD0",
                &[8],
                b"FLD1",
                &[16],
                &[0x00, 4],
                b"NIB0",
                &[4],
            ]),
        ),
        &method(
            b"WRF1",
            1,
            &cat(&[&[0x70, ARG0], b"FLD1", &[0x70], &b(0xF), b"NIB0"]),
        ),
        &method(b"_REG", 2, &cat(&[&[0x70, ARG0], b"REGS"])),
        &name(b"REGS", &[0x00]),
    ]);
    assert_eq!(load(aml), 0);
    aml_register_regionspace(&aml_root(), 0x80, ptr::null_mut(), testspace);
    assert_eq!(int(b"REGS", &[]), 0x80);

    assert_eq!(int(b"FLD0", &[]), 0x5A);
    assert_eq!(int(b"FLD1", &[]), 0x1234);
    let argv = [AmlValue::integer(0xBEEF)];
    assert_eq!(
        aml_evalname(None, Some(&aml_root()), b"WRF1", &argv, None),
        0
    );
    let m = *TESTMEM.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(&m[..4], &[0x5A, 0xEF, 0xBE, 0xF0]);
}

#[test]
fn osi_answers_windows() {
    let _g = fresh();
    // Method(OSIT, 1) { If (_OSI(Arg0)) { Return(One) } Return(Zero) }
    // Method(OSIW) { Return(_OSI("Windows 2015")) }
    let aml = cat(&[
        &method(
            b"OSIT",
            1,
            &cat(&[
                &if_(&cat(&[b"_OSI", &[ARG0]]), &ret(&[0x01])),
                &ret(&[0x00]),
            ]),
        ),
        &method(b"OSIW", 0, &ret(&cat(&[b"_OSI", &s("Windows 2015")]))),
    ]);
    assert_eq!(load(aml), 0);
    assert_eq!(int(b"OSIW", &[]), 1);
    assert_eq!(int(b"OSIT", &[AmlValue::string(b"Windows 2009")]), 1);
    assert_eq!(int(b"OSIT", &[AmlValue::string(b"Linux")]), 0);
    assert!(ACPI_MAX_OSI.load(Ordering::Relaxed) >= OSI_WIN_10);
}

#[test]
fn strings_conversions_and_references() {
    let _g = fresh();
    // Name(PKG2, Package(3) {0x10, 0x20, 0x30})
    // Method(CONC) { Return(Concat("ab", "cd")) }
    // Method(HEXS) { Return(ToHexString(0x1F)) }
    // Method(DECS) { Return(ToDecString(42)) }
    // Method(MIDS) { Return(Mid("abcdef", 2, 3)) }
    // Method(TOIN) { Return(ToInteger("1F")) }
    // Method(SIZE) { Return(SizeOf(PKG2)) }
    // Method(OTYP) { Return(ObjectType(PKG2)) }
    // Method(IDX1) { Return(DerefOf(Index(PKG2, 1))) }
    // Method(MTCH) { Return(Match(PKG2, MEQ, 0x30, MTR, 0, 0)) }
    // Method(REFT) { Store(RefOf(PKG2), Local0); Return(SizeOf(DerefOf(Local0))) }
    // Method(SIDX) { Store(5, Index(PKG2, 0)); Return(DerefOf(Index(PKG2, 0))) }
    // Method(DIV0) { Return(Divide(1, 0)) }
    // Method(DIVM) { Divide(17, 5, Local0, Local1); Return(Add(Multiply(Local1, 10), Local0)) }
    let aml = cat(&[
        &name(b"PKG2", &package(3, &cat(&[&b(0x10), &b(0x20), &b(0x30)]))),
        &method(
            b"CONC",
            0,
            &ret(&cat(&[&[0x73], &s("ab"), &s("cd"), &[NULLNAME]])),
        ),
        &method(b"HEXS", 0, &ret(&cat(&[&[0x98], &b(0x1F), &[NULLNAME]]))),
        &method(b"DECS", 0, &ret(&cat(&[&[0x97], &b(42), &[NULLNAME]]))),
        &method(
            b"MIDS",
            0,
            &ret(&cat(&[&[0x9E], &s("abcdef"), &b(2), &b(3), &[NULLNAME]])),
        ),
        &method(b"TOIN", 0, &ret(&cat(&[&[0x99], &s("1F"), &[NULLNAME]]))),
        &method(b"SIZE", 0, &ret(&cat(&[&[0x87], b"PKG2"]))),
        &method(b"OTYP", 0, &ret(&cat(&[&[0x8E], b"PKG2"]))),
        &method(
            b"IDX1",
            0,
            &ret(&cat(&[&[0x83, 0x88], b"PKG2", &[0x01, NULLNAME]])),
        ),
        &method(
            b"MTCH",
            0,
            &ret(&cat(&[
                &[0x89],
                b"PKG2",
                &[1],
                &b(0x30),
                &[0],
                &[0x00],
                &[0x00],
            ])),
        ),
        &method(
            b"REFT",
            0,
            &cat(&[
                &[0x70, 0x71],
                b"PKG2",
                &[LOCAL0],
                &ret(&[0x87, 0x83, LOCAL0]),
            ]),
        ),
        &method(
            b"SIDX",
            0,
            &cat(&[
                &cat(&[&[0x70], &b(5), &[0x88], b"PKG2", &[0x00, NULLNAME]]),
                &ret(&cat(&[&[0x83, 0x88], b"PKG2", &[0x00, NULLNAME]])),
            ]),
        ),
        &method(b"DIV0", 0, &ret(&[0x78, 0x01, 0x00, NULLNAME, NULLNAME])),
        &method(
            b"DIVM",
            0,
            &cat(&[
                &cat(&[&[0x78], &b(17), &b(5), &[LOCAL0, LOCAL1]]),
                &ret(&cat(&[
                    &[0x72, 0x77, LOCAL1],
                    &b(10),
                    &[NULLNAME, LOCAL0, NULLNAME],
                ])),
            ]),
        ),
    ]);
    assert_eq!(load(aml), 0);
    assert_eq!(eval(b"CONC", &[]).v_string(), b"abcd");
    assert_eq!(eval(b"HEXS", &[]).v_string(), b"0x1f");
    assert_eq!(eval(b"DECS", &[]).v_string(), b"42");
    assert_eq!(eval(b"MIDS", &[]).v_string(), b"cde");
    assert_eq!(int(b"TOIN", &[]), 0x1F);
    assert_eq!(int(b"SIZE", &[]), 3);
    assert_eq!(int(b"OTYP", &[]), i64::from(AML_OBJTYPE_PACKAGE));
    assert_eq!(int(b"IDX1", &[]), 0x20);
    assert_eq!(int(b"MTCH", &[]), 2);
    assert_eq!(int(b"REFT", &[]), 3);
    assert_eq!(int(b"SIDX", &[]), 5);
    assert_eq!(int(b"DIVM", &[]), 32);

    let res = AmlValue::new();
    assert_eq!(
        aml_evalname(None, Some(&aml_root()), b"DIV0", &[], Some(&res)),
        -1
    );
}

#[test]
fn devices_and_namespace_walks() {
    let _g = fresh();
    // Scope(\_SB) {
    //     Device(PCI0) {
    //         Name(_HID, EisaId("PNP0A08"))
    //         Name(_CID, EisaId("PNP0A03"))
    //         Name(_ADR, Zero)
    //         Device(ISA_) { Name(_ADR, 0x001F0000) }
    //     }
    //     Device(COM1) { Name(_HID, "PNP0501") }
    // }
    // Alias(\_SB.PCI0, \PCIA)
    let aml = cat(&[
        &scope(
            b"\\_SB_",
            &cat(&[
                &device(
                    b"PCI0",
                    &cat(&[
                        &name(b"_HID", &d(0x080A_D041)),
                        &name(b"_CID", &d(0x030A_D041)),
                        &name(b"_ADR", &[0x00]),
                        &device(b"ISA_", &name(b"_ADR", &d(0x001F_0000))),
                    ]),
                ),
                &device(b"COM1", &name(b"_HID", &s("PNP0501"))),
            ]),
        ),
        &cat(&[&[0x06, b'\\', 0x2E], b"_SB_PCI0", &[b'\\'], b"PCIA"]),
    ]);
    assert_eq!(load(aml), 0);

    let mut hids = Vec::new();
    aml_find_node(&aml_root(), b"_HID", &mut |n| {
        let parent = n.parent().expect("parent");
        let hid = AmlValue::new();
        assert_eq!(aml_evalhid(&parent, &hid), 0);
        hids.push((aml_nodename(Some(&parent)), hid.v_string()));
        0
    });
    assert_eq!(
        hids,
        [
            (b"\\_SB_.PCI0".to_vec(), b"PNP0A08".to_vec()),
            (b"\\_SB_.COM1".to_vec(), b"PNP0501".to_vec()),
        ]
    );

    let isa = aml_searchname(Some(&aml_root()), b"\\_SB_.PCI0.ISA_").expect("ISA_");
    assert_eq!(aml_nodename(Some(&isa)), b"\\_SB_.PCI0.ISA_");
    let mut adr = 0;
    assert_eq!(aml_evalinteger(None, Some(&isa), b"_ADR", &[], &mut adr), 0);
    assert_eq!(adr, 0x001F_0000);
    assert!(aml_searchrel(Some(&isa), b"COM1").is_some());
    assert!(aml_searchrel(Some(&isa), b"NOPE").is_none());
    assert!(aml_searchrel(Some(&isa), b"PCI0").is_some());

    let mut count = 0;
    aml_walknodes(Some(&aml_root()), AML_WALK_POST, &mut |_| {
        count += 1;
        0
    });
    // \, _OS_, _REV, _GL, _OSI, _GPE, _PR_, _SB_, _TZ_, _SI_, PCI0 (+5), COM1 (+1), PCIA
    assert_eq!(count, 19);

    // The alias reads as the device it names.
    let pcia = aml_searchname(Some(&aml_root()), b"PCIA").expect("PCIA");
    let (_, v) = aml_parsename(Some(&aml_root()), AmlPtr::new(b"PCIA"), false);
    assert_eq!(v.r#type(), AML_OBJTYPE_DEVICE);
    assert!(
        pcia.value()
            .is_some_and(|v| v.r#type() == AML_OBJTYPE_OBJREF)
    );
}

#[test]
fn resource_templates_parse() {
    let _g = fresh();
    // Name(_CRS, ResourceTemplate() {
    //     IO(Decode16, 0x3F8, 0x3F8, 1, 8)
    //     IRQNoFlags() {4}
    //     DWordMemory(ResourceProducer, PosDecode, MinFixed, MaxFixed, NonCacheable,
    //                 ReadWrite, 0, 0xFEBC0000, 0xFEBFFFFF, 0, 0x40000)
    // })
    let io = [0x47, 0x01, 0xF8, 0x03, 0xF8, 0x03, 0x01, 0x08];
    let irq = [0x22, 0x10, 0x00];
    let mut dw = vec![0x87, 0x17, 0x00, 0x00, 0x0C, 0x01];
    for x in [0u32, 0xFEBC_0000, 0xFEBF_FFFF, 0, 0x4_0000] {
        dw.extend_from_slice(&x.to_le_bytes());
    }
    let tmpl = cat(&[&io, &irq, &dw, &[0x79, 0x00]]);
    assert_eq!(load(name(b"_CRS", &buffer(&b(tmpl.len() as u8), &tmpl))), 0);

    let crs = eval(b"_CRS", &[]);
    let mut seen = Vec::new();
    assert_eq!(
        aml_parse_resource(&crs, &mut |idx, r| {
            match aml_crstype(r) {
                SR_IOPORT => seen.push((
                    idx,
                    u64::from(r.sr_ioport__min()),
                    u64::from(r.sr_ioport__len()),
                )),
                SR_IRQ => seen.push((idx, u64::from(r.sr_irq_irq_mask()), 0u64)),
                LR_DWORD => seen.push((
                    idx,
                    u64::from(r.lr_dword__min()),
                    u64::from(r.lr_dword__len()),
                )),
                t => panic!("unexpected descriptor {t:#x}"),
            }
            0
        }),
        0
    );
    assert_eq!(
        seen,
        [(0, 0x3F8, 8), (1, 0x10, 0), (2, 0xFEBC_0000, 0x4_0000)]
    );

    // ConcatenateResTemplate of the template with itself keeps one end tag.
    let both = aml_concatres(&Rc::new(eval(b"_CRS", &[])), &Rc::new(eval(b"_CRS", &[])));
    assert_eq!(both.length() as usize, 2 * (tmpl.len() - 2) + 2);
}

#[test]
fn forward_references_in_packages_are_fixed_up() {
    let _g = fresh();
    // Name(DEPS, Package(1) {\LATE})
    // Device(LATE) {}
    let aml = cat(&[
        &name(b"DEPS", &package(1, b"\\LATE")),
        &device(b"LATE", &[]),
    ]);
    assert_eq!(load(aml), 0);
    let deps = aml_searchname(Some(&aml_root()), b"DEPS")
        .and_then(|n| n.value())
        .expect("DEPS");
    assert_eq!(
        deps.v_package(0).map(|v| v.r#type()),
        Some(AML_OBJTYPE_NAMEREF)
    );
    let mut list = AcpiDevlistHead::new();
    acpi_getdevlist(&mut list, Some(&aml_root()), &deps, 0);
    assert_eq!(list.len(), 1);
    aml_postparse();
    assert_eq!(
        deps.v_package(0).map(|v| v.r#type()),
        Some(AML_OBJTYPE_OBJREF)
    );
    acpi_freedevlist(&mut list);
    assert!(list.is_empty());
}

/// Counts the callbacks of `notify_dev_reaches_its_callback`.
static NOTIFIED: AtomicI32 = AtomicI32::new(0);

fn on_notify(_node: &AmlNodeRef, value: i32, _arg: *mut c_void) -> i32 {
    NOTIFIED.fetch_add(value, Ordering::Relaxed);
    0
}

#[test]
fn notify_dev_reaches_its_callback() {
    let _g = fresh();
    let sb = aml_searchname(Some(&aml_root()), b"_SB_").expect("_SB_");
    aml_register_notify(&sb, Some(b"ACPI0003\0"), on_notify, ptr::null_mut(), 0);
    aml_notify_dev(Some(b"ACPI0003"), 0x80);
    aml_notify_dev(Some(b"PNP0C0A"), 0x80);
    assert_eq!(NOTIFIED.load(Ordering::Relaxed), 0x80);
}

/// A piece of a real DSDT, written the way QEMU's q35 one is: `\_S5`, `\_SB.PCI0` with a
/// `_CRS` method that patches a resource template through `CreateDWordField`s, and an
/// `_OSC` that reads and edits its capabilities buffer.
#[test]
fn q35_style_dsdt_excerpt() {
    let _g = fresh();
    // Name(\_S5, Package(4) {Zero, Zero, Zero, Zero})
    // Scope(\_SB) {
    //     Device(PCI0) {
    //         Name(_HID, EisaId("PNP0A08"))
    //         Name(_UID, Zero)
    //         Name(CRES, ResourceTemplate() {
    //             DWordMemory(ResourceProducer, PosDecode, MinFixed, MaxFixed, NonCacheable,
    //                         ReadWrite, 0, 0x80000000, 0xFEBFFFFF, 0, 0x7EC00000)
    //         })
    //         Method(_CRS) {
    //             CreateDWordField(CRES, 0x0A, PMIN)    // _MIN of the descriptor
    //             CreateDWordField(CRES, 0x16, PLEN)    // _LEN
    //             Store(0xC0000000, PMIN)
    //             Subtract(0xFEC00000, PMIN, PLEN)
    //             Return(CRES)
    //         }
    //         Method(_OSC, 4) {
    //             CreateDWordField(Arg3, 0, CDW1)
    //             CreateDWordField(Arg3, 8, CDW3)
    //             If (LEqual(Arg1, One)) { And(CDW3, 0x1F, CDW3) } Else { Or(CDW1, 0x08, CDW1) }
    //             Return(Arg3)
    //         }
    //     }
    // }
    let mut dw = vec![0x87, 0x17, 0x00, 0x00, 0x0C, 0x01];
    for x in [0u32, 0x8000_0000, 0xFEBF_FFFF, 0, 0x7EC0_0000] {
        dw.extend_from_slice(&x.to_le_bytes());
    }
    let tmpl = cat(&[&dw, &[0x79, 0x00]]);
    let crs = method(
        b"_CRS",
        0,
        &cat(&[
            &cat(&[&[0x8A], b"CRES", &b(0x0A), b"PMIN"]),
            &cat(&[&[0x8A], b"CRES", &b(0x16), b"PLEN"]),
            &cat(&[&[0x70], &d(0xC000_0000), b"PMIN"]),
            &cat(&[&[0x74], &d(0xFEC0_0000), b"PMIN", b"PLEN"]),
            &ret(b"CRES"),
        ]),
    );
    let osc = method(
        b"_OSC",
        4,
        &cat(&[
            &cat(&[&[0x8A, 0x6B, 0x00], b"CDW1"]),
            &cat(&[&[0x8A, 0x6B], &b(8), b"CDW3"]),
            &if_(
                &[0x93, ARG1, 0x01],
                &cat(&[&[0x7B], b"CDW3", &b(0x1F), b"CDW3"]),
            ),
            &else_(&cat(&[&[0x7D], b"CDW1", &b(0x08), b"CDW1"])),
            &ret(&[0x6B]),
        ]),
    );
    let aml = cat(&[
        &name(b"\\_S5_", &package(4, &[0, 0, 0, 0])),
        &scope(
            b"\\_SB_",
            &device(
                b"PCI0",
                &cat(&[
                    &name(b"_HID", &d(0x080A_D041)),
                    &name(b"_UID", &[0x00]),
                    &name(b"CRES", &buffer(&b(tmpl.len() as u8), &tmpl)),
                    &crs,
                    &osc,
                ]),
            ),
        ),
    ]);
    assert_eq!(load(aml), 0);

    assert_eq!(eval(b"\\_S5_", &[]).length(), 4);

    let pci0 = aml_searchname(Some(&aml_root()), b"\\_SB_.PCI0").expect("PCI0");
    let res = AmlValue::new();
    assert_eq!(aml_evalname(None, Some(&pci0), b"_CRS", &[], Some(&res)), 0);
    let mut win = None;
    aml_parse_resource(&res, &mut |_, r| {
        win = Some((r.lr_dword__min(), r.lr_dword__len()));
        0
    });
    assert_eq!(win, Some((0xC000_0000, 0x3EC0_0000)));

    let caps = [1u32, 0, 0xFF, 0];
    let mut bytes = Vec::new();
    for c in caps {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    let argv = [
        AmlValue::buffer(&[0; 16]),
        AmlValue::integer(1),
        AmlValue::integer(4),
        AmlValue::buffer(&bytes),
    ];
    let out = AmlValue::new();
    assert_eq!(
        aml_evalname(None, Some(&pci0), b"_OSC", &argv, Some(&out)),
        0
    );
    let o = out.v_buffer();
    assert_eq!(&o[8..12], &0x1Fu32.to_le_bytes());
    assert_eq!(&o[0..4], &1u32.to_le_bytes());
}

#[test]
fn bad_byte_code_reports_an_error() {
    let _g = fresh();
    // Method(UNDF) { Return(NOPE) }: NOPE does not exist.
    assert_eq!(load(method(b"UNDF", 0, &ret(b"NOPE"))), 0);
    let res = AmlValue::new();
    assert_eq!(
        aml_evalname(None, Some(&aml_root()), b"UNDF", &[], Some(&res)),
        -1
    );
    assert_eq!(
        aml_evalname(None, Some(&aml_root()), b"NONE", &[], Some(&res)),
        ACPI_E_BADVALUE
    );
}

/// The index/data register pair behind space 0x81: 0x100 selects, 0x101 reads and writes
/// the selected register.
static INDEXREGS: Mutex<(usize, [u8; 8])> = Mutex::new((0, [0; 8]));

/// The handler of space 0x81.
fn indexspace(_cookie: *mut c_void, iodir: i32, address: u64, _size: i32, value: &mut u64) -> i32 {
    let mut r = INDEXREGS.lock().unwrap_or_else(|e| e.into_inner());
    match (address, iodir) {
        (0x100, ACPI_IOWRITE) => r.0 = (*value as usize) & 7,
        (0x100, _) => *value = r.0 as u64,
        (_, ACPI_IOWRITE) => {
            let i = r.0;
            r.1[i] = *value as u8;
        }
        _ => *value = u64::from(r.1[r.0]),
    }
    0
}

#[test]
fn index_and_bank_fields() {
    let _g = fresh();
    *INDEXREGS.lock().unwrap_or_else(|e| e.into_inner()) = (0, [0, 0, 0, 0x34, 0x12, 0, 0, 0]);
    // OperationRegion(IREG, 0x81, 0x100, 2)
    // Field(IREG, ByteAcc, NoLock, Preserve) { IDX0, 8, DAT0, 8 }
    // IndexField(IDX0, DAT0, ByteAcc, NoLock, Preserve) { , 16, REG2, 8, REG3, 16 }
    // OperationRegion(BREG, 0x80, 0x108, 4)
    // Field(BREG, ByteAcc, NoLock, Preserve) { BNK0, 8 }
    // BankField(BREG, BNK0, One, ByteAcc, NoLock, Preserve) { Offset(2), BFL0, 8 }
    // Method(WREG, 1) { Store(Arg0, REG2) }
    let aml = cat(&[
        &cat(&[&[0x5B, 0x80], b"IREG", &[0x81], &w(0x100), &b(2)]),
        &pkg(
            &[0x5B, 0x81],
            &cat(&[b"IREG", &[0x01], b"IDX0", &[8], b"DAT0", &[8]]),
        ),
        &pkg(
            &[0x5B, 0x86],
            &cat(&[
                b"IDX0",
                b"DAT0",
                &[0x01],
                &[0x00, 16],
                b"REG2",
                &[8],
                b"REG3",
                &[16],
            ]),
        ),
        &cat(&[&[0x5B, 0x80], b"BREG", &[0x80], &w(0x108), &b(4)]),
        &pkg(&[0x5B, 0x81], &cat(&[b"BREG", &[0x01], b"BNK0", &[8]])),
        &pkg(
            &[0x5B, 0x87],
            &cat(&[
                b"BREG",
                b"BNK0",
                &[0x01],
                &[0x01],
                &[0x00, 16],
                b"BFL0",
                &[8],
            ]),
        ),
        &method(b"WREG", 1, &cat(&[&[0x70, ARG0], b"REG2"])),
    ]);
    assert_eq!(load(aml), 0);
    aml_register_regionspace(&aml_root(), 0x81, ptr::null_mut(), indexspace);
    aml_register_regionspace(&aml_root(), 0x80, ptr::null_mut(), testspace);
    *TESTMEM.lock().unwrap_or_else(|e| e.into_inner()) =
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x99, 0, 0, 0, 0, 0];

    assert_eq!(int(b"REG3", &[]), 0x1234);
    let argv = [AmlValue::integer(0x77)];
    assert_eq!(
        aml_evalname(None, Some(&aml_root()), b"WREG", &argv, None),
        0
    );
    assert_eq!(
        INDEXREGS.lock().unwrap_or_else(|e| e.into_inner()).1[2],
        0x77
    );

    // BFL0 is the byte at 0x10A once bank 1 is selected in BNK0 (0x108).
    assert_eq!(int(b"BFL0", &[]), 0x99);
    assert_eq!(TESTMEM.lock().unwrap_or_else(|e| e.into_inner())[8], 1);
}

#[test]
fn mutexes_events_and_small_operators() {
    let _g = fresh();
    // Mutex(MUT0, 0)
    // Event(EVT0)
    // Name(BUFX, Buffer(2) {})
    // Method(MTXT) { Store(Acquire(MUT0, 0xFFFF), Local0); Release(MUT0); Return(Local0) }
    // Method(EVTT) { Signal(EVT0); Return(Wait(EVT0, 10)) }
    // Method(CRF1) { Return(CondRefOf(EVT0, Local0)) }
    // Method(CRF0) { Return(CondRefOf(NONE, Local0)) }
    // Method(IDXB) { Store(0x55, Index(BUFX, 1)); Return(DerefOf(Index(BUFX, 1))) }
    // Method(MISC) { Return(Add(ShiftLeft(1, 4), FindSetLeftBit(0x10))) }
    // Method(LNEQ) { Return(LNotEqual(1, 2)) }
    // Method(LNOT) { Return(LNot(Zero)) }
    // Method(CATI) { Return(SizeOf(Concat(1, 2))) }
    // Method(TBUF) { Return(ToBuffer("ab")) }
    // Method(VPKG) { Store(3, Local0); Return(SizeOf(VarPackage(Local0) {1, 2, 3})) }
    let aml = cat(&[
        &cat(&[&[0x5B, 0x01], b"MUT0", &[0x00]]),
        &cat(&[&[0x5B, 0x02], b"EVT0"]),
        &name(b"BUFX", &buffer(&b(2), &[])),
        &method(
            b"MTXT",
            0,
            &cat(&[
                &cat(&[&[0x70, 0x5B, 0x23], b"MUT0", &[0xFF, 0xFF, LOCAL0]]),
                &cat(&[&[0x5B, 0x27], b"MUT0"]),
                &ret(&[LOCAL0]),
            ]),
        ),
        &method(
            b"EVTT",
            0,
            &cat(&[
                &cat(&[&[0x5B, 0x24], b"EVT0"]),
                &ret(&cat(&[&[0x5B, 0x25], b"EVT0", &b(10)])),
            ]),
        ),
        &method(b"CRF1", 0, &ret(&cat(&[&[0x5B, 0x12], b"EVT0", &[LOCAL0]]))),
        &method(b"CRF0", 0, &ret(&cat(&[&[0x5B, 0x12], b"NONE", &[LOCAL0]]))),
        &method(
            b"IDXB",
            0,
            &cat(&[
                &cat(&[&[0x70], &b(0x55), &[0x88], b"BUFX", &[0x01, NULLNAME]]),
                &ret(&cat(&[&[0x83, 0x88], b"BUFX", &[0x01, NULLNAME]])),
            ]),
        ),
        &method(
            b"MISC",
            0,
            &ret(&cat(&[
                &[0x72, 0x79, 0x01],
                &b(4),
                &[NULLNAME, 0x81],
                &b(0x10),
                &[NULLNAME, NULLNAME],
            ])),
        ),
        &method(b"LNEQ", 0, &ret(&[0x92, 0x93, 0x01, 0x0A, 0x02])),
        &method(b"LNOT", 0, &ret(&[0x92, 0x00])),
        &method(b"CATI", 0, &ret(&[0x87, 0x73, 0x01, 0x0A, 0x02, NULLNAME])),
        &method(b"TBUF", 0, &ret(&cat(&[&[0x96], &s("ab"), &[NULLNAME]]))),
        &method(
            b"VPKG",
            0,
            &cat(&[
                &[0x70, 0x0A, 0x03, LOCAL0],
                &ret(&cat(&[
                    &[0x87],
                    &pkg(&[0x13], &[LOCAL0, 0x01, 0x0A, 0x02, 0x0A, 0x03]),
                ])),
            ]),
        ),
    ]);
    assert_eq!(load(aml), 0);
    assert_eq!(int(b"MTXT", &[]), 0);
    assert_eq!(int(b"EVTT", &[]), 0);
    assert_eq!(int(b"CRF1", &[]), -1);
    assert_eq!(int(b"CRF0", &[]), 0);
    assert_eq!(int(b"IDXB", &[]), 0x55);
    assert_eq!(eval(b"BUFX", &[]).v_buffer(), [0, 0x55]);
    assert_eq!(int(b"MISC", &[]), 16 + 5);
    assert_eq!(int(b"LNEQ", &[]), -1);
    assert_eq!(int(b"LNOT", &[]), -1);
    assert_eq!(int(b"CATI", &[]), 16);
    let tb = eval(b"TBUF", &[]);
    assert_eq!(
        (tb.r#type(), tb.v_buffer()),
        (AML_OBJTYPE_BUFFER, b"ab".to_vec())
    );
    assert_eq!(int(b"VPKG", &[]), 3);
}
