//! Implementation of serial ports on arm microcontrollers.
//! At the moment, the implementation is simple and does not allow
//! for interrupts or DMA buffering.
//! However, it is sufficient for use with Theseus's logger.
//!
//! When compiling for a specific microcontroller, we distinguish the platform
//! by using the value of `target_vendor` specified by the custom cargo target.
//! When a `target_vendor` is specified, we rely upon the implementations of 
//! `take_serial_port`, `SerialPort`, and `SerialPortAddress` provided by
//! the platform's associated subcrate in `kernel/boards`. This is necessary because
//! each platform has its own peculiarities in working with the UART, so serial port
//! code must be implemented for each platform.
//!
//! When the `target_vendor` is unknown, we rely on a dummy implementation using semihosting,
//! a form of communication that allows a microcontroller to simulate
//! i/o operations on a host device and is supported by most Cortex-M CPUs.
//! For more info on semihosting, read (here)[https://www.keil.com/support/man/docs/armcc/armcc_pge1358787046598.htm]
#![no_std]

#[macro_use]
extern crate cfg_if;

cfg_if ! {
    if #[cfg(target_vendor = "stm32f407")] {
        extern crate stm32f4_discovery;
        pub use stm32f4_discovery::uart::{take_serial_port, SerialPort, SerialPortAddress};
    } 

    // Dummy implementation for when no physical device is present, in which case semihosting will be used
    else if #[cfg(all(target_arch = "arm", target_vendor = "unknown"))] {
        extern crate cortex_m_semihosting;
        extern crate irq_safety;
        extern crate spin;

        use cortex_m_semihosting::hio::hstdout;
        use core::fmt::{self, Write};
        use irq_safety::MutexIrqSafe;
        use spin::Once;
        
        /// Represents the serial ports available for use.
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum SerialPortAddress {
            /// In this case, we are using semihosting as a dummy implementation of
            /// serial ports, so there is only the single dummy address, `Semihost`.
            Semihost,
        }

        impl SerialPortAddress {
            /// Returns a reference to the static instance of this serial port.
            fn to_static_port(&self) -> &'static MutexIrqSafe<TriState<SerialPort>> {
                match self {
                    SerialPortAddress::Semihost => &SEMIHOSTING_DUMMY_PORT,
                }
            }
        }

        /// This type is used to ensure that an object of type `T` is only initialized once,
        /// but still allows for a caller to take ownership of the object `T`. 
        enum TriState<T> {
            Uninited,
            Inited(T),
            Taken,
        }
        impl<T> TriState<T> {
            fn take(&mut self) -> Option<T> {
                if let Self::Inited(_) = self {
                    if let Self::Inited(v) = core::mem::replace(self, Self::Taken) {
                        return Some(v);
                    }
                }
                None
            }
        }

        // Serial ports cannot be reliably probed (discovered dynamically), thus,
        // we ensure they are exposed safely as singletons through the below static instances.
        static SEMIHOSTING_DUMMY_PORT: MutexIrqSafe<TriState<SerialPort>> = MutexIrqSafe::new(TriState::Uninited);


        /// Takes ownership of the [`SerialPort`] specified by the given [`SerialPortAddress`].
        ///
        /// This function initializes the given serial port if it has not yet been initialized.
        /// If the serial port has already been initialized and taken by another crate,
        /// this returns `None`.
        ///
        /// The returned [`SerialPort`] will be restored to this crate upon being dropped.
        pub fn take_serial_port(
            serial_port_address: SerialPortAddress
        ) -> Option<SerialPort> {
            let sp = serial_port_address.to_static_port();
            let mut locked = sp.lock();
            if let TriState::Uninited = &*locked {
                *locked = TriState::Inited(SerialPort::new());
            }
            locked.take()
        }


        /// The `SerialPort` struct implements the `Write` trait for use with logging capabilities.
        pub struct SerialPort;

        impl Drop for SerialPort {
            fn drop(&mut self) {
                let sp = SerialPortAddress::Semihost.to_static_port();
                let mut sp_locked = sp.lock();
                if let TriState::Taken = &*sp_locked {
                    let dummy = SerialPort;
                    let dropped = core::mem::replace(self, dummy);
                    *sp_locked = TriState::Inited(dropped);
                }
            }
        }

        impl SerialPort {
            pub fn new() -> SerialPort {
                SerialPort
            }
        } 

        impl fmt::Write for SerialPort {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let mut semihosting_out = hstdout().unwrap();
                semihosting_out.write_all(s.as_bytes()).map_err(|_| fmt::Error)
            }
        }
    }
}
