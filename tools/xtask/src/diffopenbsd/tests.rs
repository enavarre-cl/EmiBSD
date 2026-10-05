use super::scenario::*;
use super::*;

#[test]
fn snapshot_file_parses_and_names_both_archs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let s = load_snapshot(&root).unwrap();
    for a in ["amd64", "arm64"] {
        let x = &s.arch[a];
        assert_eq!(x.install_img_sha256.len(), 64);
        assert_eq!(x.sha256_file.len(), 64);
    }
}

#[test]
fn listed_hash_reads_the_sha256_file() {
    let f = "SHA256 (bsd) = aa\nSHA256 (install80.img) = bb\n";
    assert_eq!(listed_hash(f, "install80.img").as_deref(), Some("bb"));
    assert_eq!(listed_hash(f, "install8.img"), None);
}

#[test]
fn scenario_files_and_expected_file_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ids = Vec::new();
    for set in SETS {
        let text = fs::read_to_string(root.join(DATA_DIR).join(format!("{set}.scn"))).unwrap();
        let steps = parse(set, &text).unwrap();
        assert!(!steps.is_empty(), "{set}");
        ids.extend(steps.into_iter().map(|s| (s.id, s.cmd)));
    }
    let text = fs::read_to_string(root.join(EXPECTED_FILE)).unwrap();
    let e: ExpectedFile = toml::from_str(&text).unwrap();
    for d in &e.differences {
        assert!(
            ids.iter().any(|(id, cmd)| *id == d.step && *cmd == d.cmd),
            "expected.toml names {} / {:?}, which no scenario has",
            d.step,
            d.cmd
        );
        assert!(!d.reason.trim().is_empty());
    }
}

#[test]
fn parse_steps() {
    let t = "# c\n## one\n! mkdir x\n$ ls x\n## two\n?  true\n$[dates,inodes] ls -li\n";
    let s = parse("fs", t).unwrap();
    assert_eq!(s.len(), 4);
    assert_eq!(s[0].id, "fs/one#1");
    assert_eq!(s[0].check, Check::Setup);
    assert_eq!(s[1].id, "fs/one#2");
    assert_eq!(s[2].id, "fs/two#1");
    assert_eq!(s[2].check, Check::Status);
    assert_eq!(s[2].cmd, "true");
    assert_eq!(s[3].norms, vec![Norm::Dates, Norm::Inodes]);
    assert!(parse("fs", "$ ls\n").is_err());
    assert!(parse("fs", "## a\n% ls\n").is_err());
    assert!(parse("fs", "## a\n$[bogus] ls\n").is_err());
}

#[test]
fn script_and_outcomes_round_trip() {
    let steps = parse("x", "## a\n$ echo hi\n? false\n").unwrap();
    let s = script("D=sd0\n", &steps);
    assert!(s.starts_with("D=sd0\necho \"@@HOST $(hostname)\"\n"));
    assert!(s.contains("echo \"@@B 1\"\n{ echo hi\n} </dev/null 2>&1\necho \"@@E 1 $?\"\n"));
    assert!(s.ends_with("echo \"@@DONE\"\n"));
    // What the console shows: the typed command echoed, then the output.
    let t =
        "# sh /tmp/x.sh\r\n@@HOST box\r\n@@B 1\r\nhi\r\n@@E 1 0\r\n@@B 2\r\n@@E 2 1\r\n@@DONE\r\n";
    let (host, o) = outcomes(t, 2);
    assert_eq!(host, "box");
    assert_eq!(o[0].lines, vec!["hi".to_string()]);
    assert_eq!(o[0].status, Some(0));
    assert_eq!(o[1].status, Some(1));
    let (_, o) = outcomes("@@B 1\nhalf", 2);
    assert_eq!(o[0].status, None);
    assert_eq!(o[1], Outcome::default());
}

#[test]
fn normalizers() {
    let s = Subst {
        host: "openbsd".into(),
        disk: "sd1".into(),
    };
    let l = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        normalize(
            &l(&[
                "openbsd# newfs /dev/rsd1a",
                "difftest[4242]: pledge \"rpath\""
            ]),
            &s,
            &[]
        ),
        l(&[
            "<host># newfs /dev/rsdXa",
            "difftest[PID]: pledge \"rpath\""
        ])
    );
    assert_eq!(
        normalize(
            &l(&[
                "-rw-r--r--  1 root  wheel  6 Oct  5 02:21 f",
                "drwx  2 root  wheel  512 Jan 12  2025 d"
            ]),
            &s,
            &[Norm::Dates]
        ),
        l(&[
            "-rw-r--r--  1 root  wheel  6 <date> f",
            "drwx  2 root  wheel  512 <date> d"
        ])
    );
    assert_eq!(
        normalize(
            &l(&["  812 a", "  77 b", "812 c", "x 9"]),
            &s,
            &[Norm::Inodes]
        ),
        l(&["  #1 a", "  #2 b", "#1 c", "x 9"])
    );
    assert_eq!(
        normalize(&l(&["a 12 b 3"]), &s, &[Norm::Numbers]),
        l(&["a N b N"])
    );
}

fn out(lines: &[&str], st: i32) -> Outcome {
    Outcome {
        lines: lines.iter().map(|x| x.to_string()).collect(),
        status: Some(st),
    }
}

#[test]
fn compare_counts_expected_unexpected_and_stale() {
    let steps = parse("x", "## a\n! setup\n$ one\n$ two\n? three\n$ four\n").unwrap();
    let os = Subst {
        host: "openbsd".into(),
        disk: "sd1".into(),
    };
    let es = Subst {
        host: "Amnesiac".into(),
        disk: "sd0".into(),
    };
    let o = vec![
        out(&["junk"], 1),
        out(&["openbsd ok"], 0),
        out(&["OpenBSD"], 0),
        out(&["x"], 0),
        out(&["same"], 0),
    ];
    let e = vec![
        out(&[], 0),
        out(&["Amnesiac ok"], 0),
        out(&["EmiBSD"], 0),
        out(&["y"], 0),
        out(&["other"], 0),
    ];
    let exp = vec![
        Expected {
            step: "x/a#3".into(),
            cmd: "two".into(),
            arch: None,
            reason: "branding".into(),
        },
        Expected {
            step: "x/a#4".into(),
            cmd: "three".into(),
            arch: Some("arm64".into()),
            reason: "arm64 only".into(),
        },
    ];
    let r = compare("amd64", &steps, (&os, &o), (&es, &e), &exp);
    assert_eq!(r.compared, 4);
    assert_eq!(r.equal, 2); // #2 after the host name, #4 by status only
    assert_eq!(r.expected.len(), 1);
    assert_eq!(r.unexpected.len(), 1);
    assert!(r.unexpected[0].contains("openbsd: same"));
    assert!(r.stale.is_empty());
    assert!(!r.passed());
    // On arm64 the #4 entry applies, but #4 is equal: stale.
    let r = compare("arm64", &steps, (&os, &o), (&es, &e), &exp);
    assert_eq!(r.stale.len(), 1);
}

#[test]
fn http_request_paths_stay_inside() {
    let d = Path::new("/srv");
    assert_eq!(
        http::request_path(d, "GET /install.conf?path=8.0/amd64 HTTP/1.1"),
        Some(PathBuf::from("/srv/install.conf"))
    );
    assert_eq!(http::request_path(d, "GET /../etc/passwd HTTP/1.1"), None);
    assert_eq!(http::request_path(d, "POST /x HTTP/1.1"), None);
    assert_eq!(http::request_path(d, "GET / HTTP/1.1"), None);
}
