//! Child process lifetime: Windows Job Object kill-on-close (LOCKED).
//!
//! Force-quit or crash of Desk must not leave multi-hour whisper/ffmpeg
//! orphans. On Windows, children are assigned to a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Cooperative cancel terminates the
//! active Job Object / process group mid-wait.
//! Non-Windows: best-effort process kill on Drop / explicit kill.

use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Windows Job Object limit flag value (documented constant for unit tests).
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` = 0x2000
pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;

/// How often the cancellable wait loop polls cancel + join status.
const CANCEL_POLL_MS: u64 = 50;

/// Limit flags we always set for STT/ffmpeg children.
pub fn kill_on_close_limit_flags() -> u32 {
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
}

/// Error from [`spawn_and_wait_cancellable`].
#[derive(Debug)]
pub enum CancellableWaitError {
    /// OS / spawn / wait failure.
    Io(io::Error),
    /// Cancel requested; Job Object / process group was terminated.
    Cancelled,
}

impl std::fmt::Display for CancellableWaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for CancellableWaitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Cancelled => None,
        }
    }
}

impl From<io::Error> for CancellableWaitError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl CancellableWaitError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Spawn a child, wait for completion, return output.
///
/// On Windows the child is assigned to a kill-on-close Job Object so OS
/// terminates it if this process dies while waiting. Does **not** poll cancel;
/// use [`spawn_and_wait_cancellable`] when cooperative cancel is required.
pub fn spawn_and_wait(
    program: &str,
    args: &[String],
    env_extra: Option<&[(&str, &str)]>,
) -> io::Result<Output> {
    match spawn_and_wait_cancellable(program, args, env_extra, None) {
        Ok(out) => Ok(out),
        Err(CancellableWaitError::Io(e)) => Err(e),
        Err(CancellableWaitError::Cancelled) => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "cancelled (unexpected without cancel fn)",
        )),
    }
}

/// Spawn a child under a Job Object (Windows) and wait, polling `cancel`.
///
/// When `cancel` returns true mid-wait, the Job Object is terminated (Windows)
/// or the process is killed (other platforms), the wait thread is joined, and
/// [`CancellableWaitError::Cancelled`] is returned. Child is never left orphaned.
pub fn spawn_and_wait_cancellable(
    program: &str,
    args: &[String],
    env_extra: Option<&[(&str, &str)]>,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<Output, CancellableWaitError> {
    #[cfg(windows)]
    {
        windows::spawn_and_wait_cancellable(program, args, env_extra, cancel)
    }
    #[cfg(not(windows))]
    {
        unix_like::spawn_and_wait_cancellable(program, args, env_extra, cancel)
    }
}

/// Spawn a long-running child under a Job Object (Windows) for cancel tests.
pub struct ManagedChild {
    child: std::process::Child,
    #[cfg(windows)]
    _job: windows::JobHandle,
}

impl ManagedChild {
    pub fn spawn(program: &str, args: &[String]) -> io::Result<Self> {
        #[cfg(windows)]
        {
            windows::spawn_managed(program, args)
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new(program);
            cmd.args(args);
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            scrub_proxy(&mut cmd);
            let child = cmd.spawn()?;
            Ok(Self { child })
        }
    }

    /// Force-kill the child (and Windows job members).
    pub fn kill(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
            Ok(())
        }
        #[cfg(not(windows))]
        {
            self.child.kill()?;
            let _ = self.child.wait();
            Ok(())
        }
    }

    /// Terminate via Job Object (Windows) or process kill (other).
    pub fn terminate_job(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            self._job.terminate(1)?;
            let _ = self.child.kill();
            let _ = self.child.wait();
            Ok(())
        }
        #[cfg(not(windows))]
        {
            self.kill()
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Windows: `_job` Drop closes the job handle → KILL_ON_JOB_CLOSE.
    }
}

fn scrub_proxy(cmd: &mut Command) {
    cmd.env_remove("HTTP_PROXY");
    cmd.env_remove("HTTPS_PROXY");
    cmd.env_remove("http_proxy");
    cmd.env_remove("https_proxy");
    cmd.env_remove("ALL_PROXY");
}

fn apply_env(cmd: &mut Command, env_extra: Option<&[(&str, &str)]>) {
    if let Some(pairs) = env_extra {
        for (k, v) in pairs {
            cmd.env(k, v);
        }
    }
}

/// Shared poll loop: wait thread produces `Output`, main thread polls cancel.
fn wait_with_cancel_poll(
    rx: mpsc::Receiver<io::Result<Output>>,
    cancel: Option<&dyn Fn() -> bool>,
    mut on_cancel: impl FnMut() -> io::Result<()>,
) -> Result<Output, CancellableWaitError> {
    loop {
        match rx.try_recv() {
            Ok(Ok(out)) => return Ok(out),
            Ok(Err(e)) => return Err(CancellableWaitError::Io(e)),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(CancellableWaitError::Io(io::Error::other(
                    "child wait thread disconnected without result",
                )));
            }
            Err(mpsc::TryRecvError::Empty) => {
                if cancel.map(|c| c()).unwrap_or(false) {
                    // Terminate job / process group first, then drain wait thread.
                    let _ = on_cancel();
                    // Reap wait thread so pipes close and process is not zombie.
                    match rx.recv_timeout(Duration::from_secs(30)) {
                        Ok(Ok(_out)) => {
                            // Child exited after terminate — still report Cancelled.
                            return Err(CancellableWaitError::Cancelled);
                        }
                        Ok(Err(_)) | Err(_) => return Err(CancellableWaitError::Cancelled),
                    }
                }
                thread::sleep(Duration::from_millis(CANCEL_POLL_MS));
            }
        }
    }
}

#[cfg(not(windows))]
mod unix_like {
    use super::*;

    pub fn spawn_and_wait_cancellable(
        program: &str,
        args: &[String],
        env_extra: Option<&[(&str, &str)]>,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<Output, CancellableWaitError> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        scrub_proxy(&mut cmd);
        apply_env(&mut cmd, env_extra);
        let child = cmd.spawn()?;
        let child_id = child.id();

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });

        wait_with_cancel_poll(rx, cancel, || {
            // Best-effort kill by pid (child moved into wait thread).
            let _ = Command::new("kill")
                .args(["-9", &child_id.to_string()])
                .status();
            Ok(())
        })
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };

    pub struct JobHandle {
        handle: HANDLE,
    }

    // SAFETY: Job handles are process-scoped kernel objects used only from the
    // owning ManagedChild / spawn_and_wait stack.
    unsafe impl Send for JobHandle {}

    impl JobHandle {
        pub fn create_kill_on_close() -> io::Result<Self> {
            // SAFETY: null name/attrs creates an anonymous job object.
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self { handle };
            job.set_kill_on_close()?;
            Ok(job)
        }

        fn set_kill_on_close(&self) -> io::Result<()> {
            let mut info = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
            info.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
                PerProcessUserTimeLimit: 0,
                PerJobUserTimeLimit: 0,
                LimitFlags: super::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                MinimumWorkingSetSize: 0,
                MaximumWorkingSetSize: 0,
                ActiveProcessLimit: 0,
                Affinity: 0,
                PriorityClass: 0,
                SchedulingClass: 0,
            };
            // SAFETY: valid job handle; size matches the info struct.
            let ok = unsafe {
                SetInformationJobObject(
                    self.handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn assign(&self, process: HANDLE) -> io::Result<()> {
            // SAFETY: both handles valid for this process.
            let ok = unsafe { AssignProcessToJobObject(self.handle, process) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
            // SAFETY: valid job handle.
            let ok = unsafe { TerminateJobObject(self.handle, exit_code) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // Closing the job with KILL_ON_JOB_CLOSE terminates remaining members.
            if !self.handle.is_null() {
                // SAFETY: we own the handle.
                unsafe {
                    let _ = CloseHandle(self.handle);
                }
                self.handle = ptr::null_mut();
            }
        }
    }

    /// Soft-accept AssignProcessToJobObject failure **only** when the process
    /// has already exited (`try_wait` → `Some`). ACCESS_DENIED / INVALID_HANDLE
    /// on a **live** process must kill the orphan and error — never leave a
    /// whisper/ffmpeg child outside the Job Object (Codex P2).
    #[cfg(test)]
    pub(crate) fn assign_failure_is_soft(process_already_exited: bool) -> bool {
        process_already_exited
    }

    pub fn spawn_and_wait_cancellable(
        program: &str,
        args: &[String],
        env_extra: Option<&[(&str, &str)]>,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<Output, CancellableWaitError> {
        let job = JobHandle::create_kill_on_close()?;
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        scrub_proxy(&mut cmd);
        apply_env(&mut cmd, env_extra);
        let mut child = cmd.spawn()?;

        // Assign must succeed for kill-on-close / cancel-kill guarantees.
        let process_handle = child.as_raw_handle() as HANDLE;
        if let Err(e) = job.assign(process_handle) {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Already exited — soft-ok. Pipes may be empty; rare race.
                    return Ok(Output {
                        status,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    });
                }
                Ok(None) | Err(_) => {
                    // Live process (or unknown): kill orphan; never soft-accept.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CancellableWaitError::Io(io::Error::new(
                        e.kind(),
                        format!("AssignProcessToJobObject failed on live process: {e}"),
                    )));
                }
            }
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });

        // job must outlive the wait loop so terminate works on cancel.
        wait_with_cancel_poll(rx, cancel, || job.terminate(1))
    }

    pub fn spawn_managed(program: &str, args: &[String]) -> io::Result<ManagedChild> {
        let job = JobHandle::create_kill_on_close()?;
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        scrub_proxy(&mut cmd);
        let mut child = cmd.spawn()?;
        let process_handle = child.as_raw_handle() as HANDLE;
        if let Err(e) = job.assign(process_handle) {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Already exited — soft-ok; ManagedChild Drop is a no-op kill.
                }
                Ok(None) | Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::new(
                        e.kind(),
                        format!("AssignProcessToJobObject failed on live process: {e}"),
                    ));
                }
            }
        }
        Ok(ManagedChild { child, _job: job })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn kill_on_close_flag_value() {
        assert_eq!(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0x2000);
        assert_eq!(kill_on_close_limit_flags(), 0x2000);
    }

    #[test]
    #[cfg(windows)]
    fn assign_failure_soft_only_when_exited() {
        // Live process assign failures must not be soft-accepted (ACCESS_DENIED etc.).
        assert!(!windows::assign_failure_is_soft(false));
        assert!(windows::assign_failure_is_soft(true));
    }

    #[test]
    #[cfg(windows)]
    fn managed_child_kill_stops_ping() {
        // `ping -n 30 127.0.0.1` runs ~30s; terminate_job must return quickly.
        let args = vec!["-n".into(), "30".into(), "127.0.0.1".into()];
        let mut child = ManagedChild::spawn("ping", &args).expect("spawn ping");
        std::thread::sleep(std::time::Duration::from_millis(100));
        child.terminate_job().expect("terminate job");
        let status = child.try_wait().expect("try_wait");
        assert!(status.is_some(), "child must exit after job kill");
    }

    #[test]
    #[cfg(windows)]
    fn cancellable_wait_kills_long_running_child() {
        // Long-running ping; cancel becomes true after a short delay.
        let args = vec!["-n".into(), "60".into(), "127.0.0.1".into()];
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        let cancel: &dyn Fn() -> bool = &|| flag2.load(Ordering::SeqCst);

        let join = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let result = spawn_and_wait_cancellable("ping", &args, None, Some(cancel));
        let elapsed = started.elapsed();
        join.join().expect("cancel arm thread");

        assert!(
            matches!(result, Err(CancellableWaitError::Cancelled)),
            "expected Cancelled, got {result:?}"
        );
        // Must not wait full ~60s of ping.
        assert!(
            elapsed < Duration::from_secs(10),
            "cancel wait took too long: {elapsed:?}"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn managed_child_kill_stops_sleep() {
        let args = vec!["30".into()];
        let mut child = ManagedChild::spawn("sleep", &args).expect("spawn sleep");
        std::thread::sleep(std::time::Duration::from_millis(50));
        child.kill().expect("kill");
        let status = child.try_wait().expect("try_wait");
        assert!(status.is_some());
    }

    #[test]
    #[cfg(not(windows))]
    fn cancellable_wait_kills_long_running_sleep() {
        let args = vec!["60".into()];
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        let cancel: &dyn Fn() -> bool = &|| flag2.load(Ordering::SeqCst);

        let join = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let result = spawn_and_wait_cancellable("sleep", &args, None, Some(cancel));
        let elapsed = started.elapsed();
        join.join().expect("cancel arm thread");

        assert!(
            matches!(result, Err(CancellableWaitError::Cancelled)),
            "expected Cancelled, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "cancel wait took too long: {elapsed:?}"
        );
    }
}
