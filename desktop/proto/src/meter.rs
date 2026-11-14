//! What the process has used, for the meter's `second` records: process CPU, the UI thread's
//! CPU (the meter samples from poll, on the UI thread) and resident memory.
//!
//! Unix: `getrusage(RUSAGE_SELF)` and `clock_gettime(CLOCK_THREAD_CPUTIME_ID)`; memory from
//! `/proc/self/statm` on Linux. macOS's footprint (`proc_pid_rusage`) and Windows
//! (`GetProcessTimes`, `GetThreadTimes`, `GetProcessMemoryInfo`) come with the owner's
//! measuring kit; until then those numbers are `null` there.

use std::time::Duration;

use sshee_ui::meter::ProcessSample;

#[cfg(unix)]
pub fn sample() -> ProcessSample {
    ProcessSample { cpu: process_cpu(), thread_cpu: thread_cpu(), memory: memory() }
}

#[cfg(not(unix))]
pub fn sample() -> ProcessSample {
    ProcessSample::default()
}

#[cfg(unix)]
fn process_cpu() -> Option<Duration> {
    // SAFETY: getrusage fills the struct it's given; a zeroed rusage is a valid value.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: RUSAGE_SELF and a valid pointer to a rusage we own.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return None;
    }
    let tv = |t: libc::timeval| Duration::from_secs(t.tv_sec.max(0) as u64) + Duration::from_micros(t.tv_usec.max(0) as u64);
    Some(tv(ru.ru_utime) + tv(ru.ru_stime))
}

#[cfg(unix)]
fn thread_cpu() -> Option<Duration> {
    // SAFETY: a zeroed timespec is valid; clock_gettime writes it.
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: a supported clock id and a valid pointer to a timespec we own.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } != 0 {
        return None;
    }
    Some(Duration::new(ts.tv_sec.max(0) as u64, ts.tv_nsec.clamp(0, 999_999_999) as u32))
}

#[cfg(target_os = "linux")]
fn memory() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf with a valid name has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (page > 0).then(|| resident * page as u64)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn the_process_is_measured() {
        let a = super::sample();
        // Burn a little CPU on this thread.
        let mut x = 0u64;
        for i in 0..5_000_000u64 {
            x = x.wrapping_mul(31).wrapping_add(i);
        }
        std::hint::black_box(x);
        let b = super::sample();
        assert!(b.cpu.unwrap() >= a.cpu.unwrap());
        assert!(b.thread_cpu.unwrap() > a.thread_cpu.unwrap());
        assert!(b.memory.unwrap() > 1 << 20, "{:?}", b.memory);
    }
}
