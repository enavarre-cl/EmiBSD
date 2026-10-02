//! Tests of `crc32c.rs`; see there.

use super::*;

#[test]
fn table_spot_checks() {
    assert_eq!(CRC32C_LOOKUP[0], 0x0000_0000);
    assert_eq!(CRC32C_LOOKUP[1], 0xf26b_8303);
    assert_eq!(CRC32C_LOOKUP[2], 0xe13b_70f7);
    assert_eq!(CRC32C_LOOKUP[128], 0x82f6_3b78);
    assert_eq!(CRC32C_LOOKUP[255], 0xad7d_5351);
}

#[test]
fn known_vectors() {
    // The standard CRC-32C check value.
    assert_eq!(crc32c(0, b"123456789"), 0xe306_9283);
    assert_eq!(crc32c(0, b""), 0);
    assert_eq!(crc32c(0, &[0u8; 32]), 0x8a91_36aa);
    assert_eq!(crc32c(0, &[0xffu8; 32]), 0x62a8_ab43);
}

#[test]
fn chaining_chunks_equals_one_call() {
    let whole = b"The quick brown fox jumps over the lazy dog";
    let (a, b) = whole.split_at(17);
    assert_eq!(crc32c(crc32c(0, a), b), crc32c(0, whole));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn table_matches_the_c_header() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("OPENBSD_SRC").expect("OPENBSD_SRC is not set; run `just test-ref`"),
    );
    // A relative $OPENBSD_SRC is taken from the workspace root, three levels up.
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(dir)
    };
    let path = dir.join("sys/lib/libkern/crc32c.h");
    let text = std::fs::read_to_string(&path).expect("read crc32c.h");
    let start = text.find("crc32c_lookup[] = {").expect("table start");
    let end = text[start..].find("};").expect("table end") + start;
    let values: std::vec::Vec<u32> = text[start..end]
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|tok| tok.strip_prefix("0x"))
        .map(|hex| u32::from_str_radix(hex, 16).expect("hex entry"))
        .collect();
    assert_eq!(values.len(), 256);
    assert_eq!(values[..], CRC32C_LOOKUP[..]);
}
