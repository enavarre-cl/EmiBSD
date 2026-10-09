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
//! A tiny HTTP/1.0 file server on this machine for the guests (`diff-openbsd`): it serves
//! one directory, `GET` only, on `127.0.0.1:<port>`, which a guest on QEMU's user network
//! reaches as `10.0.2.2:<port>` (see `https.rs`). The installer fetches `install.conf` and the
//! disklabel template from it, and the OpenBSD VM its test program and scenario scripts.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::Result;

/// The running server; dropping it stops it.
pub(crate) struct Server {
    pub(crate) port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Serves `dir` on a free port.
    pub(crate) fn start(dir: &Path) -> Result<Server> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| format!("http server: bind: {e}"))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let dir = dir.to_path_buf();
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    let dir = dir.clone();
                    thread::spawn(move || {
                        let _ = handle(s, &dir);
                    });
                }
            }
        });
        Ok(Server {
            port,
            stop,
            thread: Some(thread),
        })
    }

    /// The URL a guest uses for `name`.
    pub(crate) fn url(&self, name: &str) -> String {
        format!("http://10.0.2.2:{}/{name}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The file a request line names under `dir`: `GET /a/b?x HTTP/1.1` → `dir/a/b`. `None` for
/// other methods and for paths that climb out of `dir`.
pub(crate) fn request_path(dir: &Path, line: &str) -> Option<PathBuf> {
    let mut words = line.split_whitespace();
    if words.next()? != "GET" {
        return None;
    }
    let target = words.next()?;
    let path = target.split(['?', '#']).next()?.trim_start_matches('/');
    if path.is_empty() || path.split('/').any(|c| c == ".." || c.is_empty()) {
        return None;
    }
    Some(dir.join(path))
}

fn handle(mut s: TcpStream, dir: &Path) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut req = Vec::new();
    let mut buf = [0u8; 1024];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") && req.len() < 16384 {
        let n = s.read(&mut buf)?;
        if n == 0 {
            break;
        }
        req.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&req);
    let line = text.lines().next().unwrap_or("");
    match request_path(dir, line).and_then(|p| fs::read(p).ok()) {
        Some(body) => {
            write!(
                s,
                "HTTP/1.0 200 OK\r\nContent-Type: application/octet-stream\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            s.write_all(&body)?;
        }
        None => {
            s.write_all(
                b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
        }
    }
    s.flush()
}
/* </CODE> */
