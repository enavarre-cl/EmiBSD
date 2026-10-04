//! Host tests for `<nfs/nfs.h>`: the statistics snapshot, the helpers and, against the C
//! header, the constants.

use std::string::String;

use super::*;

#[test]
fn snapshot_is_in_c_order() {
    let st = Nfsstats::new();
    st.attrcache_hits.store(1, Ordering::Relaxed);
    st.rpccnt[0].store(2, Ordering::Relaxed);
    st.rpccnt[NFS_NPROCS - 1].store(3, Ordering::Relaxed);
    st.rpcretries.store(4, Ordering::Relaxed);
    st.srvvop_writes.store(5, Ordering::Relaxed);
    let w = st.snapshot();
    assert_eq!(w[0], 1);
    assert_eq!(w[16], 2);
    assert_eq!(w[16 + NFS_NPROCS - 1], 3);
    assert_eq!(w[16 + NFS_NPROCS], 4);
    assert_eq!(w[Nfsstats::NWORDS - 1], 5);
}

#[test]
fn server_reply_limits_and_ignored_errors() {
    let mut nd = NfsrvDescript::new();
    assert_eq!(nfs_srvmaxdata(&nd), NFS_V2MAXDATA);
    nd.nd_flag = ND_NFSV3;
    assert_eq!(nfs_srvmaxdata(&nd), NFS_MAXDATA);
    assert!(nfsignore_soerror(0, Errno::ECONNREFUSED));
    assert!(!nfsignore_soerror(0, Errno::EINTR));
    assert!(!nfsignore_soerror(
        i32::from(PR_CONNREQUIRED),
        Errno::ECONNREFUSED
    ));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/nfs/nfs.h");
    let ours: &[(&str, i64)] = &[
        ("NFS_TICKINTVL", NFS_TICKINTVL.into()),
        ("NFS_TIMEOUTMUL", NFS_TIMEOUTMUL.into()),
        ("NFS_MAXREXMIT", NFS_MAXREXMIT.into()),
        ("NFS_RETRANS", NFS_RETRANS.into()),
        ("NFS_MAXGRPS", NFS_MAXGRPS.into()),
        ("NFS_MINATTRTIMO", NFS_MINATTRTIMO.into()),
        ("NFS_MAXATTRTIMO", NFS_MAXATTRTIMO.into()),
        ("NFS_WSIZE", NFS_WSIZE.into()),
        ("NFS_RSIZE", NFS_RSIZE.into()),
        ("NFS_READDIRSIZE", NFS_READDIRSIZE.into()),
        ("NFS_DEFRAHEAD", NFS_DEFRAHEAD.into()),
        ("NFS_MAXRAHEAD", NFS_MAXRAHEAD.into()),
        ("NFS_MAXASYNCDAEMON", NFS_MAXASYNCDAEMON.into()),
        ("NFS_DIRBLKSIZ", NFS_DIRBLKSIZ.into()),
        ("NFS_READDIRBLKSIZ", NFS_READDIRBLKSIZ.into()),
        ("NFSSVC_NFSD", NFSSVC_NFSD.into()),
        ("NFSSVC_ADDSOCK", NFSSVC_ADDSOCK.into()),
        ("NFS_NFSSTATS", NFS_NFSSTATS.into()),
        ("NFS_NIOTHREADS", NFS_NIOTHREADS.into()),
        ("NFS_MAXID", NFS_MAXID.into()),
        ("R_TIMING", R_TIMING.into()),
        ("R_SENT", R_SENT.into()),
        ("R_SOFTTERM", R_SOFTTERM.into()),
        ("R_INTR", R_INTR.into()),
        ("R_SOCKERR", R_SOCKERR.into()),
        ("R_TPRINTFMSG", R_TPRINTFMSG.into()),
        ("R_MUSTRESEND", R_MUSTRESEND.into()),
        ("SLP_VALID", SLP_VALID.into()),
        ("SLP_DOREC", SLP_DOREC.into()),
        ("SLP_NEEDQ", SLP_NEEDQ.into()),
        ("SLP_DISCONN", SLP_DISCONN.into()),
        ("SLP_GETSTREAM", SLP_GETSTREAM.into()),
        ("SLP_LASTFRAG", SLP_LASTFRAG.into()),
        ("SLP_ALLFLAGS", SLP_ALLFLAGS.into()),
        ("NFSD_WAITING", NFSD_WAITING.into()),
        ("NFSD_REQINPROG", NFSD_REQINPROG.into()),
        ("ND_NFSV3", ND_NFSV3.into()),
        ("NFSD_CHECKSLP", NFSD_CHECKSLP.into()),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
    assert_eq!(
        defs.get("NFS_MAX_TIMER").map(String::as_str),
        Some("(NFS_WRITE_TIMER)")
    );
    assert_eq!(
        defs.get("B_INVAFTERWRITE").map(String::as_str),
        Some("B_INVAL")
    );
}
