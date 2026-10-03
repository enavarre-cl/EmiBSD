//! Build script: what `conf/newvers.sh` and the kernel Makefile do around the compiler.
//!
//! - Every build: the facts `conf/vers.rs` puts into the `version` string, as
//!   `EMIBSD_VERS_*` compile-time environment variables. They come from the environment only
//!   (no clock, no counter file, no network), so the same inputs give the same kernel:
//!   `EMIBSD_BUILD` (the build number; the justfile passes the commit count),
//!   `SOURCE_DATE_EPOCH` (the build date; the justfile passes the last commit's time),
//!   `USER`, and `EMIBSD_BUILD_HOST` (the justfile passes `hostname -s`).
//! - Bare-metal builds only: the per-architecture linker script. Host builds (tests,
//!   `cargo check`, the `sys/arch/host` double) never link a kernel.

use std::env;

/// The three-letter day names of `date(1)`, Sunday first.
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
/// The three-letter month names of `date(1)`.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The proleptic Gregorian date of `days` since 1970-01-01: (year, month 1..=12, day 1..=31).
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `date(1)`'s default output for `epoch` in UTC: `Sat Oct  3 10:00:00 UTC 2026`.
fn date_string(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    // 1970-01-01 was a Thursday.
    let wday = (days + 4).rem_euclid(7) as usize;
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} UTC {}",
        DAYS[wday],
        MONTHS[(m - 1) as usize],
        d,
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        y
    )
}

/// An environment variable the build depends on, `default` when unset or empty.
fn input(name: &str, default: &str) -> String {
    println!("cargo:rerun-if-env-changed={name}");
    match env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

/// The `newvers.sh` part: `${v}`, `${t}`, `${u}`, `${h}`, `${d}`.
fn newvers() {
    let build = input("EMIBSD_BUILD", "0");
    let build = build
        .parse::<u64>()
        .map_or_else(|_| "0".to_string(), |n| n.to_string());
    let epoch = input("SOURCE_DATE_EPOCH", "0").parse::<i64>().unwrap_or(0);
    let user = input("USER", "unknown");
    let host = input("EMIBSD_BUILD_HOST", "localhost");
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();

    println!("cargo:rustc-env=EMIBSD_VERS_BUILD={build}");
    println!("cargo:rustc-env=EMIBSD_VERS_DATE={}", date_string(epoch));
    println!("cargo:rustc-env=EMIBSD_VERS_USER={user}");
    println!("cargo:rustc-env=EMIBSD_VERS_HOST={host}");
    println!("cargo:rustc-env=EMIBSD_VERS_DIR={dir}");
}

// A build script that cannot determine its target has nothing sensible to do but stop.
#[allow(clippy::panic)]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    newvers();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("bsd: unsupported target_arch `{other}`; expected x86_64 or aarch64"),
    };
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ld = format!("{manifest_dir}/arch/{arch}/conf/kernel.ld");
    println!("cargo:rustc-link-arg-bins=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
