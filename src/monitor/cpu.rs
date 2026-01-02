//! Reads live CPU data from the Linux kernel.

use crate::{error, warning};
use cpu_monitor::CpuInstant;
use std::{
    fs::{read_dir, read_to_string, File},
    io::{BufRead, BufReader},
    process::exit,
};

pub struct Cpu {
    temp_sensor: Option<String>,
    rapl_max_uj: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            temp_sensor: find_temp_sensor(),
            rapl_max_uj: get_max_energy(),
        }
    }

    /// Warn once if temperature sensor is missing.
    pub fn warn_temp(&self) {
        if self.temp_sensor.is_none() {
            warning!("No supported CPU temperature sensor was found");
            eprintln!("         CPU temperature will not be displayed, and alarm will be disabled.");
            eprintln!("         Supported kernel modules: asusec, coretemp, k10temp, zenpower.");
        }
    }

    /// Warn once if RAPL is missing.
    pub fn warn_rapl(&self) {
        if self.rapl_max_uj == 0 {
            warning!("RAPL module was not found");
            eprintln!("         CPU power consumption will not be displayed.");
        }
    }

    /// Returns CPU temperature in °C or °F. Safe fallback: 0.
    pub fn get_temp(&self, fahrenheit: bool) -> u8 {
        let Some(sensor) = &self.temp_sensor else {
            return 0;
        };

        let Ok(data) = read_to_string(sensor) else {
            error!("Failed to get CPU temperature");
            return 0;
        };

        let Ok(mut temp) = data.trim_end().parse::<u32>() else {
            return 0;
        };

        if fahrenheit {
            temp = temp * 9 / 5 + 32_000;
        }

        ((temp as f32) / 1000.0).round() as u8
    }

    /// Reads CPU energy (µJ). Safe fallback: 0.
    pub fn read_energy(&self) -> u64 {
        if self.rapl_max_uj == 0 {
            return 0;
        }

        if let Ok(data) =
            read_to_string("/sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj")
        {
            return data.trim_end().parse::<u64>().unwrap_or(0);
        }

        0
    }

    /// Calculates CPU power in Watts. Safe fallback: 0.
    ///
    /// Formula: `W = ΔµJ / (Δms * 1000)`
    pub fn get_power(&self, initial_energy: u64, delta_millisec: u64) -> u16 {
        if self.rapl_max_uj == 0 || initial_energy == 0 || delta_millisec == 0 {
            return 0;
        }

        let current_energy = self.read_energy();
        if current_energy == 0 {
            return 0;
        }

        let delta_energy = if current_energy >= initial_energy {
            current_energy - initial_energy
        } else {
            // Counter wrap
            (self.rapl_max_uj + current_energy) - initial_energy
        };

        ((delta_energy as f64) / (delta_millisec as f64 * 1000.0))
            .round()
            .min(999.0) as u16
    }

    /// Reads CPU instant (usage baseline). Fatal if system API is broken.
    pub fn read_instant(&self) -> CpuInstant {
        CpuInstant::now().unwrap_or_else(|_| {
            error!("Failed to get CPU usage");
            exit(1);
        })
    }

    /// Returns CPU usage 0–100%.
    pub fn get_usage(&self, initial_instant: CpuInstant) -> u8 {
        let usage = (self.read_instant() - initial_instant).non_idle() * 100.0;
        usage.round().clamp(0.0, 100.0) as u8
    }

    /// Returns highest core frequency in MHz. Fatal only if `/proc/cpuinfo` is broken.
    pub fn get_frequency(&self) -> u16 {
        let cpuinfo = read_to_string("/proc/cpuinfo").unwrap_or_else(|_| {
            error!("Failed to get CPU clock");
            exit(1);
        });

        let mut highest = 0.0;
        for line in cpuinfo.lines() {
            if let Some(rest) = line.strip_prefix("cpu MHz") {
                if let Some(v) = rest.split(':').nth(1) {
                    if let Ok(mhz) = v.trim().parse::<f32>() {
                        highest = highest.max(mhz);
                    }
                }
            }
        }

        highest.round() as u16
    }
}

/// Finds a supported hwmon temperature sensor.
fn find_temp_sensor() -> Option<String> {
    for sensor in read_dir("/sys/class/hwmon").ok()? {
        let path = sensor.ok()?.path();
        let name = read_to_string(path.join("name")).ok()?;
        if ["asusec", "coretemp", "k10temp", "zenpower"].contains(&name.trim()) {
            return Some(path.join("temp1_input").to_string_lossy().to_string());
        }
    }
    None
}

/// Reads max RAPL energy range (µJ). Returns 0 if unavailable.
fn get_max_energy() -> u64 {
    read_to_string("/sys/class/powercap/intel-rapl/intel-rapl:0/max_energy_range_uj")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Gets CPU model name.
pub fn get_name() -> Option<String> {
    let file = File::open("/proc/cpuinfo").ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().flatten() {
        if let Some(rest) = line.strip_prefix("model name") {
            return rest.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    None
}
