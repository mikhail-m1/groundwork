use poem::error::InternalServerError;
use poem::{Error, Result, handler, http::StatusCode};
use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    n: u32,
    kind: DescriptorKind,
    details: String,
}

#[derive(Serialize, Debug)]
pub enum DescriptorKind {
    File,
    TCP,
    UDP,
    VNode,
    KQueue,
    Pipe,
    Other,
}

#[cfg(target_os = "macos")]
#[handler]
pub fn descriptors() -> Result<String> {
    let pid = std::process::id() as i32;
    let info = libproc::proc_pid::pidinfo::<libproc::bsd_info::BSDInfo>(pid, 0)
        .map_err(|e| Error::from_string(e, StatusCode::INTERNAL_SERVER_ERROR))?;
    let fds = libproc::proc_pid::listpidinfo::<libproc::file_info::ListFDs>(
        pid,
        info.pbi_nfiles as usize,
    )
    .map_err(|e| Error::from_string(e, StatusCode::INTERNAL_SERVER_ERROR))?;

    let descriptors = fds.iter().map(convert_mac_fd).collect::<Vec<_>>();
    serde_json::to_string(&descriptors).map_err(InternalServerError)
}

#[cfg(target_os = "macos")]
fn convert_mac_fd(fd: &libproc::file_info::ProcFDInfo) -> Descriptor {
    let (kind, details) = mac_descriptor(fd);
    Descriptor {
        n: fd.proc_fd as u32,
        kind,
        details,
    }
}

#[cfg(target_os = "macos")]
fn mac_descriptor(fd: &libproc::file_info::ProcFDInfo) -> (DescriptorKind, String) {
    let pid = std::process::id() as i32;
    let kind: libproc::file_info::ProcFDType = fd.proc_fdtype.into();
    match kind {
        libproc::file_info::ProcFDType::Socket => {
            if let Ok(socket) =
                libproc::file_info::pidfdinfo::<libproc::net_info::SocketFDInfo>(pid, fd.proc_fd)
            {
                let socket_kind: libproc::net_info::SocketInfoKind = socket.psi.soi_kind.into();
                match socket_kind {
                    libproc::net_info::SocketInfoKind::Tcp => (DescriptorKind::TCP, String::new()),
                    libproc::net_info::SocketInfoKind::Un => (DescriptorKind::UDP, String::new()),
                    _ => (DescriptorKind::Other, String::new()),
                }
            } else {
                (DescriptorKind::Other, String::new())
            }
        }

        libproc::file_info::ProcFDType::KQueue => (DescriptorKind::KQueue, String::new()),
        libproc::file_info::ProcFDType::Pipe => (DescriptorKind::Pipe, String::new()),
        libproc::file_info::ProcFDType::VNode => (DescriptorKind::File, String::new()),
        _ => (DescriptorKind::Other, String::new()),
    }
}

#[cfg(target_os = "linux")]
#[handler]
pub fn descriptors() -> Result<String> {
    let map_err =
        |e: procfs::ProcError| Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR);
    let process = procfs::process::Process::myself().map_err(map_err)?;
    let sockets = linux::sockets(&process);
    let descriptors = process
        .fd()
        .map_err(map_err)?
        .map(|d| linux::descriptor(d, &sockets))
        .collect::<Vec<_>>();
    serde_json::to_string(&descriptors).map_err(InternalServerError)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Descriptor;
    use super::DescriptorKind;
    use either::Either;
    use procfs::net::{TcpNetEntry, TcpState, UdpNetEntry};
    use procfs::process::Process;

    pub fn descriptor(
        fd_res: Result<procfs::process::FDInfo, procfs::ProcError>,
        sockets: &[SocketInfo],
    ) -> Descriptor {
        let n = fd_res.as_ref().map(|v| v.fd as u32).unwrap_or(0);
        let (kind, details) = descriptor_map(fd_res, sockets);
        Descriptor { n, kind, details }
    }

    fn descriptor_map(
        fd_res: Result<procfs::process::FDInfo, procfs::ProcError>,
        sockets: &[SocketInfo],
    ) -> (DescriptorKind, String) {
        if let Ok(fd) = fd_res {
            match fd.target {
                procfs::process::FDTarget::Path(path_buf) => (
                    DescriptorKind::File,
                    path_buf.to_string_lossy().into_owned(),
                ),
                procfs::process::FDTarget::Socket(v) => {
                    if let Some(d) = sockets.iter().find(|s| s.0 == v) {
                        format_socket(&d.1)
                    } else {
                        (DescriptorKind::Other, "Unknown socket".to_string())
                    }
                }
                procfs::process::FDTarget::Net(v) => (DescriptorKind::Other, v.to_string()),
                procfs::process::FDTarget::Pipe(v) => (DescriptorKind::Pipe, v.to_string()),
                procfs::process::FDTarget::AnonInode(s) => (DescriptorKind::Other, s),
                procfs::process::FDTarget::MemFD(s) => (DescriptorKind::Other, s),
                procfs::process::FDTarget::Other(s, _) => (DescriptorKind::Other, s),
            }
        } else {
            (DescriptorKind::Other, "failed to get info".to_string())
        }
    }

    type SocketInfo = (u64, EitherSocket);
    type EitherSocket = Either<UdpNetEntry, TcpNetEntry>;

    fn format_socket(e: &EitherSocket) -> (DescriptorKind, String) {
        match e {
            Either::Left(udp) => (
                DescriptorKind::UDP,
                format!("{} {:?}", udp.local_address, udp.state),
            ),
            Either::Right(tcp) => (
                DescriptorKind::TCP,
                format!(
                    "{:?} {}",
                    tcp.state,
                    if tcp.state == TcpState::Listen {
                        tcp.local_address
                    } else {
                        tcp.remote_address
                    }
                ),
            ),
        }
    }

    pub fn sockets(process: &Process) -> Vec<SocketInfo> {
        let udp = process
            .udp()
            .into_iter()
            .chain(process.udp6().into_iter())
            .flat_map(|v| v)
            .map(|s| (s.inode, Either::Left(s)));

        let tcp = process
            .tcp()
            .into_iter()
            .chain(process.tcp6().into_iter())
            .flat_map(|v| v)
            .map(|s| (s.inode, Either::Right(s)));

        udp.chain(tcp).collect::<Vec<_>>()
    }
}
