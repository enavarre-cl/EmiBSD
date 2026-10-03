//! Host tests for task queues: the worklist bookkeeping without worker threads.
//! `taskq_next_work` is called directly, as `taskq_thread` would; no test makes it sleep.

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::intr::IPL_NONE;
use crate::sys::task::task_pending;

fn nothing(_arg: *mut c_void) {}

fn arg(n: usize) -> *mut c_void {
    ptr::without_provenance_mut(n)
}

#[test]
fn task_set_fills_a_zeroed_task_like_the_initializer() {
    static T: Task = Task::zeroed();
    assert!(T.t_func.get().is_none());
    task_set(&T, nothing, arg(5));
    assert!(T.t_func.get().is_some());
    assert_eq!(T.t_arg.get(), arg(5));
    assert!(!task_pending(&T));

    let init = Task::new(nothing, arg(7));
    assert!(init.t_func.get().is_some());
    assert_eq!(init.t_arg.get(), arg(7));
    assert!(!task_pending(&init));
}

#[test]
fn add_and_del_keep_the_flag_and_the_worklist_in_step() {
    static TQ: Taskq = Taskq::new(b"tqtest", 1, IPL_NONE, 0);
    static A: Task = Task::new(nothing, ptr::null_mut());
    static B: Task = Task::new(nothing, ptr::null_mut());

    assert!(task_add(&TQ, &A));
    assert!(task_pending(&A));
    assert!(!task_add(&TQ, &A), "already pending");
    assert!(task_add(&TQ, &B));
    assert_eq!(TQ.tq_worklist.iter().count(), 2);

    assert!(task_del(&TQ, &A));
    assert!(!task_pending(&A));
    assert!(!task_del(&TQ, &A), "no longer pending");
    assert!(TQ.tq_worklist.first().is_some_and(|t| ptr::eq(t, &B)));

    assert!(task_del(&TQ, &B));
    assert!(TQ.tq_worklist.is_empty());
}

#[test]
fn next_work_hands_out_copies_in_order() {
    static TQ: Taskq = Taskq::new(b"tqfifo", 1, IPL_NONE, 0);
    static T1: Task = Task::zeroed();
    static T2: Task = Task::zeroed();
    static T3: Task = Task::zeroed();
    TQ.tq_state.set(TqState::Running);
    for (n, t) in [&T1, &T2, &T3].into_iter().enumerate() {
        task_set(t, nothing, arg(n + 1));
        assert!(task_add(&TQ, t));
    }
    assert!(task_del(&TQ, &T2), "a deleted task is not handed out");

    let w = taskq_next_work(&TQ).expect("T1");
    assert_eq!(w.t_arg.get(), arg(1));
    assert!(!task_pending(&w), "the copy is not on any queue");
    assert!(!task_pending(&T1), "taken off the worklist");
    assert!(task_add(&TQ, &T1), "may be queued again at once");

    // A destroyed queue still drains its worklist before the thread is told to stop.
    TQ.tq_state.set(TqState::Destroyed);
    assert_eq!(taskq_next_work(&TQ).expect("T3").t_arg.get(), arg(3));
    assert_eq!(taskq_next_work(&TQ).expect("T1 again").t_arg.get(), arg(1));
    assert!(taskq_next_work(&TQ).is_none());
}

#[test]
fn next_work_on_an_idle_queue_that_is_not_running_is_none() {
    static TQ: Taskq = Taskq::new(b"tqidle", 1, IPL_NONE, 0);
    assert!(taskq_next_work(&TQ).is_none(), "TQ_S_CREATED and empty");
}

#[test]
fn destroy_before_the_threads_leaves_the_free_to_taskq_create_thread() {
    let _g = setup_real_memory();
    let p = malloc(size_of::<Taskq>(), M_DEVBUF, M_WAITOK)
        .expect("a block")
        .cast::<Taskq>();
    // SAFETY: a fresh block of the right size, as in `taskq_create`.
    unsafe { p.as_ptr().write(Taskq::new(b"tqdead", 1, IPL_NONE, 0)) };

    // SAFETY: `p` is a live queue, destroyed once.
    unsafe { taskq_destroy(p) };
    // SAFETY: a queue in TQ_S_CREATED is not freed by `taskq_destroy`.
    assert_eq!(unsafe { p.as_ref() }.tq_state.get(), TqState::Destroyed);

    // The deferred creation sees TQ_S_DESTROYED and frees the queue instead of forking.
    taskq_create_thread(p.as_ptr().cast());
}

#[test]
fn the_system_queues_start_created_with_one_thread() {
    for (tq, name, flags) in [
        (SYSTQ, b"systq".as_slice(), 0),
        (SYSTQMP, b"systqmp", TASKQ_MPSAFE),
    ] {
        assert_eq!(tq.tq_name, name);
        assert_eq!(tq.tq_nthreads, 1);
        assert_eq!(tq.tq_flags, flags);
        assert_eq!(tq.tq_mtx.mtx_wantipl.get(), IPL_HIGH);
    }
}
