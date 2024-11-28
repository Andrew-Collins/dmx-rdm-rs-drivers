//! Library for using rp2040s/raspberry pi picos with the [dmx-rdm-rs](https://crates.io/crates/dmx-rdm) library.
//!
//! This was tested using the [Waveshare Pico-2CH-RS485](https://www.waveshare.com/wiki/Pico-2CH-RS485)
//! and does not require a pin for switching between receiving and transmitting on the transceiver.
//! The schematic for this board is also [available](https://files.waveshare.com/upload/0/02/Pico-2CH-RS485.pdf).

#![no_std]

use core::fmt::Formatter;
use dmx_rdm::dmx_uart_driver::{
    DmxRecvUartDriver, DmxRespUartDriver, DmxUartDriver, DmxUartDriverError,
};
use embedded_hal_0_2::timer::{Cancel, CountDown};
use fugit::{ExtU32, ExtU64};
use rp2040_hal::uart::{
    Enabled, ReadError, ReadErrorType, UartDevice, UartPeripheral, ValidUartPinout,
};

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Rp2040DriverError {
    Parity,
    Framing,
    Overflow,
}

impl core::fmt::Display for Rp2040DriverError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Rp2040DriverError::Parity => write!(f, "Parity error!"),
            Rp2040DriverError::Framing => write!(f, "Framing error!"),
            Rp2040DriverError::Overflow => write!(f, "Overflow error!"),
        }
    }
}

pub struct Rp2040Driver<'a, D: UartDevice, P: ValidUartPinout<D>> {
    uart: UartPeripheral<Enabled, D, P>,
    countdown: rp2040_hal::timer::CountDown<'a>,
}

impl<D: UartDevice, P: ValidUartPinout<D>> Rp2040Driver<'_, D, P> {
    pub fn new(
        uart: UartPeripheral<Enabled, D, P>,
        countdown: rp2040_hal::timer::CountDown,
    ) -> Rp2040Driver<D, P> {
        Rp2040Driver { uart, countdown }
    }

    fn begin_package(&mut self) {
        self.uart.lowlevel_break_start();

        self.countdown.start(200u64.micros()); // BRK
        while self.countdown.wait() == Err(nb::Error::WouldBlock) {}
        self.uart.lowlevel_break_stop();

        self.countdown.start(48u64.micros()); // MAB
        while self.countdown.wait() == Err(nb::Error::WouldBlock) {}
    }
}

impl<D: UartDevice, P: ValidUartPinout<D>> DmxUartDriver for Rp2040Driver<'_, D, P> {
    type DriverError = Rp2040DriverError;
}

impl<D: UartDevice, P: ValidUartPinout<D>> DmxRecvUartDriver for Rp2040Driver<'_, D, P> {
    fn read_frames(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        self.countdown.start(timeout_us.micros());
        loop {
            match self.uart.read_raw(&mut buffer[0..1]) {
                Ok(_) => {
                    // is this really the best way to clear the buffer?
                    continue;
                }
                Err(error) => match error {
                    nb::Error::Other(ReadError {
                        err_type: ReadErrorType::Break,
                        ..
                    }) => {
                        break;
                    }
                    nb::Error::WouldBlock => {
                        if self.countdown.wait() != Err(nb::Error::WouldBlock) {
                            return Err(DmxUartDriverError::TimeoutError);
                        }
                    }
                    _ => continue,
                },
            }
        }

        self.countdown.cancel().unwrap();
        let read_bytes = self.read_frames_no_break(buffer, timeout_us)?;

        Ok(read_bytes)
    }

    fn read_frames_no_break(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        const MAXIMUM_MAB_TIME_MS: u32 = 1;

        let buffer_size = buffer.len();
        let mut head = 0;

        self.countdown.start(timeout_us.micros());

        while head < buffer_size {
            let bytes_read = match self.uart.read_raw(&mut buffer[head..buffer_size]) {
                Ok(bytes_read) => {
                    self.countdown.start(MAXIMUM_MAB_TIME_MS.millis());

                    Ok(bytes_read)
                }
                Err(err) => match err {
                    nb::Error::Other(ref read_error) => match read_error.err_type {
                        ReadErrorType::Break => {
                            if head == 0 {
                                continue;
                            }

                            break;
                        }
                        ReadErrorType::Overrun => {
                            Err(DmxUartDriverError::DriverError(Rp2040DriverError::Overflow))
                        }
                        ReadErrorType::Parity => {
                            Err(DmxUartDriverError::DriverError(Rp2040DriverError::Parity))
                        }
                        ReadErrorType::Framing => {
                            Err(DmxUartDriverError::DriverError(Rp2040DriverError::Framing))
                        }
                    },
                    nb::Error::WouldBlock => {
                        if self.countdown.wait() != Err(nb::Error::WouldBlock) {
                            if head == 0 {
                                return Err(DmxUartDriverError::TimeoutError);
                            }

                            return Ok(head);
                        }

                        continue;
                    }
                },
            }?;

            head += bytes_read;
        }

        self.countdown.cancel().unwrap();

        Ok(head)
    }
}

impl<D: UartDevice, P: ValidUartPinout<D>> DmxRespUartDriver for Rp2040Driver<'_, D, P> {
    fn write_frames(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        self.begin_package();
        self.write_frames_no_break(buffer)
    }

    fn write_frames_no_break(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>> {
        self.uart.write_full_blocking(buffer);
        while self.uart.uart_is_busy() {}
        Ok(buffer.len())
    }
}
