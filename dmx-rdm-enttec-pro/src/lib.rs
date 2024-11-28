//! Library for using an Enttec DMX Pro (OpenDMX devices won't work)
//! with the [dmx-rdm-rs](https://crates.io/crates/dmx-rdm) library.
//!
//! It is important that the Enttec DMX Pro is flashed with the RDM firmware.
//! Refer to the [official api documentation](https://cdn.enttec.com/pdf/assets/70304/70304_DMX_USB_PRO_API.pdf)
//! for more information.
//!
//! ## Example
//! ```
//! use libftd2xx::Ftdi;
//! use dmx_rdm_enttec_pro::create_dmx_controller_from_enttec_pro;
//!
//! let mut dmx_controller = create_dmx_controller_from_enttec_pro(Ftdi::with_index(0).unwrap()).unwrap();
//! ```

use dmx_rdm::consts::DMX_NULL_START;
use dmx_rdm::dmx_controller::{DmxController, DmxControllerConfig};
use dmx_rdm::dmx_driver::{
    ControllerDriverErrorDef, CustomStartCodeControllerDriver, DiscoveryOption,
    DmxControllerDriver, DmxError, RdmControllerDriver,
};
use dmx_rdm::rdm_data::{deserialize_discovery_response, RdmData, RdmDeserializationError};
use dmx_rdm::unique_identifier::UniqueIdentifier;
use libftd2xx::{FtStatus, Ftdi, FtdiCommon, TimeoutError};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::thread::sleep;
use std::time::Duration;

const ENTTEC_MANUFACTURER_ID: u16 = 0x454E;
const START_OF_MESSAGE_DELIMITER: u8 = 0x7E;
const END_OF_MESSAGE_DELIMITER: u8 = 0xE7;
const MAX_DATA_LENGTH: usize = 600;
const MIN_PACKAGE_SIZE: usize = 5;

const RECEIVED_DMX_PACKET: u8 = 5;
const SEND_DMX_PACKET_REQUEST: u8 = 6;
const SEND_RDM_PACKET_REQUEST: u8 = 7;
const GET_WIDGET_SERIAL_NUMBER: u8 = 10;
const SEND_RDM_DISCOVERY_REQUEST: u8 = 11;

#[derive(Debug, Clone)]
struct EnttecMessage {
    pub label: u8,
    pub data: Vec<u8>,
}

impl EnttecMessage {
    pub fn serialize(&self) -> Vec<u8> {
        assert!(self.data.len() <= MAX_DATA_LENGTH);

        let mut result = Vec::new();
        result.push(START_OF_MESSAGE_DELIMITER);
        result.push(self.label);
        result.extend_from_slice(&(self.data.len() as u16).to_le_bytes());
        result.extend_from_slice(&self.data);
        result.push(END_OF_MESSAGE_DELIMITER);

        result
    }

    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < MIN_PACKAGE_SIZE {
            return None;
        }

        if data[0] != START_OF_MESSAGE_DELIMITER || data[data.len() - 1] != END_OF_MESSAGE_DELIMITER
        {
            return None;
        }

        let label = data[1];
        let data_length = u16::from_le_bytes(data[2..4].try_into().unwrap()) as usize;
        if data_length > MAX_DATA_LENGTH || data.len() != MIN_PACKAGE_SIZE + data_length {
            return None;
        }

        let mut enttec_data = Vec::new();
        enttec_data.extend_from_slice(&data[4..data_length + 4]);

        Some(Self {
            label,
            data: enttec_data,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EnttecProError {
    /// The data size in the received package is too big.
    LengthOutOfRange,
    /// The received package couldn't been deserialized.
    EnttecDeserializationError,
    /// The received rdm package couldn't been deserialized.
    RdmDeserializationError(RdmDeserializationError),
    /// An error was raised by the ftdi library.
    FtdiError(TimeoutError),
}

impl From<TimeoutError> for EnttecProError {
    fn from(value: TimeoutError) -> Self {
        Self::FtdiError(value)
    }
}

impl From<FtStatus> for EnttecProError {
    fn from(value: FtStatus) -> Self {
        Self::FtdiError(value.into())
    }
}

impl Display for EnttecProError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let text_to_write = match self {
            EnttecProError::LengthOutOfRange => "length out of range",
            EnttecProError::EnttecDeserializationError => "package couldn't been deserialized",
            EnttecProError::RdmDeserializationError(rdm_deserialization_error) => {
                return write!(f, "{}", rdm_deserialization_error);
            }
            EnttecProError::FtdiError(ftdi_error) => return write!(f, "{}", ftdi_error),
        };

        write!(f, "{}", text_to_write)
    }
}

impl Error for EnttecProError {}

pub struct EnttecProDriver {
    serial_port: Ftdi,
}

impl EnttecProDriver {
    pub fn new(mut serial_port: Ftdi) -> Result<Self, EnttecProError> {
        serial_port.set_timeouts(Duration::from_millis(50), Duration::from_millis(50))?;

        Ok(Self { serial_port })
    }

    pub fn get_rdm_uid(&mut self) -> Result<UniqueIdentifier, EnttecProError> {
        self.serial_port.write_all(
            &EnttecMessage {
                label: GET_WIDGET_SERIAL_NUMBER,
                data: Vec::new(),
            }
            .serialize(),
        )?;

        let response = loop {
            let response = self.read_package()?;

            if response.label == GET_WIDGET_SERIAL_NUMBER {
                break response;
            }
        };

        if response.data.len() != 4 {
            return Err(EnttecProError::LengthOutOfRange);
        }

        let device_address = u32::from_le_bytes(response.data.try_into().unwrap());

        Ok(UniqueIdentifier::new(ENTTEC_MANUFACTURER_ID, device_address).unwrap())
    }

    fn read_package(&mut self) -> Result<EnttecMessage, EnttecProError> {
        let mut receive_buffer = [0u8; 605];
        loop {
            self.serial_port.read_all(&mut receive_buffer[..1])?;

            if receive_buffer[0] == START_OF_MESSAGE_DELIMITER {
                break;
            }
        }

        self.serial_port.read_all(&mut receive_buffer[1..4])?;
        let data_size = u16::from_le_bytes(receive_buffer[2..4].try_into().unwrap()) as usize;

        if data_size > MAX_DATA_LENGTH {
            return Err(EnttecProError::LengthOutOfRange);
        }

        self.serial_port
            .read_all(&mut receive_buffer[4..data_size + 5])?;

        Ok(
            match EnttecMessage::deserialize(&receive_buffer[0..data_size + 5]) {
                None => return Err(EnttecProError::EnttecDeserializationError),
                Some(message) => message,
            },
        )
    }
}

impl ControllerDriverErrorDef for EnttecProDriver {
    type DriverError = EnttecProError;
}

impl CustomStartCodeControllerDriver for EnttecProDriver {
    fn send_custom_package(
        &mut self,
        start_code: u8,
        package: &[u8],
    ) -> Result<(), DmxError<Self::DriverError>> {
        let mut data = Vec::new();

        data.push(start_code);
        data.extend_from_slice(package);

        self.serial_port
            .write(
                &EnttecMessage {
                    label: SEND_DMX_PACKET_REQUEST,
                    data,
                }
                .serialize(),
            )
            .map_err(|error| DmxError::DriverError(error.into()))?;

        Ok(())
    }
}

impl DmxControllerDriver for EnttecProDriver {
    fn send_dmx_package(&mut self, package: &[u8]) -> Result<(), DmxError<Self::DriverError>> {
        self.send_custom_package(DMX_NULL_START, package)
    }
}

impl RdmControllerDriver for EnttecProDriver {
    fn send_rdm(&mut self, package: RdmData) -> Result<(), DmxError<Self::DriverError>> {
        let label = if let RdmData::Request(ref request) = package {
            if request.parameter_id == 0x0001 {
                SEND_RDM_DISCOVERY_REQUEST
            } else {
                SEND_RDM_PACKET_REQUEST
            }
        } else {
            SEND_RDM_PACKET_REQUEST
        };

        self.serial_port
            .write(
                &EnttecMessage {
                    label,
                    data: package.serialize().to_vec(),
                }
                .serialize(),
            )
            .map_err(|error| DmxError::DriverError(error.into()))?;

        sleep(Duration::from_millis(5));

        Ok(())
    }

    fn receive_rdm(&mut self) -> Result<RdmData, DmxError<Self::DriverError>> {
        let package = loop {
            let recv_package = self.read_package()?;
            if recv_package.label == RECEIVED_DMX_PACKET {
                break recv_package;
            }
        };

        Ok(RdmData::deserialize(&package.data[1..])
            .map_err(EnttecProError::RdmDeserializationError)?)
    }

    fn receive_rdm_discovery_response(
        &mut self,
    ) -> Result<DiscoveryOption, DmxError<Self::DriverError>> {
        let package = loop {
            let recv_package = match self.read_package() {
                Ok(recv_package) => recv_package,
                Err(EnttecProError::FtdiError(TimeoutError::Timeout { .. })) => {
                    return Ok(DiscoveryOption::NoDevice)
                }
                other => other?,
            };

            if recv_package.label == RECEIVED_DMX_PACKET {
                break recv_package;
            }
        };

        let discovery_option = match deserialize_discovery_response(&package.data[1..]) {
            Ok(unique_identifier) => DiscoveryOption::Found(unique_identifier),
            Err(_) => DiscoveryOption::Collision,
        };

        Ok(discovery_option)
    }

    fn send_rdm_discovery_response(
        &mut self,
        _uid: UniqueIdentifier,
    ) -> Result<(), DmxError<Self::DriverError>> {
        // The Enttec DMX Pro does this by itself.
        Ok(())
    }
}

pub fn create_dmx_controller_from_enttec_pro(
    serial_port: Ftdi,
) -> Result<DmxController<EnttecProDriver>, EnttecProError> {
    let mut driver = EnttecProDriver::new(serial_port)?;
    let rdm_uid = driver.get_rdm_uid()?;

    Ok(DmxController::new(driver, &DmxControllerConfig { rdm_uid }))
}
