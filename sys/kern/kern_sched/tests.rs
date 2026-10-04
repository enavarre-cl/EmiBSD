//! Host tests of `kern/kern_sched.rs`: the CPU sets and the cost estimate.

use super::*;

#[test]
fn cpusets_track_the_host_cpu() {
    let ci = curcpu();
    let set = Cpuset::new();
    assert!(!cpuset_isset(&set, ci));
    cpuset_add(&set, ci);
    assert!(cpuset_isset(&set, ci));
    assert_eq!(cpuset_cardinality(&set), 1);
    let empty = cpuset_complement(&set, &set);
    assert_eq!(cpuset_cardinality(&empty), 0);
    let both = cpuset_intersection(&set, &cpuset_copy(&set));
    assert_eq!(cpuset_cardinality(&both), 1);
    cpuset_del(&set, ci);
    assert!(!cpuset_isset(&set, ci));
}

#[test]
fn cpuset_first_finds_the_registered_cpu() {
    let ci = curcpu();
    cpuset_init_cpu(ci);
    let set = Cpuset::new();
    assert!(cpuset_first(&set).is_none());
    cpuset_add(&set, ci);
    assert!(cpuset_first(&set).is_some_and(|c| ptr::eq(c, ci)));
}

#[test]
fn log2_rounds_down_and_maps_zero_to_zero() {
    assert_eq!(log2(0), 0);
    assert_eq!(log2(1), 0);
    assert_eq!(log2(2), 1);
    assert_eq!(log2(3), 1);
    assert_eq!(log2(1024), 10);
    assert_eq!(log2(u32::MAX), 31);
}

/// A busy, non-primary CPU running priority 50 with nothing queued.
const BUSY: CpuCostInputs = CpuCostInputs {
    idle: false,
    queued: false,
    primary: false,
    usrpri: 50,
    curpriority: 50,
    nrun: 0,
    resident: None,
};

#[test]
fn an_idle_cpu_costs_nothing() {
    let c = CpuCostInputs { idle: true, ..BUSY };
    assert_eq!(cpu_cost(c), 0);
}

#[test]
fn a_busy_cpu_costs_the_priority_gap_plus_one_runnable() {
    assert_eq!(cpu_cost(BUSY), 3);
    // A thread of worse priority (higher number) than the running one costs more.
    let worse = CpuCostInputs { usrpri: 60, ..BUSY };
    assert_eq!(cpu_cost(worse), 13);
    // A better one less: it would preempt.
    let better = CpuCostInputs { usrpri: 40, ..BUSY };
    assert_eq!(cpu_cost(better), -7);
}

#[test]
fn queued_threads_and_the_primary_cpu_add_cost() {
    let queued = CpuCostInputs {
        queued: true,
        nrun: 2,
        ..BUSY
    };
    assert_eq!(cpu_cost(queued), 3 + 2 * 3);
    let primary = CpuCostInputs {
        primary: true,
        ..BUSY
    };
    assert_eq!(cpu_cost(primary), 3 + 3);
}

#[test]
fn a_warm_cache_lowers_the_cost() {
    let warm = CpuCostInputs {
        resident: Some(256),
        ..BUSY
    };
    assert_eq!(cpu_cost(warm), 3 - 8);
    let empty = CpuCostInputs {
        resident: Some(0),
        ..BUSY
    };
    assert_eq!(cpu_cost(empty), 3);
}
