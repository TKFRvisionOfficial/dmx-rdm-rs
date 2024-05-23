use crate::consts::{
    DMX_MAX_PACKAGE_SIZE, DMX_NULL_START, PREAMBLE_BYTE, RDM_DISCOVERY_RESPONSE_SIZE,
    RDM_MAX_PACKAGE_SIZE, SC_RDM, SEPARATOR_BYTE,
};
use crate::dmx_receiver::DmxFrame;
use crate::dmx_uart_driver::{
    DmxRecvUartDriver, DmxRespUartDriver, DmxUartDriver, DmxUartDriverError,
};
use crate::rdm_data::{deserialize_discovery_response, RdmData, RdmDeserializationError};
use crate::unique_identifier::UniqueIdentifier;
use crate::utils::{calculate_checksum, encode_disc_unique};

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DmxError<E> {
    /// There were fewer bytes written to the uart then there should have been.
    UartOverflow,
    /// The request timed time out.
    /// **Important:** If you implement a driver make sure this error gets raised instead
    /// of a driver specific error.
    TimeoutError,
    /// Raised when an RDM package could not be deserialized.
    DeserializationError(RdmDeserializationError),
    /// An error raised by the uart driver.
    DriverError(E),
}

impl<E: core::fmt::Display> core::fmt::Display for DmxError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DmxError::UartOverflow => write!(f, "uart overflowed"),
            DmxError::TimeoutError => write!(f, "request timed out"),
            DmxError::DeserializationError(error) => error.fmt(f),
            DmxError::DriverError(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "std")]
impl<E: core::fmt::Display + core::fmt::Debug> std::error::Error for DmxError<E> {}

#[derive(Debug)]
pub enum DiscoveryOption {
    /// No device responded to the discovery request.
    /// There aren't any devices in the specified unique id range.
    NoDevice,
    /// The response to the discovery request couldn't be deserialized.
    /// There are multiple devices in the specified unique id range.
    Collision,
    /// The discovery response was successfully deserialized.
    /// There is only one device in the specified unique id range.
    Found(UniqueIdentifier),
}

impl<E> From<DmxUartDriverError<E>> for DmxError<E> {
    fn from(value: DmxUartDriverError<E>) -> Self {
        match value {
            DmxUartDriverError::TimeoutError => Self::TimeoutError,
            DmxUartDriverError::DriverError(driver_error) => Self::DriverError(driver_error),
        }
    }
}

/// Trait that ensures that the same Error is used in the [DmxControllerDriver] as well as the [RdmControllerDriver].
pub trait ControllerDriverErrorDef {
    /// The driver specific error.
    type DriverError;
}

/// Trait for controlling DMX fixtures.
pub trait DmxControllerDriver: ControllerDriverErrorDef {
    /// Send a DMX512 package. It shouldn't be bigger than 512 bytes.
    fn send_dmx_package(&mut self, package: &[u8]) -> Result<(), DmxError<Self::DriverError>>;
}

/// Trait for sending and receiving RDM packages from a controller point of view.
pub trait RdmControllerDriver: ControllerDriverErrorDef {
    /// Sends an RDM package.
    fn send_rdm(&mut self, package: RdmData) -> Result<(), DmxError<Self::DriverError>>;
    /// Receives an RDM package.
    fn receive_rdm(&mut self) -> Result<RdmData, DmxError<Self::DriverError>>;
    /// Receives an RDM discovery response.
    /// Returns the received device id.
    fn receive_rdm_discovery_response(
        &mut self,
    ) -> Result<DiscoveryOption, DmxError<Self::DriverError>>;
    /// Send a dmx discovery response. If this functionality is already been solved
    /// by the device add hand, provide an empty function.
    fn send_rdm_discovery_response(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<(), DmxError<Self::DriverError>>;
}

/// Trait for implementing packages with custom start codes.
pub trait CustomStartCodeControllerDriver: ControllerDriverErrorDef {
    /// Sends a package with a custom start code.
    fn send_custom_package(
        &mut self,
        start_code: u8,
        package: &[u8],
    ) -> Result<(), DmxError<Self::DriverError>>;
}

impl<D: DmxUartDriver> ControllerDriverErrorDef for D {
    type DriverError = D::DriverError;
}

impl<D: DmxRespUartDriver> CustomStartCodeControllerDriver for D {
    fn send_custom_package(
        &mut self,
        start_code: u8,
        package: &[u8],
    ) -> Result<(), DmxError<Self::DriverError>> {
        let mut frame_buffer: heapless::Vec<_, DMX_MAX_PACKAGE_SIZE> = heapless::Vec::new();
        frame_buffer.push(start_code).unwrap();
        frame_buffer
            .extend_from_slice(package)
            .or(Err(DmxError::UartOverflow))?;

        if self.write_frames(&frame_buffer)? != package.len() {
            return Err(DmxError::UartOverflow);
        }

        Ok(())
    }
}

impl<D: DmxRespUartDriver> DmxControllerDriver for D {
    fn send_dmx_package(&mut self, package: &[u8]) -> Result<(), DmxError<Self::DriverError>> {
        self.send_custom_package(DMX_NULL_START, package)
    }
}

const READ_TIMEOUT_US: u32 = 2800;
impl<D: DmxRespUartDriver + DmxRecvUartDriver> RdmControllerDriver for D {
    fn send_rdm(&mut self, rdm_package: RdmData) -> Result<(), DmxError<Self::DriverError>> {
        let serialized_package = rdm_package.serialize();
        let written_bytes = self.write_frames(&serialized_package)?;

        if serialized_package.len() != written_bytes {
            return Err(DmxError::UartOverflow);
        }

        Ok(())
    }

    fn receive_rdm(&mut self) -> Result<RdmData, DmxError<Self::DriverError>> {
        // Very imprecise value for testing
        let mut receive_buffer = [0u8; RDM_MAX_PACKAGE_SIZE];
        let mut bytes_read = self.read_frames(&mut receive_buffer[0..3], READ_TIMEOUT_US)?;

        // plus two checksum bytes
        let message_length = receive_buffer[2] as usize + 2;
        if message_length < 3 {
            return Err(DmxError::DeserializationError(
                RdmDeserializationError::WrongMessageLength(message_length),
            ));
        }
        if message_length > RDM_MAX_PACKAGE_SIZE {
            return Err(DmxError::DeserializationError(
                RdmDeserializationError::WrongMessageLength(message_length),
            ));
        }

        bytes_read +=
            self.read_frames_no_break(&mut receive_buffer[3..message_length], READ_TIMEOUT_US)?;
        let response = RdmData::deserialize(&receive_buffer[..bytes_read])
            .map_err(DmxError::DeserializationError)?;

        Ok(response)
    }

    fn receive_rdm_discovery_response(
        &mut self,
    ) -> Result<DiscoveryOption, DmxError<Self::DriverError>> {
        let mut receive_buffer = [0u8; 32]; // the actual package is 24 bytes
        let bytes_read = match self.read_frames_no_break(&mut receive_buffer, READ_TIMEOUT_US) {
            Err(DmxUartDriverError::TimeoutError) => return Ok(DiscoveryOption::NoDevice),
            result => result,
        }?;

        if bytes_read < RDM_DISCOVERY_RESPONSE_SIZE {
            // Is this a collision?
            return Err(DmxError::DeserializationError(
                RdmDeserializationError::BufferTooSmall,
            ));
        }

        Ok(
            deserialize_discovery_response(&receive_buffer[..bytes_read])
                .map_or(DiscoveryOption::Collision, DiscoveryOption::Found),
        )
    }

    fn send_rdm_discovery_response(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<(), DmxError<Self::DriverError>> {
        let mut frame_buffer = [PREAMBLE_BYTE; 24];
        frame_buffer[7] = SEPARATOR_BYTE;

        let uid_buffer = uid.to_bytes();
        encode_disc_unique(&uid_buffer, &mut frame_buffer[8..20]);

        let checksum = calculate_checksum(&frame_buffer[8..20]);
        encode_disc_unique(&checksum.to_be_bytes(), &mut frame_buffer[20..24]);

        self.write_frames_no_break(&frame_buffer)?;

        Ok(())
    }
}

pub trait DmxReceiver: ControllerDriverErrorDef {
    /// Receive a DMX512 package.
    fn receive_package(&mut self) -> Result<DmxFrame, DmxError<Self::DriverError>>;
}

impl<D: DmxRecvUartDriver> DmxReceiver for D {
    fn receive_package(&mut self) -> Result<DmxFrame, DmxError<D::DriverError>> {
        const READ_TIMEOUT_US: u32 = 2800;

        let mut buffer = [0u8; DMX_MAX_PACKAGE_SIZE];
        let mut bytes_read = self.read_frames(&mut buffer[0..3], READ_TIMEOUT_US)?;
        if bytes_read < 2 {
            return Err(DmxError::DeserializationError(
                RdmDeserializationError::WrongMessageLength(bytes_read),
            ));
        }

        // workaround for rdm packages to have better receive times
        let start_code = buffer[0];
        let message_size = if start_code == SC_RDM && bytes_read == 3 {
            // message size plus two checksum bytes
            buffer[2] as usize + 2
        } else {
            DMX_MAX_PACKAGE_SIZE
        };

        bytes_read += self.read_frames_no_break(&mut buffer[3..message_size], READ_TIMEOUT_US)?;
        Ok(DmxFrame::from_slice(&buffer[..bytes_read]).unwrap())
    }
}
