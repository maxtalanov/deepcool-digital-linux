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

        // Base HID packet
        let mut base: [u8; 64] = [0; 64];
        base[0] = 16;
        base[1] = 104;
        base[2] = 1;
        base[3] = 2;
        base[4] = 11;
        base[5] = 1;
        base[6] = 2;
        base[7] = 5;

        loop {
            let mut pkt = base;

            // CPU instant (always available)
            let cpu_instant = self.cpu.read_instant();

            // Optional energy → optional power
            let power: u16 = self
                .cpu
                .read_energy()
                .and_then(|energy| {
                    self.cpu
                        .get_power(energy, self.update.as_millis() as u64)
                        .ok()
                })
                .unwrap_or(0);

            // Wait for update interval
            sleep(self.update);

            // Power (safe: 0 if unavailable)
            let power_bytes = power.to_be_bytes();
            pkt[8] = power_bytes[0];
            pkt[9] = power_bytes[1];

            // Temperature
            let temp = (self.cpu.get_temp(self.fahrenheit) as f32).to_be_bytes();
            pkt[10] = if self.fahrenheit { 1 } else { 0 };
            pkt[11] = temp[0];
            pkt[12] = temp[1];
            pkt[13] = temp[2];
            pkt[14] = temp[3];

            // CPU usage
            pkt[15] = self.cpu.get_usage(cpu_instant);

            // Checksum + terminator
            let checksum: u16 = pkt[1..=15].iter().map(|&x| x as u16).sum();
            pkt[16] = (checksum % 256) as u8;
            pkt[17] = 22;

            device.write(&pkt).unwrap();
        }
    }
}
