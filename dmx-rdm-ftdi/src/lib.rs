//! Library for using ftdi uarts, connected to rs485 transceivers with the [dmx-rdm-rs](https://crates.io/crates/dmx-rdm)
//! library. Most usb rs485 cables using a ftdi chipset will work. There are even once
//! that already have the XLR-connector attached to them.
//!
//! <div class="warning">This driver won't work with Enttec OpenDMX or Enttec DMX Pro devices.
//! Refer to the readme for more details.</div>

use dmx_rdm::consts::{DMX_BAUD, INTER_SLOT_TIME_MILLIS};
use dmx_rdm::dmx_uart_driver::{
    DmxRecvUartDriver, DmxRespUartDriver, DmxUartDriver, DmxUartDriverError,
};
use libftd2xx::{BitsPerWord, FtStatus, Ftdi, FtdiCommon, Parity, StopBits};
use std::time::{Duration, SystemTime};

pub struct FtdiDriverConfig {
    /// In order to comply with the standard this value has to be set to 2ms.
    /// This is extremely cpu intensive. Most of the time lower rates will suffice but be careful.
    pub latency_timer: Duration,
}

impl Default for FtdiDriverConfig {
    fn default() -> Self {
        Self {
            // this puts a lot of work on the kernel but complies with the standard
            latency_timer: Duration::from_millis(2),
        }
    }
}

pub struct FtdiDriver {
    serial_port: Ftdi,
    latency_timer_us: u32,
}

impl FtdiDriver {
    pub fn new(mut serial_port: Ftdi, config: FtdiDriverConfig) -> Result<Self, FtStatus> {
        serial_port.set_baud_rate(DMX_BAUD)?;
        serial_port.set_data_characteristics(BitsPerWord::Bits8, StopBits::Bits2, Parity::No)?;
        serial_port.set_flow_control_none()?;
        serial_port.set_timeouts(
            Duration::from_millis(INTER_SLOT_TIME_MILLIS as u64),
            Duration::from_secs(1),
        )?;
        serial_port.set_latency_timer(config.latency_timer)?;

        Ok(Self {
            serial_port,
            latency_timer_us: config.latency_timer.as_micros() as u32,
        })
    }

    fn begin_package(&mut self) -> Result<(), FtStatus> {
        while self.serial_port.status()?.ammount_in_tx_queue != 0 {}

        spin_sleep::sleep(Duration::from_millis(50));

        self.serial_port.set_break_on()?;
        self.serial_port.set_break_off()?;
        // no sleeps since clearing the break already takes long enough 😉

        Ok(())
    }

    fn check_timeout(&self, requested_timeout_us: u32) -> u32 {
        // Bypassing the timer check, because we are expecting to be in the middle of a package.
        if requested_timeout_us == 0 {
            return 0;
        }

        if requested_timeout_us < self.latency_timer_us {
            #[cfg(feature = "log")]
            log::warn!("Requested timeout ({}µs) is shorter then latency timer ({}µs). This will cause timing issues. Using latency timer to prevent errors.", requested_timeout_us, self.latency_timer_us);

            return self.latency_timer_us;
        }

        requested_timeout_us
    }
}

impl DmxUartDriver for FtdiDriver {
    type DriverError = FtStatus;
}

impl DmxRespUartDriver for FtdiDriver {
    fn write_frames(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        self.begin_package()?;
        self.write_frames_no_break(buffer)
    }

    fn write_frames_no_break(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        Ok(self.serial_port.write(buffer)?)
    }
}

impl DmxRecvUartDriver for FtdiDriver {
    fn read_frames(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        // for some bizarre reason a break shows up as a single 0x00 byte
        let start_time = SystemTime::now();
        let mut break_byte = [0xFFu8; 1];

        let actual_timeout = self.check_timeout(timeout_us);

        while start_time.elapsed().unwrap().as_micros() < actual_timeout as u128 {
            let bytes_read = self.serial_port.read(&mut break_byte)?;
            if bytes_read != 0 && break_byte[0] == 0 {
                return self.read_frames_no_break(buffer, 1);
            }
        }

        Err(DmxUartDriverError::TimeoutError)
    }

    fn read_frames_no_break(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        let buffer_size = buffer.len();
        let mut head = 0;

        let actual_timeout_us = self.check_timeout(timeout_us);

        let mut slot_start = SystemTime::now();
        while head < buffer_size {
            let bytes_read = self.serial_port.read(&mut buffer[head..])?;
            head += bytes_read;

            if head == 0 {
                if slot_start.elapsed().unwrap().as_micros() < actual_timeout_us as u128 {
                    continue;
                }

                return Err(DmxUartDriverError::TimeoutError);
            }

            if bytes_read > 0 {
                slot_start = SystemTime::now();
            } else if slot_start.elapsed().unwrap().as_millis() >= INTER_SLOT_TIME_MILLIS as u128 {
                break;
            }
        }

        Ok(head)
    }
}
