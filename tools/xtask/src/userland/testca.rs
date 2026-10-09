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
//! The ramdisk's TLS trust (M9+): `/etc/ssl/cert.pem`, LibreSSL's default CA bundle
//! (`lib/libcrypto/cert.pem`, what OpenBSD's `distribution` target installs), and
//! `/etc/ssl/emibsd-test-ca.pem`, the certificate of a test CA made here once, so that
//! `just smoke-https` can check a TLS connection to a server on this machine
//! (`openssl s_server`, started by `cargo xtask smoke --https-server`, `https.rs`).
//!
//! The CA is made with this machine's `/usr/bin/openssl` (LibreSSL 3.3.6 on macOS) into
//! `target/userland/test-ca/`, shared by both architectures, and kept: it is made only when
//! one of its files is missing (`rm -r target/userland/test-ca` makes a new one). Keys are
//! random, so a new CA differs from the old; names, serial numbers, extensions and the
//! ten-year validity are fixed (docs/SETUP.md, "The test CA"). The files:
//!
//! - `ca.key`, `ca.pem`: the CA (`CN=EmiBSD test CA`, `CA:TRUE`);
//! - `server.key`, `server.pem`: `CN=emibsd-host`, `subjectAltName=DNS:emibsd-host`, signed
//!   by the CA: the "trusted" server (`/etc/hosts` maps `emibsd-host` to `10.0.2.2`, QEMU's
//!   user-network alias of this machine);
//! - `untrusted.key`, `untrusted.pem`: the same names, self-signed: the "untrusted" server
//!   a client must refuse.
//!
//! All keys are ECDSA P-256. Nothing here is secret: it is a test fixture.

use super::*;

/// The directory of the test CA, below `target/userland`.
pub(crate) const TEST_CA_DIR: &str = "test-ca";

/// The host name the test servers' certificates are for (`/etc/hosts` in the ramdisk).
pub(super) const TEST_HOST: &str = "emibsd-host";

/// LibreSSL's default CA bundle, relative to the OpenBSD sources.
const CERT_PEM: &str = "lib/libcrypto/cert.pem";

/// The `openssl.cnf` the CA is made with: no prompts, the extensions of each certificate.
const OPENSSL_CNF: &str = "\
# EmiBSD test CA (tools/xtask, userland/testca.rs). A test fixture, not a real CA.
[ req ]
distinguished_name = dn

[ dn ]

[ v3_ca ]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash

[ v3_server ]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:emibsd-host
";

/// Validity of every certificate, in days.
const DAYS: &str = "3650";

/// The files of a complete test CA.
const FILES: &[&str] = &[
    "ca.key",
    "ca.pem",
    "server.key",
    "server.pem",
    "untrusted.key",
    "untrusted.pem",
];

/// `/usr/bin/openssl` (macOS's LibreSSL), or `$EMIBSD_OPENSSL`.
pub(crate) fn openssl() -> Result<PathBuf> {
    let p = std::env::var_os("EMIBSD_OPENSSL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/openssl"));
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!(
            "{}: not found; it makes the test CA and runs the test server (docs/SETUP.md, \
             \"The test CA\"); set $EMIBSD_OPENSSL",
            p.display()
        )
        .into())
    }
}

/// Makes the test CA in `dir` unless it is complete already; returns `dir`.
pub(super) fn ensure(dir: &Path) -> Result<PathBuf> {
    if FILES.iter().all(|f| dir.join(f).is_file()) {
        return Ok(dir.to_path_buf());
    }
    let openssl = openssl()?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    ramdisk::write_if_changed(&dir.join("openssl.cnf"), OPENSSL_CNF)?;
    let ossl = |args: &[&str]| -> Result<()> {
        run(Command::new(&openssl)
            .args(args)
            .current_dir(dir)
            .env("OPENSSL_CONF", dir.join("openssl.cnf")))
    };
    for key in ["ca.key", "server.key", "untrusted.key"] {
        ossl(&[
            "ecparam",
            "-name",
            "prime256v1",
            "-genkey",
            "-noout",
            "-out",
            key,
        ])?;
    }
    let subject_host = format!("/O=EmiBSD/CN={TEST_HOST}");
    let self_signed = |key: &str, serial: &str, subj: &str, ext: &str, out: &str| {
        ossl(&[
            "req",
            "-config",
            "openssl.cnf",
            "-new",
            "-x509",
            "-sha256",
            "-key",
            key,
            "-days",
            DAYS,
            "-set_serial",
            serial,
            "-subj",
            subj,
            "-extensions",
            ext,
            "-out",
            out,
        ])
    };
    self_signed(
        "ca.key",
        "1",
        "/O=EmiBSD/CN=EmiBSD test CA",
        "v3_ca",
        "ca.pem",
    )?;
    ossl(&[
        "req",
        "-config",
        "openssl.cnf",
        "-new",
        "-sha256",
        "-key",
        "server.key",
        "-subj",
        &subject_host,
        "-out",
        "server.csr",
    ])?;
    ossl(&[
        "x509",
        "-req",
        "-sha256",
        "-in",
        "server.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-set_serial",
        "2",
        "-days",
        DAYS,
        "-extfile",
        "openssl.cnf",
        "-extensions",
        "v3_server",
        "-out",
        "server.pem",
    ])?;
    self_signed(
        "untrusted.key",
        "3",
        &subject_host,
        "v3_server",
        "untrusted.pem",
    )?;
    // The trusted server's chain must verify against the CA, the untrusted one's must not.
    ossl(&["verify", "-CAfile", "ca.pem", "server.pem"])?;
    if ossl(&["verify", "-CAfile", "ca.pem", "untrusted.pem"]).is_ok() {
        return Err(format!(
            "{}: untrusted.pem verifies against the test CA",
            dir.display()
        )
        .into());
    }
    println!(
        "  test CA: made {} with {}",
        dir.display(),
        openssl.display()
    );
    Ok(dir.to_path_buf())
}

/// The files of the ramdisk's `/etc/ssl`: (name, source).
pub(super) fn ssl_files(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, PathBuf)>> {
    let userland = ctx
        .out
        .parent()
        .ok_or("target/userland: no parent directory")?;
    let ca = ensure(&userland.join(TEST_CA_DIR))?;
    let bundle = ctx.src.join(CERT_PEM);
    if !bundle.is_file() {
        return Err(format!("{}: missing", bundle.display()).into());
    }
    if let Ok(mut inputs) = ctx.inputs.lock() {
        inputs.insert(bundle.clone());
    }
    Ok(vec![
        ("cert.pem", bundle),
        ("emibsd-test-ca.pem", ca.join("ca.pem")),
    ])
}
/* </CODE> */
