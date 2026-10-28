//! Optional Linux CPU pinning so each emulator thread can sit on its own physical core.
//!
//! Without pinning the scheduler sometimes piles two emulator threads onto the same core
//! while leaving others idle. Pinning is a no-op on other platforms and when the process
//! is not allowed any CPUs to spread across.

/// Logical CPUs this process is allowed to run on, from `/proc/self/status`.
#[cfg(target_os = "linux")]
pub fn allowed_cpus() -> Vec<usize> {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return Vec::new();
    };
    let Some(list) = status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
    else {
        return Vec::new();
    };
    let mut cpus = Vec::new();
    for part in list.trim().split(',') {
        let Some((start, end)) = part.split_once('-') else {
            if let Ok(cpu) = part.trim().parse::<usize>() {
                cpus.push(cpu);
            }
            continue;
        };
        let (Ok(start), Ok(end)) = (start.trim().parse::<usize>(), end.trim().parse::<usize>())
        else {
            continue;
        };
        cpus.extend(start..=end);
    }
    cpus.sort_unstable();
    cpus.dedup();
    cpus
}

#[cfg(target_os = "linux")]
fn read_cpu_id(path: String) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// One logical CPU per physical core (the first one in each core), restricted to the CPUs
/// this process is allowed to use. Returns an empty list when pinning is unsupported.
#[cfg(target_os = "linux")]
pub fn core_cpus() -> Vec<usize> {
    use std::collections::HashMap;

    let allowed = allowed_cpus();
    // core -> the smallest logical CPU number in it
    let mut cores: HashMap<(i32, i32), usize> = HashMap::new();
    for cpu in &allowed {
        let base = format!("/sys/devices/system/cpu/cpu{cpu}/topology");
        let Some(package) = read_cpu_id(format!("{base}/physical_package_id")) else {
            continue;
        };
        let Some(core) = read_cpu_id(format!("{base}/core_id")) else {
            continue;
        };
        let entry = cores.entry((package, core)).or_insert(*cpu);
        if *cpu < *entry {
            *entry = *cpu;
        }
    }
    let mut cpus: Vec<usize> = cores.into_values().collect();
    if cpus.is_empty() {
        // No topology information: fall back to one emulator per allowed logical CPU.
        return allowed;
    }
    cpus.sort_unstable();
    cpus
}

#[cfg(not(target_os = "linux"))]
pub fn core_cpus() -> Vec<usize> {
    Vec::new()
}

/// Pin the current thread to one logical CPU. No-op off Linux.
#[cfg(target_os = "linux")]
pub fn pin_current_thread(cpu: usize) {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn pin_current_thread(_cpu: usize) {}
