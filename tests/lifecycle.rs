// Application lifecycle phase tests. A test process has no terminal, so
// `drive_session` is driven with a substitute session token whose `Drop` marks
// the teardown point. That keeps "Exit runs after the terminal is restored"
// observable without a TTY.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use icmd::__private::{PhaseRunner, drive_session, drive_session_with, run_session};
use icmd::{
    AppHandle, AppLifecycle, AppPhase, AppSession, Component, ComponentContext, ExitReason, Node,
    Props, RenderError, RuntimeConfig, Size,
};

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, entry: impl Into<String>) {
        self.0.lock().expect("log poisoned").push(entry.into());
    }

    fn entries(&self) -> Vec<String> {
        self.0.lock().expect("log poisoned").clone()
    }
}

// Stands in for `TerminalSession`: dropping it is the teardown point.
struct Token(Log);

impl Drop for Token {
    fn drop(&mut self) {
        self.0.push("teardown");
    }
}

// Every phase runs once, in order, and Exit runs after the session token has
// been dropped (which is where terminal teardown happens in production). Ready
// runs on the first presented frame and never again.
#[test]
fn phases_run_once_in_order_and_exit_runs_after_teardown() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_boot({
            let log = log.clone();
            move |_| log.push("boot")
        })
        .on_mount({
            let log = log.clone();
            move |_| log.push("mount")
        })
        .on_ready({
            let log = log.clone();
            move |_| log.push("ready")
        })
        .on_unmount({
            let log = log.clone();
            move |_| log.push("unmount")
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let enter_log = log.clone();
    let body_log = log.clone();
    let result = drive_session(
        lifecycle,
        Size::new(20, 5),
        move || Ok(Token(enter_log)),
        move |runner, _token| {
            body_log.push("frame");
            assert!(runner.note_frame_presented().is_none());
            // A second frame must not re-run Ready.
            assert!(runner.note_frame_presented().is_none());
            Ok(())
        },
    );
    result.expect("a session with no error succeeds");
    assert_eq!(
        log.entries(),
        vec![
            "boot", "mount", "frame", "ready", "unmount", "teardown", "exit"
        ]
    );
}

// Startup phases run in registration order; teardown phases run in reverse, so
// the last thing acquired is the first thing released.
#[test]
fn startup_phases_are_fifo_and_teardown_phases_are_lifo() {
    let log = Log::default();
    let mut lifecycle = AppLifecycle::new();
    for name in ["a", "b", "c"] {
        let boot = log.clone();
        lifecycle = lifecycle.on_boot(move |_| boot.push(format!("boot-{name}")));
        let mount = log.clone();
        lifecycle = lifecycle.on_mount(move |_| mount.push(format!("mount-{name}")));
        let unmount = log.clone();
        lifecycle = lifecycle.on_unmount(move |_| unmount.push(format!("unmount-{name}")));
        let exit = log.clone();
        lifecycle = lifecycle.on_exit(move |_| exit.push(format!("exit-{name}")));
    }
    drive_session(lifecycle, Size::new(2, 2), || Ok(()), |_, _| Ok(()))
        .expect("a session with no error succeeds");
    assert_eq!(
        log.entries(),
        [
            "boot-a",
            "boot-b",
            "boot-c",
            "mount-a",
            "mount-b",
            "mount-c",
            // Reverse registration order.
            "unmount-c",
            "unmount-b",
            "unmount-a",
            "exit-c",
            "exit-b",
            "exit-a",
        ]
    );
}

// A Boot fault means the terminal was never acquired, so Mount and Unmount do
// not apply. Exit still runs, which is what lets an application release a
// resource it opened during Boot.
#[test]
fn a_boot_fault_skips_mount_and_unmount_but_runs_exit() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_boot(|_| panic!("boot hook fails"))
        .on_mount({
            let log = log.clone();
            move |_| log.push("mount")
        })
        .on_unmount({
            let log = log.clone();
            move |_| log.push("unmount")
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let result = drive_session(
        lifecycle,
        Size::new(4, 2),
        || -> Result<(), RenderError> { panic!("the terminal must not be acquired") },
        |_, _| -> Result<(), RenderError> { panic!("the loop must not run") },
    );
    assert!(
        matches!(
            &result,
            Err(RenderError::Lifecycle {
                phase: AppPhase::Boot,
                index: 0
            })
        ),
        "a boot fault must be reported: {result:?}"
    );
    assert_eq!(log.entries(), vec!["exit"]);
}

// If the terminal cannot be acquired there is nothing to unmount, but Exit still
// runs for anything Boot opened.
#[test]
fn a_terminal_entry_failure_runs_exit_only() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_mount({
            let log = log.clone();
            move |_| log.push("mount")
        })
        .on_unmount({
            let log = log.clone();
            move |_| log.push("unmount")
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let result = drive_session(
        lifecycle,
        Size::new(4, 2),
        || -> Result<(), RenderError> { Err(RenderError::Io(io::Error::other("no tty"))) },
        |_, _| -> Result<(), RenderError> { panic!("the loop must not run") },
    );
    assert!(matches!(&result, Err(RenderError::Io(_))), "{result:?}");
    assert_eq!(log.entries(), vec!["exit"]);
}

// A Mount fault skips the event loop entirely, but the terminal was acquired, so
// the teardown sequence still runs in order.
#[test]
fn a_mount_fault_skips_the_loop_and_still_tears_down() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_mount(|_| panic!("mount hook fails"))
        .on_unmount({
            let log = log.clone();
            move |_| log.push("unmount")
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let enter_log = log.clone();
    let body_ran = Arc::new(AtomicBool::new(false));
    let flag = body_ran.clone();
    let result = drive_session(
        lifecycle,
        Size::new(4, 2),
        move || Ok(Token(enter_log)),
        move |_, _| {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        },
    );
    assert!(
        matches!(
            &result,
            Err(RenderError::Lifecycle {
                phase: AppPhase::Mount,
                index: 0
            })
        ),
        "{result:?}"
    );
    assert!(!body_ran.load(Ordering::Relaxed), "the loop must not run");
    assert_eq!(log.entries(), vec!["unmount", "teardown", "exit"]);
}

// A Ready fault is a session error, not a reason to skip teardown.
#[test]
fn a_ready_fault_stops_the_loop_and_still_runs_unmount_and_exit() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_ready(|_| panic!("ready hook fails"))
        .on_unmount({
            let log = log.clone();
            move |_| log.push("unmount")
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let enter_log = log.clone();
    let result = drive_session(
        lifecycle,
        Size::new(4, 2),
        move || Ok(Token(enter_log)),
        move |runner, _| runner.note_frame_presented().map_or(Ok(()), Err),
    );
    assert!(
        matches!(
            &result,
            Err(RenderError::Lifecycle {
                phase: AppPhase::Ready,
                index: 0
            })
        ),
        "{result:?}"
    );
    assert_eq!(log.entries(), vec!["unmount", "teardown", "exit"]);
}

// A panicking hook is contained: the rest of its phase still runs and the first
// fault is reported with its registration index.
#[test]
fn a_panicking_hook_is_contained_and_the_rest_of_its_phase_runs() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_exit({
            let log = log.clone();
            move |_| {
                log.push("first");
                panic!("exit hook fails");
            }
        })
        .on_exit({
            let log = log.clone();
            move |_| log.push("second")
        });
    let result = drive_session(lifecycle, Size::new(4, 2), || Ok(()), |_, _| Ok(()));
    assert!(
        matches!(
            &result,
            Err(RenderError::Lifecycle {
                phase: AppPhase::Exit,
                index: 0
            })
        ),
        "{result:?}"
    );
    // Exit is a teardown phase, so the later-registered hook runs first; the
    // panicking hook still ran after it.
    assert_eq!(log.entries(), vec!["second", "first"]);
}

// The error that stopped the session is reported even when a teardown hook also
// fails.
#[test]
fn a_setup_error_is_never_masked_by_a_teardown_fault() {
    let log = Log::default();
    let lifecycle = AppLifecycle::new()
        .on_unmount(|_| panic!("unmount hook fails"))
        .on_exit({
            let log = log.clone();
            move |_| log.push("exit")
        });
    let enter_log = log.clone();
    let result = drive_session(
        lifecycle,
        Size::new(4, 2),
        move || Ok(Token(enter_log)),
        |_, _| Err(RenderError::ApplicationCallback("body failed")),
    );
    assert!(
        matches!(
            &result,
            Err(RenderError::ApplicationCallback("body failed"))
        ),
        "{result:?}"
    );
    assert_eq!(log.entries(), vec!["teardown", "exit"]);
}

// The Exit phase can tell why the session ended.
#[test]
fn exit_reason_reports_why_the_session_ended() {
    fn run_and_capture(
        lifecycle: AppLifecycle,
        enter: impl FnOnce() -> Result<(), RenderError>,
        body: impl FnOnce(&mut PhaseRunner, &mut ()) -> Result<(), RenderError>,
    ) -> Option<ExitReason> {
        let seen = Arc::new(Mutex::new(None));
        let sink = seen.clone();
        let lifecycle = lifecycle.on_exit(move |session: &AppSession| {
            *sink.lock().expect("capture poisoned") = Some(session.exit);
        });
        let _ = drive_session(lifecycle, Size::new(2, 2), enter, body);
        seen.lock().expect("capture poisoned").flatten()
    }

    let exit_key = run_and_capture(AppLifecycle::new(), || Ok(()), |_, _| Ok(()));
    assert_eq!(exit_key, Some(ExitReason::ExitKey));

    let closed = run_and_capture(
        AppLifecycle::new(),
        || Ok(()),
        |_, _| Err(RenderError::RuntimeClosed),
    );
    assert_eq!(closed, Some(ExitReason::RuntimeClosed));

    let failed = run_and_capture(
        AppLifecycle::new(),
        || Ok(()),
        |_, _| Err(RenderError::ApplicationCallback("boom")),
    );
    assert_eq!(failed, Some(ExitReason::Failed));

    let aborted = run_and_capture(
        AppLifecycle::new(),
        || Err(RenderError::Io(io::Error::other("no tty"))),
        |_, _| Ok(()),
    );
    assert_eq!(aborted, Some(ExitReason::Aborted));
}

// The viewport and frame count a hook observes come from the runner, and phase
// metadata is part of the observable contract.
#[test]
fn a_session_reports_the_viewport_and_presented_frame_count() {
    let seen = Arc::new(Mutex::new(None));
    let sink = seen.clone();
    let lifecycle = AppLifecycle::new().on_exit(move |session: &AppSession| {
        *sink.lock().expect("capture poisoned") =
            Some((session.phase, session.viewport, session.frames_presented));
    });
    drive_session(
        lifecycle,
        Size::new(7, 3),
        || Ok(()),
        |runner, _| {
            assert!(runner.note_frame_presented().is_none());
            assert!(runner.note_frame_presented().is_none());
            Ok(())
        },
    )
    .expect("a session with no error succeeds");
    let (phase, viewport, frames) = seen.lock().expect("capture poisoned").expect("exit ran");
    assert_eq!(phase, AppPhase::Exit);
    assert_eq!(viewport, Size::new(7, 3));
    assert_eq!(frames, 2);
}

// An empty lifecycle is exactly the historical `render` behavior: no hook runs,
// and a session with no error is a success.
#[test]
fn an_empty_lifecycle_preserves_the_default_render_contract() {
    assert!(AppLifecycle::default().is_empty());
    assert!(!AppLifecycle::new().on_exit(|_| {}).is_empty());

    assert_eq!(
        AppPhase::ALL,
        [
            AppPhase::Boot,
            AppPhase::Mount,
            AppPhase::Ready,
            AppPhase::Unmount,
            AppPhase::Exit,
        ]
    );
    assert!(!AppPhase::Boot.is_teardown());
    assert!(!AppPhase::Mount.is_teardown());
    assert!(!AppPhase::Ready.is_teardown());
    assert!(AppPhase::Unmount.is_teardown());
    assert!(AppPhase::Exit.is_teardown());
    assert_eq!(AppPhase::Exit.to_string(), "exit");
    assert_eq!(ExitReason::ExitKey.to_string(), "exit key");

    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let result = drive_session(
        AppLifecycle::default(),
        Size::new(4, 2),
        || Ok(()),
        move |runner, _| {
            assert!(
                runner.note_frame_presented().is_none(),
                "no Ready hook is registered"
            );
            flag.store(true, Ordering::Relaxed);
            Ok(())
        },
    );
    result.expect("an empty lifecycle is not an error");
    assert!(ran.load(Ordering::Relaxed));
}

// A handle is a plain shared control surface: clones observe one session state,
// so a request made through a clone is visible to the handle the loop holds.
#[test]
fn a_handle_requests_and_cancels_an_exit() {
    let handle = AppHandle::default();
    assert!(!handle.exit_requested());
    assert_eq!(handle.frames_presented(), 0);
    assert!(handle.uptime() < Duration::from_secs(30));

    handle.request_exit();
    assert!(handle.exit_requested());

    let clone = handle.clone();
    clone.cancel_exit_request();
    assert!(
        !handle.exit_requested(),
        "clones share one session state, so a cancel is visible to the original"
    );

    clone.request_exit();
    assert!(
        handle.exit_requested(),
        "a request from a clone reaches the handle the loop holds"
    );
    assert_eq!(ExitReason::Requested.to_string(), "exit requested");
}

// The runner and the application share one handle, so the reason the loop
// recorded survives the default derivation instead of being relabelled.
#[test]
fn a_requested_exit_is_reported_as_requested() {
    let handle = AppHandle::default();
    let seen = Arc::new(Mutex::new(None));
    let sink = seen.clone();
    let lifecycle = AppLifecycle::new().on_exit(move |session: &AppSession| {
        *sink.lock().expect("capture poisoned") = Some(session.exit);
    });

    let request = handle.clone();
    let result = drive_session_with(
        handle.clone(),
        lifecycle,
        Size::new(4, 2),
        || Ok(()),
        move |runner, _| {
            assert!(!runner.handle().exit_requested());
            request.request_exit();
            assert!(
                runner.handle().exit_requested(),
                "the runner and the application share one handle"
            );
            // What `run_session` records when its guard observes a request.
            runner.set_exit(ExitReason::Requested);
            Ok(())
        },
    );
    result.expect("a requested exit is not an error");
    assert!(handle.exit_requested());
    assert_eq!(
        seen.lock().expect("capture poisoned").unwrap(),
        Some(ExitReason::Requested)
    );
}

// End to end through the real event loop, with no terminal involved: a
// component asks the session to stop and the loop stops cleanly, then the
// teardown phases report why. A button press reaches the same guard one step
// later, from its `on_press` handler after commit.
#[test]
fn a_component_requesting_an_exit_stops_the_event_loop() {
    use std::sync::atomic::AtomicUsize;

    fn quitter(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
        let exit = cx.use_handle();
        // Requests the exit at commit time, which is where an event handler's
        // request is observed before the next loop turn.
        cx.use_mount_effect(move || exit.request_exit());
        icmd::text("bye")
    }

    let handle = AppHandle::default();
    let seen = Arc::new(Mutex::new(None));
    let sink = seen.clone();
    let lifecycle = AppLifecycle::new().on_exit(move |session: &AppSession| {
        *sink.lock().expect("capture poisoned") = Some(session.exit);
    });

    let written = Arc::new(AtomicUsize::new(0));
    let count = written.clone();
    let result = drive_session_with(
        handle.clone(),
        lifecycle,
        Size::new(10, 2),
        || Ok(()),
        move |runner, _| {
            // No terminal: the writer only counts the frames the loop presents.
            run_session(
                quitter.apply(()),
                &RuntimeConfig::default(),
                runner,
                &mut |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
        },
    );

    result.expect("a requested exit stops the loop without an error");
    assert_eq!(
        seen.lock().expect("capture poisoned").unwrap(),
        Some(ExitReason::Requested),
        "the loop's reason survives and the Exit phase reports it"
    );
    assert!(
        written.load(Ordering::Relaxed) >= 1,
        "the first frame was presented before the exit"
    );
    assert!(handle.frames_presented() >= 1);
}
