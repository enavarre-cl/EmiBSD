//! Tests of the AML types and their macros.

use super::*;

#[test]
fn method_and_field_flags() {
    assert_eq!(aml_method_argcount(0x0b), 3);
    assert_eq!(aml_method_serialized(0x0b), 1);
    assert_eq!(aml_method_synclevel(0x5b), 5);
    assert_eq!(aml_field_access(0x11), AML_FIELD_BYTEACC);
    assert_eq!(aml_field_lock(0x13), AML_FIELD_LOCK_ON);
    assert_eq!(aml_field_update(0x43), AML_FIELD_WRITEASZEROES);
    assert_eq!(aml_field_attr(0x0b05), 0x0b);
    assert_eq!(aml_bitmask(13), 0x20);
    assert_eq!(aml_bytelen(9), 2);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/acpi/amltypes.h");
    crate::reftest::assert_defines!(defs;
        AMLOP_ZERO, AMLOP_ONE, AMLOP_ALIAS, AMLOP_NAME, AMLOP_BYTEPREFIX, AMLOP_WORDPREFIX,
        AMLOP_DWORDPREFIX, AMLOP_STRINGPREFIX, AMLOP_QWORDPREFIX, AMLOP_SCOPE, AMLOP_BUFFER,
        AMLOP_PACKAGE, AMLOP_VARPACKAGE, AMLOP_METHOD, AMLOP_DUALNAMEPREFIX,
        AMLOP_MULTINAMEPREFIX, AMLOP_EXTPREFIX, AMLOP_MUTEX, AMLOP_EVENT, AMLOP_CONDREFOF,
        AMLOP_CREATEFIELD, AMLOP_LOADTABLE, AMLOP_LOAD, AMLOP_STALL, AMLOP_SLEEP,
        AMLOP_ACQUIRE, AMLOP_SIGNAL, AMLOP_WAIT, AMLOP_RESET, AMLOP_RELEASE, AMLOP_FROMBCD,
        AMLOP_TOBCD, AMLOP_UNLOAD, AMLOP_REVISION, AMLOP_DEBUG, AMLOP_FATAL, AMLOP_TIMER,
        AMLOP_OPREGION, AMLOP_FIELD, AMLOP_DEVICE, AMLOP_PROCESSOR, AMLOP_POWERRSRC,
        AMLOP_THERMALZONE, AMLOP_INDEXFIELD, AMLOP_BANKFIELD, AMLOP_DATAREGION,
        AMLOP_ROOTCHAR, AMLOP_PARENTPREFIX, AMLOP_NAMECHAR, AMLOP_LOCAL0, AMLOP_LOCAL1,
        AMLOP_LOCAL2, AMLOP_LOCAL3, AMLOP_LOCAL4, AMLOP_LOCAL5, AMLOP_LOCAL6, AMLOP_LOCAL7,
        AMLOP_ARG0, AMLOP_ARG1, AMLOP_ARG2, AMLOP_ARG3, AMLOP_ARG4, AMLOP_ARG5, AMLOP_ARG6,
        AMLOP_STORE, AMLOP_REFOF, AMLOP_ADD, AMLOP_CONCAT, AMLOP_SUBTRACT, AMLOP_INCREMENT,
        AMLOP_DECREMENT, AMLOP_MULTIPLY, AMLOP_DIVIDE, AMLOP_SHL, AMLOP_SHR, AMLOP_AND,
        AMLOP_NAND, AMLOP_OR, AMLOP_NOR, AMLOP_XOR, AMLOP_NOT, AMLOP_FINDSETLEFTBIT,
        AMLOP_FINDSETRIGHTBIT, AMLOP_DEREFOF, AMLOP_CONCATRES, AMLOP_MOD, AMLOP_NOTIFY,
        AMLOP_SIZEOF, AMLOP_INDEX, AMLOP_MATCH, AMLOP_CREATEDWORDFIELD,
        AMLOP_CREATEWORDFIELD, AMLOP_CREATEBYTEFIELD, AMLOP_CREATEBITFIELD,
        AMLOP_OBJECTTYPE, AMLOP_CREATEQWORDFIELD, AMLOP_LAND, AMLOP_LOR, AMLOP_LNOT,
        AMLOP_LNOTEQUAL, AMLOP_LLESSEQUAL, AMLOP_LGREATEREQUAL, AMLOP_LEQUAL,
        AMLOP_LGREATER, AMLOP_LLESS, AMLOP_TOBUFFER, AMLOP_TODECSTRING, AMLOP_TOHEXSTRING,
        AMLOP_TOINTEGER, AMLOP_TOSTRING, AMLOP_COPYOBJECT, AMLOP_MID, AMLOP_CONTINUE,
        AMLOP_IF, AMLOP_ELSE, AMLOP_WHILE, AMLOP_NOP, AMLOP_RETURN, AMLOP_BREAK,
        AMLOP_BREAKPOINT, AMLOP_ONES, AMLOP_INVALID, AML_MATCH_TR, AML_MATCH_EQ,
        AML_MATCH_LE, AML_MATCH_LT, AML_MATCH_GE, AML_MATCH_GT, AML_FIELD_ACCESSMASK,
        AML_FIELD_ANYACC, AML_FIELD_BYTEACC, AML_FIELD_WORDACC, AML_FIELD_DWORDACC,
        AML_FIELD_QWORDACC, AML_FIELD_BUFFERACC, AML_FIELD_LOCK_OFF, AML_FIELD_LOCK_ON,
        AML_FIELD_PRESERVE, AML_FIELD_WRITEASONES, AML_FIELD_WRITEASZEROES,
        AML_FIELD_RESERVED, AML_FIELD_ATTR__, AML_NO_TIMEOUT,
    );
}

#[test]
fn aml_pointers_read_zero_past_the_end() {
    static CODE: [u8; 4] = [1, 2, 3, 4];
    let p = AmlPtr::new(&CODE);
    assert_eq!(p.get16(), 0x0201);
    assert_eq!(p.add(2).get32(), 0x0403);
    assert_eq!(p.add(9).get8(), 0);
    assert!(p < p.add(1));
    assert_eq!(p.add(3).diff(p), 3);
    assert_eq!(p.add(1).bytes(8), &[2, 3, 4]);
}

#[test]
fn values_and_nodes() {
    let v = AmlValue::integer(7);
    assert_eq!(
        (v.r#type(), v.v_integer(), v.length()),
        (AML_OBJTYPE_INTEGER, 7, 8)
    );
    let s = AmlValue::string(b"ab\0c");
    assert_eq!((s.length(), s.v_string()), (4, b"ab".to_vec()));
    let root = Rc::new(AmlNode::new(None, *b"\\\0\0\0\0"));
    let child = Rc::new(AmlNode::new(Some(&root), *b"CHLD\0"));
    root.add_son(child.clone());
    assert!(child.parent().is_some_and(|p| Rc::ptr_eq(&p, &root)));
    assert_eq!(child.name(), b"CHLD");
    assert_eq!(
        root.take_first_son().map(|c| Rc::ptr_eq(&c, &child)),
        Some(true)
    );
    drop(root);
    assert!(child.parent().is_none());
}
