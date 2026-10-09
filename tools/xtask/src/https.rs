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
//! `--https-server DIR:PORT:MODE` for `cargo xtask smoke` and `smoke2` (M9+): TLS servers on
//! this machine for the guest's ftp(1) and nc(1), started before the VM boots and killed
//! when the run ends (`Servers` kills them when dropped).
//!
//! A guest on QEMU's user network reaches this machine at 10.0.2.2: slirp turns a
//! connection to that address into one to the host's 127.0.0.1 (the ramdisk's `/etc/hosts`
//! names it `emibsd-host`). So the servers listen on `PORT` here and the guest connects to
//! `emibsd-host:PORT`; no `hostfwd`/`guestfwd` is needed.
//!
//! Each server is this machine's `openssl s_server` (macOS's LibreSSL, `/usr/bin/openssl`,
//! or `$EMIBSD_OPENSSL`), with a certificate of the test CA `just userland` makes in
//! `target/userland/test-ca` (`userland/testca.rs`), run in `DIR` (relative to the
//! workspace root, which must exist). `MODE` is:
//!
//! - `trusted`: `-WWW` (answers `GET /<path>` with the file `DIR/<path>`) with the
//!   `emibsd-host` certificate the CA signed;
//! - `untrusted`: `-WWW` with a self-signed `emibsd-host` certificate a client must refuse;
//! - `echo`: plain mode with the trusted certificate; what a client sends is written back
//!   to it (the server's output is piped into its input).

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::Result;
use crate::userland::testca;

/// What a server answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Trusted,
    Untrusted,
    Echo,
}

/// One `--https-server` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub dir: PathBuf,
    pub port: u16,
    pub mode: Mode,
}

/// Parses `DIR:PORT:MODE` (the directory may not contain `:`).
pub fn parse_spec(s: &str) -> Result<Spec> {
    let bad = || format!("--https-server {s}: expected DIR:PORT:trusted|untrusted|echo");
    let mut parts = s.rsplitn(3, ':');
    let (Some(mode), Some(port), Some(dir)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad().into());
    };
    let mode = match mode {
        "trusted" => Mode::Trusted,
        "untrusted" => Mode::Untrusted,
        "echo" => Mode::Echo,
        _ => return Err(bad().into()),
    };
    let port = port.parse::<u16>().map_err(|_| bad())?;
    if dir.is_empty() || port == 0 {
        return Err(bad().into());
    }
    Ok(Spec {
        dir: PathBuf::from(dir),
        port,
        mode,
    })
}

/// The running servers; dropping it kills them.
pub struct Servers(Vec<Child>);

impl Drop for Servers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// How long a server may take to start listening.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Starts one server per spec (`--https-server` values, parsed here) and waits until each
/// accepts connections.
pub fn start(root: &Path, values: &[&str]) -> Result<Servers> {
    let mut servers = Servers(Vec::new());
    if values.is_empty() {
        return Ok(servers);
    }
    let openssl = testca::openssl()?;
    let ca = root
        .join("target")
        .join("userland")
        .join(testca::TEST_CA_DIR);
    for v in values {
        let spec = parse_spec(v)?;
        let (cert, key) = match spec.mode {
            Mode::Trusted | Mode::Echo => ("server.pem", "server.key"),
            Mode::Untrusted => ("untrusted.pem", "untrusted.key"),
        };
        let (cert, key) = (ca.join(cert), ca.join(key));
        if !cert.is_file() || !key.is_file() {
            return Err(format!(
                "https server: {} or {} missing; run `just userland` (it makes the test CA)",
                cert.display(),
                key.display()
            )
            .into());
        }
        let dir = root.join(&spec.dir);
        if !dir.is_dir() {
            return Err(format!("https server: {}: no such directory", dir.display()).into());
        }
        wait_free(spec.port)?;
        let mut cmd = Command::new(&openssl);
        cmd.arg("s_server")
            .args(["-accept", &spec.port.to_string()])
            .arg("-cert")
            .arg(&cert)
            .arg("-key")
            .arg(&key)
            .current_dir(&dir)
            .stderr(Stdio::null());
        match spec.mode {
            Mode::Trusted | Mode::Untrusted => {
                cmd.args(["-WWW", "-quiet"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null());
            }
            Mode::Echo => {
                cmd.arg("-quiet")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped());
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("https server: {}: {e}", openssl.display()))?;
        if spec.mode == Mode::Echo {
            // `-quiet` prints only what the client sent; send it back.
            let (Some(mut out), Some(mut input)) = (child.stdout.take(), child.stdin.take()) else {
                return Err("https server: no pipes to the echo server".into());
            };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = out.read(&mut buf) {
                    if n == 0 || input.write_all(&buf[..n]).is_err() || input.flush().is_err() {
                        break;
                    }
                }
            });
        }
        servers.0.push(child);
        wait_listening(spec.port)?;
        println!(
            "https server: {} on 127.0.0.1:{} (the guest's emibsd-host:{}), {:?}, in {}",
            cert.display(),
            spec.port,
            spec.port,
            spec.mode,
            dir.display()
        );
    }
    Ok(servers)
}

/// How long [`wait_free`] waits for another run's server to go away.
const FREE_TIMEOUT: Duration = Duration::from_secs(300);

/// Waits until nothing accepts TCP connections on 127.0.0.1:`port`. The ports are fixed (the
/// guest's commands name them), so a second run on this machine that needs them at the same
/// time (`smoke-https` in two worktrees) would otherwise fail to bind, and the guest would
/// talk to the other run's server while [`wait_listening`] took it for its own. Waiting
/// serialises the two; `smoke-all` itself never runs two such recipes at once.
fn wait_free(port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let started = Instant::now();
    let mut told = false;
    while TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
        if started.elapsed() > FREE_TIMEOUT {
            return Err(format!(
                "https server: {addr} still in use after {}s",
                FREE_TIMEOUT.as_secs()
            )
            .into());
        }
        if !told {
            println!("https server: {addr} is in use (another run?); waiting for it");
            told = true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

/// Waits until something accepts TCP connections on 127.0.0.1:`port`. The probe connects
/// and closes without a TLS handshake, which `s_server` drops before its next `accept`.
fn wait_listening(port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "https server: nothing listens on {addr} after {}s (port in use?)",
        START_TIMEOUT.as_secs()
    )
    .into())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_parse() {
        assert_eq!(
            parse_spec("target/https-www:8443:trusted").unwrap(),
            Spec {
                dir: PathBuf::from("target/https-www"),
                port: 8443,
                mode: Mode::Trusted
            }
        );
        assert_eq!(parse_spec("d:8444:echo").unwrap().mode, Mode::Echo);
        assert_eq!(parse_spec("d:1:untrusted").unwrap().mode, Mode::Untrusted);
        for bad in [
            "d:8443",
            "d:x:trusted",
            "d:0:echo",
            ":8443:echo",
            "d:8443:open",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad}");
        }
    }

    /// Runs the three servers against `openssl s_client`. Needs `/usr/bin/openssl` and the
    /// test CA (`just userland`): `cargo test -p xtask -- --ignored servers_answer`.
    #[test]
    #[ignore]
    fn servers_answer() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let www = root.join("target/https-test-www");
        std::fs::create_dir_all(&www).unwrap();
        std::fs::write(www.join("hello.txt"), "hello over https\n").unwrap();
        let _servers = start(
            &root,
            &[
                "target/https-test-www:18443:trusted",
                "target/https-test-www:18444:echo",
                "target/https-test-www:18445:untrusted",
            ],
        )
        .unwrap();
        let ca = root.join("target/userland/test-ca/ca.pem");
        let client = |port: u16, input: &str| -> (String, String) {
            let mut c = Command::new(testca::openssl().unwrap())
                .args(["s_client", "-connect", &format!("127.0.0.1:{port}")])
                .args([
                    "-servername",
                    "emibsd-host",
                    "-verify",
                    "5",
                    "-verify_return_error",
                ])
                .arg("-CAfile")
                .arg(&ca)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = c.stdin.take().unwrap();
            stdin.write_all(input.as_bytes()).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(stdin);
            let out = c.wait_with_output().unwrap();
            (
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        };
        let (out, _) = client(18443, "GET /hello.txt HTTP/1.0\r\n\r\n");
        assert!(out.contains("hello over https"), "{out}");
        let (out, _) = client(18444, "ping-echo\n");
        assert!(out.matches("ping-echo").count() >= 1, "{out}");
        let (out, err) = client(18445, "GET /hello.txt HTTP/1.0\r\n\r\n");
        assert!(!out.contains("hello over https"), "{out}{err}");
    }
}
/* </TESTS> */
