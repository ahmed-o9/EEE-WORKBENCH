//! Resource discovery and conservative simulation memory estimates.
//!
//! Rendering capability is reported by the desktop frontend (WebGL2 feature
//! detection). This crate intentionally does not require a vendor GPU SDK.

use std::num::NonZeroUsize;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Low,
    Moderate,
    High,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSnapshot {
    pub total_memory_bytes: Option<u64>,
    pub available_memory_bytes: Option<u64>,
    pub used_memory_bytes: Option<u64>,
    pub memory_pressure: MemoryPressure,
    pub logical_cpu_cores: usize,
    /// `None` means the current platform backend does not expose a reliable
    /// instantaneous CPU utilization value without a sampling interval.
    pub cpu_usage_percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationEstimate {
    pub unknown_count: usize,
    pub time_points: usize,
    pub matrix_bytes: u64,
    pub result_bytes: u64,
    pub estimated_peak_bytes: u64,
    pub available_memory_bytes: Option<u64>,
    pub recommended_memory_budget_bytes: Option<u64>,
    pub fits_recommended_budget: Option<bool>,
    pub recommended_workers: usize,
}

impl ResourceSnapshot {
    pub fn detect() -> Self {
        let (total, available) = detect_memory();
        let used = total
            .zip(available)
            .map(|(total, free)| total.saturating_sub(free));
        let pressure = match (total, available) {
            (Some(total), Some(available)) if total > 0 => {
                let used_ratio = 1.0 - (available.min(total) as f64 / total as f64);
                if used_ratio >= 0.90 {
                    MemoryPressure::High
                } else if used_ratio >= 0.75 {
                    MemoryPressure::Moderate
                } else {
                    MemoryPressure::Low
                }
            }
            _ => MemoryPressure::Unknown,
        };
        let logical_cpu_cores = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).expect("one is non-zero"))
            .get();
        Self {
            total_memory_bytes: total,
            available_memory_bytes: available,
            used_memory_bytes: used,
            memory_pressure: pressure,
            logical_cpu_cores,
            cpu_usage_percent: None,
        }
    }

    /// Compute budget uses a portion of currently available RAM, leaving room
    /// for Windows, the WebView, the editor and other desktop applications.
    pub fn recommended_memory_budget_bytes(&self) -> Option<u64> {
        self.available_memory_bytes
            .map(|available| (available as f64 * 0.35) as u64)
    }

    pub fn estimate_dense_simulation(
        &self,
        unknown_count: usize,
        time_points: usize,
        trace_count: usize,
    ) -> Result<SimulationEstimate, String> {
        let n = u64::try_from(unknown_count).map_err(|_| "unknown count is too large")?;
        let points = u64::try_from(time_points).map_err(|_| "time point count is too large")?;
        let traces = u64::try_from(trace_count).map_err(|_| "trace count is too large")?;

        // Matrix plus LU workspace: two dense f64 matrices and vectors.
        let matrix_entries = n
            .checked_mul(n)
            .ok_or("matrix size overflows address space")?;
        let matrix_bytes = matrix_entries
            .checked_mul(16)
            .and_then(|bytes| bytes.checked_add(n.saturating_mul(16)))
            .ok_or("matrix memory estimate overflows address space")?;
        // Result vectors: time axis plus node/device traces; a factor of two
        // covers working buffers while the result is assembled.
        let result_entries = points
            .checked_mul(traces.checked_add(1).ok_or("trace count overflow")?)
            .ok_or("result array size overflows address space")?;
        let result_bytes = result_entries
            .checked_mul(16)
            .ok_or("result memory estimate overflows address space")?;
        let estimated_peak_bytes = matrix_bytes
            .checked_add(result_bytes)
            .ok_or("simulation memory estimate overflows address space")?;
        let budget = self.recommended_memory_budget_bytes();
        let fits = budget.map(|limit| estimated_peak_bytes <= limit);
        let recommended_workers = self.logical_cpu_cores.clamp(1, 4);

        Ok(SimulationEstimate {
            unknown_count,
            time_points,
            matrix_bytes,
            result_bytes,
            estimated_peak_bytes,
            available_memory_bytes: self.available_memory_bytes,
            recommended_memory_budget_bytes: budget,
            fits_recommended_budget: fits,
            recommended_workers,
        })
    }
}

#[cfg(target_os = "linux")]
fn detect_memory() -> (Option<u64>, Option<u64>) {
    let contents = match std::fs::read_to_string("/proc/meminfo") {
        Ok(contents) => contents,
        Err(_) => return (None, None),
    };
    let mut total_kib = None;
    let mut available_kib = None;
    let mut free_kib = None;
    let mut buffers_kib = 0_u64;
    let mut cached_kib = 0_u64;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let key = fields.next().unwrap_or_default();
        let value = fields.next().and_then(|value| value.parse::<u64>().ok());
        match key {
            "MemTotal:" => total_kib = value,
            "MemAvailable:" => available_kib = value,
            "MemFree:" => free_kib = value,
            "Buffers:" => buffers_kib = value.unwrap_or(0),
            "Cached:" => cached_kib = value.unwrap_or(0),
            _ => {}
        }
    }
    let total = total_kib.and_then(|value| value.checked_mul(1024));
    let available = available_kib
        .or_else(|| {
            free_kib.map(|free| free.saturating_add(buffers_kib).saturating_add(cached_kib))
        })
        .and_then(|value| value.checked_mul(1024));
    (total, available)
}

#[cfg(target_os = "windows")]
fn detect_memory() -> (Option<u64>, Option<u64>) {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_physical: 0,
        available_physical: 0,
        total_page_file: 0,
        available_page_file: 0,
        total_virtual: 0,
        available_virtual: 0,
        available_extended_virtual: 0,
    };
    // SAFETY: `status` is initialized with the exact Win32 structure size and
    // remains writable for the duration of the documented API call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        (None, None)
    } else {
        (Some(status.total_physical), Some(status.available_physical))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn detect_memory() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_memory_estimate_is_monotonic_and_overflow_checked() {
        let snapshot = ResourceSnapshot {
            total_memory_bytes: Some(4 * 1024 * MIB),
            available_memory_bytes: Some(2 * 1024 * MIB),
            used_memory_bytes: Some(2 * 1024 * MIB),
            memory_pressure: MemoryPressure::Moderate,
            logical_cpu_cores: 2,
            cpu_usage_percent: None,
        };
        let small = snapshot.estimate_dense_simulation(20, 100, 5).unwrap();
        let large = snapshot.estimate_dense_simulation(40, 200, 10).unwrap();
        assert!(large.estimated_peak_bytes > small.estimated_peak_bytes);
        assert_eq!(
            small.recommended_memory_budget_bytes,
            Some((2 * 1024 * MIB) * 35 / 100)
        );
        assert!(snapshot
            .estimate_dense_simulation(usize::MAX, usize::MAX, usize::MAX)
            .is_err());
    }

    #[test]
    fn cpu_worker_count_is_never_zero() {
        let snapshot = ResourceSnapshot::detect();
        assert!(snapshot.logical_cpu_cores >= 1);
        assert!((1..=4).contains(
            &snapshot
                .estimate_dense_simulation(2, 2, 2)
                .unwrap()
                .recommended_workers
        ));
    }
}
