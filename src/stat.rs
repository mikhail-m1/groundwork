use std::sync::Arc;

use poem::error::InternalServerError;
use poem::web::Data;
use poem::{Error, Result, handler, http::StatusCode};
use serde::Serialize;

pub struct StatsData {
    name: String,
    usage_time_to_us: f64,
}

impl StatsData {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            usage_time_to_us: usage_time_to_us(),
        }
    }
}

#[cfg(target_os = "macos")]
fn usage_time_to_us() -> f64 {
    unsafe {
        let mut i = mach2::mach_time::mach_timebase_info { numer: 0, denom: 0 };
        if mach2::mach_time::mach_timebase_info(&mut i) != 0 {
            panic!();
        }
        i.numer as f64 / i.denom as f64 / 1e3
    }
}

#[cfg(target_os = "linux")]
fn usage_time_to_us() -> f64 {
    let ticks_per_second = procfs::ticks_per_second();
    1e6 / ticks_per_second as f64
}

#[handler]
pub fn stats(data: Data<&Arc<StatsData>>) -> Result<String> {
    let allocator_metrics = alloc_metrics::global_metrics();
    let mem_allocated_bytes = allocator_metrics.allocated_bytes as u64;
    let allocations = allocator_metrics.allocations as u64;
    let name = data.name.clone();
    let hostname = hostname::get()
        .map_err(InternalServerError)?
        .into_string()
        .map_err(|e| Error::from_string(e.to_string_lossy(), StatusCode::INTERNAL_SERVER_ERROR))?;

    let stats = {
        #[cfg(target_os = "linux")]
        {
            use procfs::WithCurrentSystemInfo;
            let map_err = |e: procfs::ProcError| {
                Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
            };
            let process = procfs::process::Process::myself().map_err(map_err)?;
            let status = process.status().map_err(map_err)?;
            let stat = process.stat().map_err(map_err)?;
            Stats {
                name,
                hostname,
                mem_allocated_bytes,
                allocations,
                mem_rss: status.vmrss.unwrap() * 1024,
                mem_rss_peak: status.vmhwm.unwrap() * 1024,
                mem_virtual: status.vmsize.unwrap() * 1024,
                fd_count: process.fd_count().map_err(map_err)? as u64,
                threads_count: status.threads,
                user_time_us: (stat.utime as f64 * data.usage_time_to_us) as u64,
                system_time_us: (stat.stime as f64 * data.usage_time_to_us) as u64,
                start_time_ms: stat.starttime().get().map_err(map_err)?.timestamp_millis() as u64,
            }
        }
        #[cfg(target_os = "macos")]
        {
            let pid = std::process::id();
            let info = libproc::proc_pid::pidinfo::<libproc::task_info::TaskAllInfo>(pid as i32, 1)
                .map_err(|e| Error::from_string(e, StatusCode::INTERNAL_SERVER_ERROR))?;
            let mut rusage;
            unsafe {
                rusage = std::mem::zeroed();
                if libc::getrusage(libc::RUSAGE_SELF, &mut rusage) != libc::EXIT_SUCCESS {
                    panic!();
                }
            }
            Stats {
                name,
                hostname,
                mem_allocated_bytes,
                allocations,
                mem_rss: info.ptinfo.pti_resident_size,
                mem_rss_peak: rusage.ru_maxrss as u64,
                mem_virtual: info.ptinfo.pti_virtual_size,
                fd_count: info.pbsd.pbi_nfiles as u64,
                threads_count: info.ptinfo.pti_threadnum as u64,
                user_time_us: (info.ptinfo.pti_total_user as f64 * data.usage_time_to_us) as u64,
                system_time_us: (info.ptinfo.pti_total_system as f64 * data.usage_time_to_us)
                    as u64,
                start_time_ms: info.pbsd.pbi_start_tvsec * 1000 + info.pbsd.pbi_start_tvusec / 1000,
            }
        }
    };
    serde_json::to_string(&stats).map_err(InternalServerError)
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Stats {
    name: String,
    hostname: String,
    mem_rss: u64,
    mem_rss_peak: u64,
    mem_virtual: u64,
    mem_allocated_bytes: u64,
    allocations: u64,
    fd_count: u64,
    threads_count: u64,
    user_time_us: u64,
    system_time_us: u64,
    start_time_ms: u64,
}
