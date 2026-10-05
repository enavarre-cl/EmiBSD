use std::vec::Vec;

use super::*;

#[test]
fn table_ends_match_the_c() {
    assert_eq!(MULAWTOLIN16[0], [0x02, 0x84]);
    assert_eq!(MULAWTOLIN16[127], [0x80, 0x00]);
    assert_eq!(MULAWTOLIN16[128], [0xfd, 0x7c]);
    assert_eq!(MULAWTOLIN16[255], [0x80, 0x00]);
    assert_eq!(LINTOMULAW[..8], [0, 0, 0, 0, 0, 1, 1, 1]);
    assert_eq!(LINTOMULAW[127], 0x67);
    assert_eq!(LINTOMULAW[128], 0xff);
    assert_eq!(LINTOMULAW[255], 0x80);
}

#[test]
fn mulaw8_round_trips() {
    // 8-bit linear keeps only the high byte of the 16-bit sample, so a mu-law code comes
    // back as a neighbour at most 288 (16-bit) away, a little more than one 8-bit step.
    let mut codes: Vec<u8> = (0..=255).collect();
    let orig = codes.clone();
    mulaw_to_slinear8(&mut codes);
    slinear8_to_mulaw(&mut codes);
    for (&o, &c) in orig.iter().zip(&codes) {
        let d = (i32::from(mulaw_decode(o)) - i32::from(mulaw_decode(c))).abs();
        assert!(d <= 288, "code {o:#x} -> {c:#x}");
    }
    // Linear 8-bit samples come back within 2.
    let mut lin: Vec<u8> = (0..=255).collect();
    slinear8_to_mulaw(&mut lin);
    mulaw_to_slinear8(&mut lin);
    for (i, &l) in lin.iter().enumerate() {
        let want = i as u8 as i8;
        let got = l as i8;
        assert!(
            (i32::from(want) - i32::from(got)).abs() <= 2,
            "{want} -> {got}"
        );
    }
}

#[test]
fn mulaw24_matches_the_8_bit_tables() {
    let mut buf: Vec<u8> = Vec::new();
    for code in 0u8..=255 {
        buf.extend_from_slice(&(u32::from(code) << 16).to_ne_bytes());
    }
    buf.push(0xaa); // a partial group is left alone
    mulaw24_to_slinear24(&mut buf);
    for (code, q) in buf.chunks_exact(4).enumerate() {
        let w = i32::from_ne_bytes([q[0], q[1], q[2], q[3]]);
        let lin = i32::from(mulaw_decode(code as u8));
        assert_eq!(w, lin << 8, "code {code:#x}");
    }
    assert_eq!(buf[1024], 0xaa);
    slinear24_to_mulaw24(&mut buf);
    for (code, q) in buf.chunks_exact(4).enumerate() {
        let w = u32::from_ne_bytes([q[0], q[1], q[2], q[3]]);
        let back = (w >> 16) as u8;
        let mut one = [code as u8];
        mulaw_to_slinear8(&mut one);
        slinear8_to_mulaw(&mut one);
        assert_eq!(back, one[0], "code {code:#x}");
        assert_eq!(w & 0xff00_ffff, 0);
    }
}

/// The hex bytes of a C table, between `name` and the next `};`.
fn c_table(src: &str, name: &str) -> Vec<u8> {
    let start = src.find(name).expect("table");
    let body = &src[start..];
    let body = &body[body.find('=').expect("initialiser")..body.find("};").expect("end")];
    body.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.starts_with("0x"))
        .map(|t| u8::from_str_radix(&t[2..], 16).expect("hex"))
        .collect()
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn tables_match_the_c() {
    let path = crate::reftest::openbsd_src().join("sys/dev/mulaw.c");
    let src = std::fs::read_to_string(path).expect("mulaw.c");
    let lin16 = c_table(&src, "mulawtolin16[256][2]");
    assert_eq!(lin16.len(), 512);
    for (i, pair) in lin16.chunks_exact(2).enumerate() {
        assert_eq!(MULAWTOLIN16[i], [pair[0], pair[1]], "mulawtolin16[{i}]");
    }
    let tomu = c_table(&src, "lintomulaw[256]");
    assert_eq!(tomu.len(), 256);
    assert_eq!(&LINTOMULAW[..], &tomu[..]);
}
