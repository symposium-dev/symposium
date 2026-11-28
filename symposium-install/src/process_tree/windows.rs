//! Windows acquisition ownership. No completion-port message is an exit barrier.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_MORE_DATA, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, OpenThread, PROCESS_SYNCHRONIZE, ResumeThread,
    THREAD_SUSPEND_RESUME, WaitForSingleObject,
};

use super::Console;

const CLEANUP_LIMIT: Duration = Duration::from_secs(5);

fn handle(value: &impl AsRawHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}

fn owned(value: HANDLE) -> OwnedHandle {
    // SAFETY: callers supply newly created, valid handles, transferring ownership.
    unsafe { OwnedHandle::from_raw_handle(value.0) }
}

pub(super) struct JobChild {
    child: Child,
    job: OwnedHandle,
}

impl JobChild {
    pub(super) fn spawn(mut command: Command, console: Console, flags: u32) -> io::Result<Self> {
        if flags & CREATE_SUSPENDED.0 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "acquisition cannot start explicitly suspended",
            ));
        }
        // SAFETY: no security descriptor or name pointers are supplied.
        let job = owned(unsafe { CreateJobObjectW(None, None) }?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: limits has the exact layout and size required by this class.
        unsafe {
            SetInformationJobObject(
                handle(&job),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of_val(&limits) as u32,
            )
        }?;
        let flags = flags
            | CREATE_SUSPENDED.0
            | match console {
                Console::Hidden => CREATE_NO_WINDOW.0,
                Console::Inherited => 0,
            };
        command.creation_flags(flags);
        let child = command.spawn()?;
        let mut running = Self { child, job };
        // SAFETY: both handles are live; suspension prevents descendants before assignment.
        let assigned =
            unsafe { AssignProcessToJobObject(handle(&running.job), handle(&running.child)) };
        if let Err(error) = assigned {
            // The child is not in our job yet. Terminate it directly before returning.
            if let Err(cleanup) = running.child.kill() {
                tracing::error!(%cleanup, "could not terminate unassigned installer");
            }
            if let Err(cleanup) =
                wait_process(handle(&running.child), Instant::now() + CLEANUP_LIMIT)
            {
                tracing::error!(%cleanup, "could not confirm unassigned installer exit");
            }
            return Err(error.into());
        }
        if let Err(error) = resume(running.child.id()) {
            if let Err(cleanup) = running.terminate_and_wait() {
                tracing::error!(%cleanup, "could not clean up suspended installer");
            }
            return Err(error);
        }
        Ok(running)
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            // Preserve normal completion: a successful installer may start a daemon.
            // Cancellation keeps kill-on-close enabled as a fallback if termination fails.
            let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // SAFETY: the job and correctly sized limits remain live during the call.
            unsafe {
                SetInformationJobObject(
                    handle(&self.job),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as _,
                    size_of_val(&limits) as u32,
                )
            }?;
        }
        Ok(status)
    }

    pub(super) fn terminate_and_wait(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + CLEANUP_LIMIT;
        // Retain process handles before termination: PID reuse and job notifications
        // cannot establish whether these particular processes have finished.
        let processes = self.process_handles(deadline);
        // SAFETY: the job is live and owned by this acquisition.
        unsafe { TerminateJobObject(handle(&self.job), 1) }?;
        let processes = processes?;
        for process in processes {
            wait_process(handle(&process), deadline)?;
        }
        loop {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: info is writable and matches the requested information class.
            unsafe {
                QueryInformationJobObject(
                    Some(handle(&self.job)),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as _,
                    size_of_val(&info) as u32,
                    None,
                )
            }?;
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "installer job did not drain before cleanup deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn process_handles(&self, deadline: Instant) -> io::Result<Vec<OwnedHandle>> {
        let mut capacity = 32;
        loop {
            // usize storage supplies the alignment required by ProcessIdList.
            let mut storage = vec![0usize; capacity + 2];
            let length =
                u32::try_from(size_of_val(storage.as_slice())).map_err(io::Error::other)?;
            // SAFETY: storage is aligned, initialized, and its byte length is supplied.
            let result = unsafe {
                QueryInformationJobObject(
                    Some(handle(&self.job)),
                    JobObjectBasicProcessIdList,
                    storage.as_mut_ptr().cast(),
                    length,
                    None,
                )
            };
            match result {
                Ok(()) => {
                    // SAFETY: a successful query populated the correctly aligned header;
                    // validate its count against the supplied allocation before reading IDs.
                    let info =
                        unsafe { &*storage.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
                    let count = info.NumberOfProcessIdsInList as usize;
                    if count > capacity {
                        return Err(io::Error::other(
                            "installer job returned too many process IDs",
                        ));
                    }
                    // SAFETY: the checked count fits within storage after the fixed header.
                    let ids =
                        unsafe { std::slice::from_raw_parts(info.ProcessIdList.as_ptr(), count) };
                    return ids
                        .iter()
                        .map(|&pid| {
                            if Instant::now() >= deadline {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "installer process inspection exceeded cleanup deadline",
                                ));
                            }
                            let pid = u32::try_from(pid).map_err(io::Error::other)?;
                            // SAFETY: OpenProcess validates the PID; successful handles are owned.
                            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
                                .map(owned)
                                .map_err(Into::into)
                        })
                        .collect();
                }
                Err(error)
                    if error.code() == ERROR_MORE_DATA.to_hresult()
                        && Instant::now() < deadline =>
                {
                    capacity = capacity
                        .checked_mul(2)
                        .ok_or_else(|| io::Error::other("installer job process list overflow"))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

fn wait_process(process: HANDLE, deadline: Instant) -> io::Result<()> {
    let millis = deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    let millis = u32::try_from(millis).unwrap_or(u32::MAX - 1);
    // SAFETY: callers retain ownership of a handle with SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process, millis) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "installer process did not exit before cleanup deadline",
        )),
        _ => Err(io::Error::last_os_error()),
    }
}

fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: no caller-owned pointers are supplied; successful snapshot is owned.
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }?);
    let mut thread = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: thread is initialized with the required size; snapshot remains live.
    unsafe { Thread32First(handle(&snapshot), &mut thread) }?;
    loop {
        if thread.th32OwnerProcessID == pid {
            // The initial thread is the only thread of our suspended child.
            // SAFETY: OpenThread validates the ID and returns an owned handle.
            let initial =
                owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, thread.th32ThreadID) }?);
            // SAFETY: initial grants THREAD_SUSPEND_RESUME and remains live.
            if unsafe { ResumeThread(handle(&initial)) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: snapshot and correctly initialized thread remain live.
        match unsafe { Thread32Next(handle(&snapshot), &mut thread) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => {
                return Err(io::Error::other("suspended installer thread not found"));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcess;

    #[test]
    fn waiting_for_a_live_process_respects_the_cleanup_deadline() {
        // SAFETY: GetCurrentProcess returns a live pseudo-handle with synchronization access.
        let process = unsafe { GetCurrentProcess() };
        let started = Instant::now();
        let error = wait_process(process, started + Duration::from_millis(20)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
