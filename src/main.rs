mod devices;
mod monitor;
mod utils;

use colored::*;
use devices::*;
use hidapi::{HidApi, HidDevice};
use monitor::{cpu, gpu};
use std::ffi::CString;
use std::process::exit;
use utils::{args::Args, status::*};

/// Common warning checks for command arguments.
mod common_warnings {
    use crate::{devices::Mode, utils::args::Args, warning};

    pub fn mode_change(args: &Args) {
        if args.mode != Mode::Default {
            warning!("Display mode cannot be changed, value will be ignored");
        }
    }

    pub fn secondary_mode(args: &Args) {
        if args.secondary != Mode::Default {
            warning!("Secondary display mode is not supported, value will be ignored");
        }
    }

    pub fn fahrenheit(args: &Args) {
        if args.fahrenheit {
            warning!("Displaying ˚F is not supported, value will be ignored");
        }
    }

    pub fn alarm_hardcoded(args: &Args) {
        if args.alarm {
            warning!("The alarm is hard-coded in your device, value will be ignored");
        }
    }

    pub fn rotate(args: &Args) {
        if args.rotate > 0 {
            warning!("Display rotation is not supported, value will be ignored");
        }
    }
}

fn main() {
    let args = Args::read();
    println!("--- Deepcool Digital Linux ---");

    /* ================= GPU ================= */

    let pci_device = {
        let gpus = gpu::pci::get_gpu_list();

        if gpus.is_empty() {
            None
        } else {
            let selected = match args.gpuid {
                Some((vendor, id)) => {
                    let mut nth = 1;

                    if id > 0 {
                        gpus.iter()
                            .filter(|g| g.vendor == vendor && g.bus > 0)
                            .find(|_| {
                                let ok = nth == id;
                                nth += 1;
                                ok
                            })
                            .cloned()
                    } else {
                        gpus.first()
                            .filter(|g| g.vendor == vendor && g.bus == 0)
                            .cloned()
                    }
                }
                None => gpus
                    .iter()
                    .find(|g| g.bus > 0)
                    .cloned()
                    .or_else(|| gpus.first().cloned()),
            };

            selected.or_else(|| {
                error!("No GPU was found with the specified GPUID");
                exit(1)
            })
        }
    };

    match cpu::get_name() {
        Some(name) => println!("CPU MON.: {}", name.bright_green()),
        None => println!("CPU MON.: {}", "Unknown CPU".bright_green()),
    }

    match &pci_device {
        Some(gpu) => println!("GPU MON.: {}", gpu.name.bright_green()),
        None => println!("GPU MON.: {}", "none".bright_black()),
    }

    println!("-----");

    /* ================= HID ================= */

    let api = HidApi::new().unwrap_or_else(|e| {
        error!(e);
        exit(1);
    });

    let (product_id, forced_device): (u16, Option<HidDevice>) =
        if let Some(path) = &args.hidraw {
            if args.pid == 0 {
                error!("--hidraw requires --pid (e.g. --pid 16)");
                exit(1);
            }

            let cpath = CString::new(path.as_str()).unwrap_or_else(|_| {
                error!("Invalid --hidraw path");
                exit(1);
            });

            let dev = api
                .open_path(cpath.as_c_str())
                .unwrap_or_else(|_| device_error());

            println!("Device found: {}", format!("hidraw={path}").bright_green());

            (args.pid, Some(dev))
        } else {
            let mut pid = 0u16;

            for d in api.device_list() {
                if d.vendor_id() == DEFAULT_VENDOR_ID
                    && (args.pid == 0 || d.product_id() == args.pid)
                {
                    pid = d.product_id();
                    println!(
                        "Device found: {}",
                        d.product_string().unwrap_or("Unknown").bright_green()
                    );
                    break;
                }
            }

            if pid == 0 {
                if args.pid > 0 {
                    error!("No DeepCool device was found with the specified PID");
                } else {
                    error!("No DeepCool device was found");
                }
                exit(1);
            }

            (pid, None)
        };

    let cpu = cpu::Cpu::new();
    let gpu = gpu::Gpu::new(pci_device);

    /* ================= DISPATCH ================= */

    match product_id {
        /* ===== AK400 DIGITAL PRO ===== */
        16 => {
            println!("Supported modes: {}", "auto".bold());

            let ak400 =
                devices::ak400_pro::Display::new(cpu, args.update, args.fahrenheit);

            print_device_status(
                &devices::ak400_pro::DEFAULT_MODE,
                None,
                None,
                if args.fahrenheit {
                    TemperatureUnit::Fahrenheit
                } else {
                    TemperatureUnit::Celsius
                },
                Alarm {
                    state: AlarmState::Auto,
                    temp_limit: if args.fahrenheit {
                        devices::ak400_pro::TEMP_LIMIT_F
                    } else {
                        devices::ak400_pro::TEMP_LIMIT_C
                    },
                    temp_warning: if args.fahrenheit {
                        devices::ak400_pro::TEMP_WARNING_F
                    } else {
                        devices::ak400_pro::TEMP_WARNING_C
                    },
                },
                args.update,
            );

            common_warnings::mode_change(&args);
            common_warnings::secondary_mode(&args);
            common_warnings::alarm_hardcoded(&args);
            common_warnings::rotate(&args);

            if let Some(dev) = forced_device {
                ak400.run_device(dev);
            } else {
                ak400.run(&api, DEFAULT_VENDOR_ID, product_id);
            }
        }

        /* ===== UNSUPPORTED ===== */
        _ => {
            println!("Device not yet supported!");

            let dev = api
                .open(DEFAULT_VENDOR_ID, product_id)
                .unwrap_or_else(|_| device_error());

            let info = dev.get_device_info().unwrap();

            println!("Vendor ID: {}", info.vendor_id());
            println!("Product ID: {}", info.product_id());
            println!(
                "Device: {}",
                info.product_string().unwrap_or("unknown")
            );
        }
    }
}
