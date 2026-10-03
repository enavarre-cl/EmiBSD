use super::*;

use crate::sys::proc::Pgrp;
use crate::sys::signal::{SIGEMT, SIGINFO, SIGIO, SIGQUIT, SIGTHR, SIGTRAP, SIGURG, SIGWINCH};
use crate::sys::signalvar::SA_CANTMASK;

/// A process with one thread, its own `sigacts` and a process group, laid out by the test
/// (nothing here is linked into the kernel's lists).
struct World {
    pr: Process,
    p: Proc,
    ps: Sigacts,
    pg: Pgrp,
}

impl World {
    fn new(pid: Pid) -> std::boxed::Box<Self> {
        let w = std::boxed::Box::new(World {
            pr: Process::new(),
            p: Proc::new(),
            ps: Sigacts::new(),
            pg: Pgrp::new(),
        });
        w.pr.ps_pid.set(pid);
        w.pr.ps_sigacts.set(&w.ps);
        w.pg.pg_jobc.set(1);
        w.pr.ps_pgrp.set(&w.pg);
        w.p.p_p.set(&w.pr);
        w
    }
}

#[test]
fn sigprop_is_the_c_table() {
    assert_eq!(SIGPROP.len(), NSIG as usize);
    assert_eq!(SIGPROP[0], 0);
    for sig in [
        SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGEMT, SIGFPE, SIGBUS, SIGSEGV, SIGSYS,
    ] {
        assert_eq!(SIGPROP[sig as usize], SA_KILL | SA_CORE, "signal {sig}");
    }
    for sig in [SIGURG, SIGCHLD, SIGIO, SIGWINCH, SIGINFO, SIGTHR] {
        assert_eq!(SIGPROP[sig as usize], SA_IGNORE, "signal {sig}");
    }
    assert_eq!(SIGPROP[SIGCONT as usize], SA_IGNORE | SA_CONT);
    assert_eq!(SIGPROP[SIGSTOP as usize], SA_STOP);
    for sig in [SIGTSTP, SIGTTIN, SIGTTOU] {
        assert_eq!(SIGPROP[sig as usize], SA_STOP | SA_TTYSTOP, "signal {sig}");
    }
    // The stop signals are exactly STOPSIGMASK; no signal is SA_CANTMASK in this table.
    let stops = (1..NSIG)
        .filter(|&s| SIGPROP[s as usize] & SA_STOP != 0)
        .fold(0, |m, s| m | sigmask(s));
    assert_eq!(stops, STOPSIGMASK);
    assert!(SIGPROP.iter().all(|&p| p & SA_CANTMASK == 0));
}

#[test]
fn ffs_counts_from_one() {
    assert_eq!(ffs(0), 0);
    assert_eq!(ffs(1), 1);
    assert_eq!(ffs(sigmask(SIGUSR1) | sigmask(SIGUSR2)), SIGUSR1);
    assert_eq!(ffs(0x8000_0000), 32);
}

#[test]
fn initsiginfo_fills_user_and_fault_members() {
    let si = initsiginfo(SIGUSR1, 0, SI_USER, Sigval::from_int(7));
    assert_eq!((si.si_signo, si.si_code), (SIGUSR1, SI_USER));
    assert_eq!(si.si_value().sival_int(), 7);

    let si = initsiginfo(SIGSEGV, 14, 2, Sigval::from_ptr(0x1000));
    assert_eq!((si.si_signo, si.si_code), (SIGSEGV, 2));
    assert_eq!(si.si_addr(), 0x1000);
    assert_eq!(si.si_trapno(), 14);

    // Kernel codes of other signals carry nothing more.
    let si = initsiginfo(SIGXFSZ, 3, 1, Sigval::from_ptr(0x1000));
    assert_eq!(si.si_addr(), 0);
}

#[test]
fn siginit_ignores_what_is_ignored_by_default_but_sigcont() {
    let ps = Sigacts::new();
    siginit(&ps);
    let ignored = ps.ps_sigignore.get();
    assert_ne!(ignored & sigmask(SIGCHLD), 0);
    assert_ne!(ignored & sigmask(SIGURG), 0);
    assert_eq!(ignored & sigmask(SIGCONT), 0);
    assert_eq!(ignored & sigmask(SIGKILL), 0);
    assert_eq!(
        ps.ps_sigflags.load(Ordering::Relaxed),
        SAS_NOCLDWAIT | SAS_NOCLDSTOP
    );
}

#[test]
fn setsigvec_records_the_action_and_execsigs_resets_it() {
    let w = World::new(5);
    let ps = &w.ps;

    let mut sa = Sigaction {
        sa_handler: 0x4000,
        sa_mask: sigmask(SIGHUP) | sigmask(SIGKILL),
        sa_flags: SA_RESTART | SA_SIGINFO,
    };
    setsigvec(&w.p, SIGUSR1, &mut sa);
    let bit = sigmask(SIGUSR1);
    assert_eq!(ps.ps_sigact[SIGUSR1 as usize].get(), 0x4000);
    // The signal blocks itself (no SA_NODEFER); SIGKILL can never be masked.
    assert_eq!(
        ps.ps_catchmask[SIGUSR1 as usize].get(),
        sigmask(SIGHUP) | bit
    );
    assert_ne!(ps.ps_sigcatch.get() & bit, 0);
    assert_ne!(ps.ps_siginfo.get() & bit, 0);
    assert_eq!(ps.ps_sigintr.get() & bit, 0);
    assert_eq!(ps.ps_sigignore.get() & bit, 0);

    // SIG_IGN for SIGCHLD ignores it and asks for no zombies (init does not exist here).
    w.pr.ps_siglist.store(sigmask(SIGCHLD), Ordering::Relaxed);
    let mut ign = Sigaction {
        sa_handler: SIG_IGN,
        ..Sigaction::default()
    };
    setsigvec(&w.p, SIGCHLD, &mut ign);
    assert_ne!(ps.ps_sigignore.get() & sigmask(SIGCHLD), 0);
    assert_eq!(w.pr.ps_siglist.load(Ordering::Relaxed), 0);
    assert_ne!(ps.ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDWAIT, 0);

    execsigs(&w.p);
    assert_eq!(ps.ps_sigact[SIGUSR1 as usize].get(), SIG_DFL);
    assert_eq!(ps.ps_sigcatch.get(), 0);
    assert_eq!(ps.ps_sigact[SIGCHLD as usize].get(), SIG_DFL);
    assert_eq!(ps.ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDWAIT, 0);
    assert_eq!(w.p.p_sigstk.get().ss_flags, SS_DISABLE);
}

#[test]
fn cursig_picks_the_lowest_deliverable_signal() {
    let w = World::new(5);
    siginit(&w.ps);
    let mut ctx = Sigctx::default();

    // Nothing pending.
    assert_eq!(cursig(&w.p, &mut ctx, false), 0);

    // A caught SIGUSR2, a masked SIGHUP and an ignored-by-default SIGCHLD.
    let mut sa = Sigaction {
        sa_handler: 0x4000,
        ..Sigaction::default()
    };
    setsigvec(&w.p, SIGUSR2, &mut sa);
    w.p.p_sigmask.set(sigmask(SIGHUP));
    w.pr.ps_siglist.store(
        sigmask(SIGHUP) | sigmask(SIGUSR2) | sigmask(SIGCHLD),
        Ordering::Relaxed,
    );
    // SIGCHLD is pending although ignored (it was posted before), so cursig drops it and
    // returns the caught SIGUSR2; the masked SIGHUP stays pending.
    assert_eq!(cursig(&w.p, &mut ctx, false), SIGUSR2);
    assert_eq!(ctx.sig_action, 0x4000);
    assert!(ctx.sig_catch);
    // The taken signal moved to the thread's list for postsig.
    assert_eq!(w.p.p_siglist.load(Ordering::Relaxed), sigmask(SIGUSR2));
    assert_eq!(w.pr.ps_siglist.load(Ordering::Relaxed), sigmask(SIGHUP));

    // Unmasking SIGHUP (default action: kill) makes it the next one, ahead of SIGUSR2.
    w.p.p_sigmask.set(0);
    assert_eq!(cursig(&w.p, &mut ctx, false), SIGHUP);
    assert_eq!(ctx.sig_action, SIG_DFL);
    assert_eq!(sigpending(&w.p), sigmask(SIGHUP) | sigmask(SIGUSR2));
}

#[test]
fn cursig_ignores_default_actions_for_init() {
    let w = World::new(1);
    let mut ctx = Sigctx::default();
    w.pr.ps_siglist.store(sigmask(SIGTERM), Ordering::Relaxed);
    assert_eq!(cursig(&w.p, &mut ctx, false), 0);
    assert_eq!(sigpending(&w.p), 0);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn sigprop_matches_kern_sig_c() {
    let path = crate::reftest::openbsd_src().join("sys/kern/kern_sig.c");
    let text = std::fs::read_to_string(&path).expect("kern_sig.c");
    let start = text.find("const int sigprop[NSIG] = {").expect("sigprop");
    let body = &text[start..];
    let body = &body[body.find('{').expect("{") + 1..body.find("};").expect("};")];
    let rows: std::vec::Vec<i32> = body
        .lines()
        .filter_map(|l| {
            let v = l.split("/*").next()?.trim().trim_end_matches(',');
            if v.is_empty() {
                return None;
            }
            Some(v.split('|').fold(0, |m, f| {
                m | match f.trim() {
                    "0" => 0,
                    "SA_KILL" => SA_KILL,
                    "SA_CORE" => SA_CORE,
                    "SA_STOP" => SA_STOP,
                    "SA_TTYSTOP" => SA_TTYSTOP,
                    "SA_IGNORE" => SA_IGNORE,
                    "SA_CONT" => SA_CONT,
                    other => panic!("unknown property {other}"),
                }
            }))
        })
        .collect();
    assert_eq!(rows.as_slice(), &SIGPROP[..]);
}
