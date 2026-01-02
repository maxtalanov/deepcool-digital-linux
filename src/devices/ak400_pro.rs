//! Display module for:
//! - AK400 DIGITAL PRO

use crate::monitor::cpu::Cpu;
use super::{device_error, Mode};
use hidapi::{HidApi, HidDevice};
use std::{thread::sleep, time::Duration};

pub const DEFAULT_MODE: Mode = Mode::Auto;

// The temperature limits are hard-coded in the device
pub const TEMP_WARNING_C: u8 = 80;
pub const TEMP_WARNING_F: u8 = 176;
pub const TEMP_LIMIT_C: u8 = 90;
pub const TEMP_LIMIT_F: u8 = 194;

pub struct Display {
    cpu: Cpu,
    update: Duration,
    fahrenheit: bool,
}

impl Display {
    pub fn new(cpu: Cpu, update: Duration, fahrenheit: bool) -> Self {
        Self {
            cpu,
            update,
            fahrenheit,
        }
    }

    /// Original entrypoint (enumeration by VID/PID).
    pub fn run(&self, api: &HidApi, vid: u16, pid: u16) {
        let device = api.open(vid, pid).unwrap_or_else(|_| device_error());
        self.run_device(device);
    }

    /// New entrypoint (already opened device, e.g. via --hidraw + open_path()).
    pub fn run_device(&self, device: HidDevice) {
        // Warn once; do NOT abort on server CPUs
        self.cpu.warn_temp();
        self.cpu.warn_rapl();

        // Base HID packet (constant part)
        let base_data: [u8; 64] = {
            let mut d = [0u8; 64];
            d[0] = 16;
            d[1] = 104;
            d[2] = 1;
            d[3] = 2;
            d[4] = 11;
            d[5] = 1;
            d[6] = 2;
            d[7] = 5;
            d
        };

        loop {
            // Start from base packet every iteration
            let mut status_data = base_data;

            // CPU instant (always works)
            let cpu_instant = self.cpu.read_instant();

            // Energy may be 0 on Xeon / server CPUs
            let cpu_energy = self.cpu.read_energy();

            sleep(self.update);

            // Power (safe for servers)
            let power: u16 = if cpu_energy > 0 {
                self.cpu.get_power(cpu_energy, self.update.as_millis() as u64)
            } else {
                0
            };

            let power_bytes = power.to_be_bytes();
            status_data[8] = power_bytes[0];
            status_data[9] = power_bytes[1];

            // Temperature
            let temp = (self.cpu.get_temp(self.fahrenheit) as f32).to_be_bytes();
            status_data[10] = if self.fahrenheit { 1 } else { 0 };
            status_data[11] = temp[0];
            status_data[12] = temp[1];
            status_data[13] = temp[2];
            status_data[14] = temp[3];

            // CPU usage
            status_data[15] = self.cpu.get_usage(cpu_instant);

            // Checksum & terminator
            let checksum: u16 = status_data[1..=15]
                .iter()
                .map(|&x| x as u16)
                .sum();

            status_data[16] = (checksum % 256) as u8;
            status_data[17] = 22;

            device.write(&status_data).unwrap();
        }
    }
}
