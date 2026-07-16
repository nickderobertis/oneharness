//! Owned subprocess trees and bounded pipe capture.
//!
//! A harness launcher is not necessarily the workload: npm shims commonly
//! start a native child which can outlive the launcher. Every spawned process is
//! therefore contained as one owned tree (a process group on Unix, a kill-on-
//! close Job Object on Windows). Timeout and streaming teardown terminate that
//! whole tree, reap the direct child, and bound pipe draining so an escaped
//! descendant can never hold the caller forever.

use std::io::{self, Read};
use std::process::{Child, ChildStdin, Command, ExitStatus};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

const TERM_GRACE: Duration = Duration::from_millis(100);
const PIPE_CLOSE_GRACE: Duration = Duration::from_millis(100);
const PIPE_DRAIN_GRACE: Duration = Duration::from_millis(250);

/// A stdout read observed by a streaming caller.
pub(crate) enum PipeEvent {
    Data(Vec<u8>),
    Closed,
    Deadline,
}

/// Captured bytes after the direct child and its owned descendants are done.
pub(crate) struct Finished {
    pub stdout: String,
    pub stderr: String,
}

/// Why the runner is finishing a process.
pub(crate) enum Finish {
    /// The direct child exited by itself.
    Exited,
    /// The run timed out, the stream consumer stopped, or waiting failed.
    Terminate,
}

/// One direct child, all of its descendants, and both output drains.
pub(crate) struct Process {
    child: Option<Child>,
    tree: platform::Tree,
    stdout: PipeDrain,
    stderr: PipeDrain,
}

impl Process {
    /// Spawn `command` into a platform-owned process tree and start draining its
    /// already-piped stdout/stderr. Windows starts suspended, assigns the process
    /// to its Job Object, and only then resumes it, so a fast launcher cannot
    /// create an unowned descendant in the assignment race.
    pub(crate) fn spawn(mut command: Command) -> io::Result<Self> {
        let mut tree = platform::Tree::prepare(&mut command)?;
        let mut child = command.spawn()?;
        if let Err(error) = tree.attach_and_start(&mut child) {
            tree.terminate(&mut child, TERM_GRACE);
            return Err(error);
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "child stdout was not piped")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "child stderr was not piped")
        })?;

        Ok(Self {
            child: Some(child),
            tree,
            stdout: PipeDrain::spawn(stdout),
            stderr: PipeDrain::spawn(stderr),
        })
    }

    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut().and_then(|child| child.stdin.take())
    }

    /// Wait until `deadline` for the direct child. `None` means it did not exit
    /// by the deadline; an error is a wait failure. The deadline check happens
    /// before polling, so output that looks complete never extends the runtime.
    pub(crate) fn wait_until(&mut self, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        self.child
            .as_mut()
            .expect("live process has a child")
            .wait_timeout(deadline - now)
    }

    /// Receive one stdout chunk for incremental parsing while preserving the
    /// exact bytes for the final capture. stderr is opportunistically drained on
    /// every poll; its reader thread can therefore never backpressure the child.
    pub(crate) fn recv_stdout_until(&mut self, deadline: Instant) -> PipeEvent {
        self.stderr.drain_available();
        self.stdout.recv_until(deadline)
    }

    /// Finish the owned tree and return its best-effort output. Pipe drain is
    /// bounded even if a process somehow escaped containment and kept an
    /// inherited handle open.
    pub(crate) fn finish(mut self, reason: Finish) -> Finished {
        match reason {
            Finish::Exited => {
                let close_deadline = Instant::now() + PIPE_CLOSE_GRACE;
                self.drain_pipes_until(close_deadline);
                if !self.pipes_closed() {
                    self.terminate();
                }
            }
            Finish::Terminate => self.terminate(),
        }

        self.drain_pipes_until(Instant::now() + PIPE_DRAIN_GRACE);
        let child = self.child.take();
        drop(child);
        Finished {
            stdout: self.stdout.take_string(),
            stderr: self.stderr.take_string(),
        }
    }

    fn terminate(&mut self) {
        if let Some(child) = self.child.as_mut() {
            self.tree.terminate(child, TERM_GRACE);
        }
    }

    fn drain_pipes_until(&mut self, deadline: Instant) {
        self.stdout.drain_until(deadline);
        self.stderr.drain_until(deadline);
        self.stdout.drain_available();
        self.stderr.drain_available();
    }

    fn pipes_closed(&self) -> bool {
        self.stdout.closed && self.stderr.closed
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Covers an unwind or any future early return between spawn and finish.
        if let Some(mut child) = self.child.take() {
            self.tree.terminate(&mut child, TERM_GRACE);
        }
    }
}

enum PipeMessage {
    Data(Vec<u8>),
    Closed,
}

struct PipeDrain {
    receiver: Receiver<PipeMessage>,
    bytes: Vec<u8>,
    closed: bool,
}

impl PipeDrain {
    fn spawn<R>(mut reader: R) -> Self
    where
        R: Read + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if sender
                            .send(PipeMessage::Data(chunk[..count].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = sender.send(PipeMessage::Closed);
        });
        Self {
            receiver,
            bytes: Vec::new(),
            closed: false,
        }
    }

    fn recv_until(&mut self, deadline: Instant) -> PipeEvent {
        if self.closed {
            return PipeEvent::Closed;
        }
        let now = Instant::now();
        if now >= deadline {
            return PipeEvent::Deadline;
        }
        match self.receiver.recv_timeout(deadline - now) {
            Ok(PipeMessage::Data(chunk)) => {
                self.bytes.extend_from_slice(&chunk);
                PipeEvent::Data(chunk)
            }
            Ok(PipeMessage::Closed) | Err(RecvTimeoutError::Disconnected) => {
                self.closed = true;
                PipeEvent::Closed
            }
            Err(RecvTimeoutError::Timeout) => PipeEvent::Deadline,
        }
    }

    fn drain_until(&mut self, deadline: Instant) {
        while !self.closed {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match self.receiver.recv_timeout(deadline - now) {
                Ok(PipeMessage::Data(chunk)) => self.bytes.extend_from_slice(&chunk),
                Ok(PipeMessage::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    self.closed = true;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
    }

    fn drain_available(&mut self) {
        while !self.closed {
            match self.receiver.try_recv() {
                Ok(PipeMessage::Data(chunk)) => self.bytes.extend_from_slice(&chunk),
                Ok(PipeMessage::Closed) | Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
    }

    fn take_string(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.bytes)).into_owned()
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::time::Duration;

    use wait_timeout::ChildExt;

    pub(super) struct Tree {
        process_group: Option<libc::pid_t>,
    }

    impl Tree {
        pub(super) fn prepare(command: &mut Command) -> io::Result<Self> {
            // SAFETY: `pre_exec` runs after fork in the child. The closure calls
            // only async-signal-safe `setpgid`, captures nothing, and reports the
            // OS error directly. Making the child its own group leader before
            // exec closes the descendant-escape race.
            unsafe {
                command.pre_exec(|| {
                    if libc::setpgid(0, 0) == -1 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            Ok(Self {
                process_group: None,
            })
        }

        pub(super) fn attach_and_start(&mut self, child: &mut Child) -> io::Result<()> {
            self.process_group = Some(child.id().try_into().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "child PID does not fit pid_t")
            })?);
            Ok(())
        }

        pub(super) fn terminate(&mut self, child: &mut Child, grace: Duration) {
            if let Some(group) = self.process_group {
                signal_group(group, libc::SIGTERM);
                let _ = child.wait_timeout(grace);
                // Always follow with KILL: the direct child may have honored TERM
                // while a descendant ignored it and retained the output pipes.
                signal_group(group, libc::SIGKILL);
            } else {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }

    fn signal_group(group: libc::pid_t, signal: libc::c_int) {
        // SAFETY: a negative PID addresses the process group created above.
        // ESRCH is expected when every member already exited; all failures are
        // best-effort because reaping/drain bounds still guarantee our return.
        unsafe {
            libc::kill(-group, signal);
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::ptr;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
    };

    pub(super) struct Tree {
        job: HANDLE,
    }

    impl Tree {
        pub(super) fn prepare(command: &mut Command) -> io::Result<Self> {
            // SAFETY: null security/name pointers request an unnamed Job Object
            // with default security. The returned handle is owned by `Tree`.
            let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` has the exact structure/size required by this
            // information class and remains alive for the duration of the call.
            let configured = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if configured == 0 {
                let error = io::Error::last_os_error();
                // SAFETY: `job` is a live handle owned by this function.
                unsafe { CloseHandle(job) };
                return Err(error);
            }

            // `std::process::Command` ORs these flags into its CreateProcessW
            // call. The primary thread cannot spawn children before assignment.
            command.creation_flags(CREATE_SUSPENDED);
            Ok(Self { job })
        }

        pub(super) fn attach_and_start(&mut self, child: &mut Child) -> io::Result<()> {
            let process = child.as_raw_handle() as HANDLE;
            // SAFETY: both handles are live; the process is still suspended.
            if unsafe { AssignProcessToJobObject(self.job, process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_primary_thread(child.id())
        }

        pub(super) fn terminate(&mut self, child: &mut Child, _grace: Duration) {
            // Windows has no general graceful signal for an arbitrary console
            // subtree. Terminating the Job atomically covers every descendant.
            // SAFETY: `self.job` remains live until `Drop`.
            unsafe { TerminateJobObject(self.job, 1) };
            let _ = child.wait();
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            // KILL_ON_JOB_CLOSE is the final containment backstop. At normal
            // finish the processes are already reaped, so this is a no-op there.
            // SAFETY: `job` is owned by `Tree` and closed exactly once.
            unsafe { CloseHandle(self.job) };
        }
    }

    fn resume_primary_thread(process_id: u32) -> io::Result<()> {
        // A CREATE_SUSPENDED process has only its primary thread. Enumerating it
        // after Job assignment lets us resume without relying on undocumented
        // process APIs; no application code can run during this window.
        // SAFETY: ToolHelp returns an owned snapshot handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let result = find_and_resume(snapshot, process_id);
        // SAFETY: `snapshot` is owned here and closed exactly once.
        unsafe { CloseHandle(snapshot) };
        result
    }

    fn find_and_resume(snapshot: HANDLE, process_id: u32) -> io::Result<()> {
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..THREADENTRY32::default()
        };
        // SAFETY: `entry` has the documented size and remains writable.
        let mut present = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while present {
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: the thread id came from the live snapshot. The handle
                // is opened with only the access needed to resume it.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: `thread` is a live thread handle.
                let resumed = unsafe { ResumeThread(thread) };
                // SAFETY: `thread` is owned here and closed exactly once.
                unsafe { CloseHandle(thread) };
                if resumed == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            // SAFETY: same initialized snapshot/entry contract as Thread32First.
            present = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "suspended child primary thread was not found",
        ))
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::io;
    use std::process::{Child, Command};
    use std::time::Duration;

    pub(super) struct Tree;

    impl Tree {
        pub(super) fn prepare(_command: &mut Command) -> io::Result<Self> {
            Ok(Self)
        }

        pub(super) fn attach_and_start(&mut self, _child: &mut Child) -> io::Result<()> {
            Ok(())
        }

        pub(super) fn terminate(&mut self, child: &mut Child, _grace: Duration) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
