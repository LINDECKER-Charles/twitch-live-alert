use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{OsString, c_void},
    io,
    mem::size_of,
    net::Ipv4Addr,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr, slice,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, WAIT_OBJECT_0, WAIT_TIMEOUT},
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    },
    Networking::WinSock::AF_INET,
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW, SW_SHOWNORMAL},
    },
};

/// Windows supplies the PID for a listening socket. HTTP data cannot establish ownership.
pub fn listener(port: u16) -> Result<Option<u32>> {
    // DWORD alignment also satisfies MIB_TCPROW_OWNER_PID. The table is a count + rows.
    let mut buffer = vec![0u32; 256];
    loop {
        let mut bytes = (buffer.len() * size_of::<u32>()) as u32;
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut bytes,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            ensure!(bytes <= 64 * 1024 * 1024, "Table TCP trop grande.");
            buffer.resize((bytes as usize).div_ceil(size_of::<u32>()), 0);
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32))
                .context("Lecture des ports Windows");
        }
        let count = buffer[0] as usize;
        let (_, storage) = buffer.split_at(1);
        ensure!(
            count <= size_of_val(storage) / size_of::<MIB_TCPROW_OWNER_PID>(),
            "Table TCP invalide."
        );
        let rows = unsafe {
            slice::from_raw_parts(storage.as_ptr().cast::<MIB_TCPROW_OWNER_PID>(), count)
        };
        let mut owner = None;
        for row in rows {
            let addr = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
            if u16::from_be(row.dwLocalPort as u16) == port
                && (addr.is_unspecified() || addr == Ipv4Addr::LOCALHOST)
            {
                ensure!(
                    owner.is_none(),
                    "Plusieurs applications écoutent sur le port {port}."
                );
                owner = Some(row.dwOwningPid);
            }
        }
        return Ok(owner);
    }
}

pub struct Process {
    pub pid: u32,
    handle: OwnedHandle,
}

impl Process {
    pub fn verify(pid: u32, expected: &Path, port: u16) -> Result<Self> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error()).context("Accès au processus Windows");
        }
        // OpenProcess returned our own handle. Keeping it prevents PID reuse.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        ensure!(
            session(pid)? == session(std::process::id())?,
            "Le service appartient à une autre session Windows."
        );
        let mut name = vec![0u16; 32_768];
        let mut length = name.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(handle.as_raw_handle(), 0, name.as_mut_ptr(), &mut length)
        } == 0
        {
            return Err(io::Error::last_os_error()).context("Identification du binaire du service");
        }
        let path = PathBuf::from(OsString::from_wide(&name[..length as usize])).canonicalize()?;
        ensure!(
            path == expected,
            "Le port {port} est occupé par une autre application (PID {pid}). Fermez-la puis relancez Twitch Live Alert."
        );
        Ok(Self { pid, handle })
    }

    pub fn exited(&self, milliseconds: u32) -> Result<bool> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), milliseconds) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => bail!("État du service Windows indisponible."),
        }
    }
}

fn session(pid: u32) -> Result<u32> {
    let mut id = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut id) } == 0 {
        return Err(io::Error::last_os_error()).context("Identification de la session Windows");
    }
    Ok(id)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(text: &str) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Twitch Live Alert").as_ptr(),
            MB_ICONERROR | MB_OK,
        )
    };
}

pub fn open_url(url: &tauri::Url) {
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            wide("open").as_ptr(),
            wide(url.as_str()).as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        show_error("Windows n’a pas pu ouvrir le navigateur par défaut.");
    }
}

pub fn child_job(child: &std::process::Child) -> Result<OwnedHandle> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error()).context("Création du groupe de processus Windows");
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&info) as u32,
        )
    } == 0
        || unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0
    {
        return Err(io::Error::last_os_error())
            .context("Rattachement du service au lanceur Windows");
    }
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_socket_owner_and_executable_from_windows() {
        let socket = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        let pid = listener(port).unwrap().unwrap();
        assert_eq!(pid, std::process::id());
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let process = Process::verify(pid, &executable, port).unwrap();
        assert!(!process.exited(0).unwrap());
        assert!(Process::verify(pid, Path::new("foreign.exe"), port).is_err());
    }
}
