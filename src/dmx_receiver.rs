use crate::consts::SC_RDM;
use crate::dmx_driver::{DmxError, DmxReceiver, RdmControllerDriver};
use crate::rdm_data::{RdmData, RdmDeserializationError, RdmRequestData, RdmResponseData};
use crate::rdm_responder::{
    DmxReceiverContext, RdmAnswer, RdmResponderConfig, RdmResponderHandlerFunc,
    RdmResponderPackageHandler, RdmResult,
};
use crate::rdm_types::StatusMessage;
use crate::types::NackReason;

/// A vector that contains one DmxFrame. The first byte is the start code. 0x00 is the dmx start code.
pub type DmxFrame = heapless::Vec<u8, 513>;

pub enum ResponseOption {
    NoResponse,
    Response(DmxFrame),
    ResponseNoBreak(DmxFrame),
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct HandlePackageError;

impl core::fmt::Display for HandlePackageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Couldn't handle package.")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for HandlePackageError {}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Errors that can happen during polling. These errors should not cause panics.
pub enum PollingError<DriverError, HandlerError> {
    /// There were fewer bytes written to the uart then there should have been.
    UartOverflow,
    /// The request timed time out.
    /// **Important:** If you implement a driver make sure this error gets raised instead
    /// of a driver specific error.
    TimeoutError,
    /// The start code is unknown.
    UnknownStartCode,
    /// The package size is insufficient.
    WrongPackageSize,
    /// The received package doesn't match the request.
    NotMatching,
    /// A driver specific error occurred.
    DriverError(DriverError),
    /// A handler specific error occurred.
    HandlerError(HandlerError),
    /// Raised when an RDM package could not be deserialized.
    DeserializationError(RdmDeserializationError),
}

impl<DriverError, HandlerError> From<DmxError<DriverError>>
    for PollingError<DriverError, HandlerError>
{
    fn from(value: DmxError<DriverError>) -> Self {
        match value {
            DmxError::UartOverflow => Self::UartOverflow,
            DmxError::TimeoutError => Self::TimeoutError,
            DmxError::DeserializationError(deserialization_error) => {
                Self::DeserializationError(deserialization_error)
            },
            DmxError::DriverError(driver_error) => Self::DriverError(driver_error),
        }
    }
}

impl<DriverError: core::fmt::Display, HandlerError: core::fmt::Display> core::fmt::Display
    for PollingError<DriverError, HandlerError>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let error_message = match self {
            PollingError::UartOverflow => "Uart overflow.",
            PollingError::TimeoutError => "Timeout error.",
            PollingError::DeserializationError(_) => "Deserialization error.",
            PollingError::NotMatching => "Received response and not request.",
            PollingError::UnknownStartCode => "The start code is unknown.",
            PollingError::WrongPackageSize => "The package size is insufficient.",
            PollingError::DriverError(error) => {
                return core::fmt::Display::fmt(error, f);
            },
            PollingError::HandlerError(error) => {
                return core::fmt::Display::fmt(error, f);
            },
        };

        write!(f, "{}", error_message)
    }
}

#[cfg(feature = "std")]
impl<
        DriverError: core::fmt::Display + core::fmt::Debug,
        HandlerError: core::fmt::Display + core::fmt::Debug,
    > std::error::Error for PollingError<DriverError, HandlerError>
{
}

/// A handler for dmx and custom rdm packages.
pub trait DmxResponderHandler {
    type Error;

    /// Handle rdm requests that aren't handled by the [RdmResponder] itself.
    fn handle_rdm(
        &mut self,
        _request: &RdmRequestData,
        _context: &mut DmxReceiverContext,
    ) -> Result<RdmResult, Self::Error> {
        Ok(RdmResult::NotAcknowledged(
            NackReason::UnsupportedCommandClass as u16,
        ))
    }

    /// Handle all received frames that have a different start code than `0xCC` (the rdm start code).
    /// The first byte is the start code. If start code is `0x00` it's a DMX Package.
    fn handle_dmx(
        &mut self,
        _dmx_frame: DmxFrame,
        _context: &mut DmxReceiverContext,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The structure to build an RDM Receiver.
pub struct RdmResponder<D: DmxReceiver + RdmControllerDriver, const MQ_SIZE: usize> {
    driver: D,
    rdm_receiver_handler: RdmResponderPackageHandler<MQ_SIZE>,
}

impl<D: DmxReceiver + RdmControllerDriver, const MQ_SIZE: usize> RdmResponder<D, MQ_SIZE> {
    /// Creates a new [RdmResponder].
    pub fn new(driver: D, config: RdmResponderConfig) -> Self {
        Self {
            driver,
            rdm_receiver_handler: RdmResponderPackageHandler::new(config),
        }
    }

    /// Call this function as often as you can or on a serial interrupt. It will
    /// receive a package and handle it.
    ///
    /// Returns false if no package was received.
    pub fn poll<HandlerError>(
        &mut self,
        handler: &mut dyn DmxResponderHandler<Error = HandlerError>,
    ) -> Result<bool, PollingError<D::DriverError, HandlerError>> {
        let package = match self.driver.receive_package() {
            Err(DmxError::TimeoutError) => return Ok(false),
            result => result?,
        };

        if package.is_empty() {
            return Err(PollingError::WrongPackageSize);
        }

        let start_code = package[0];
        match start_code {
            SC_RDM => {
                self.handle_rdm(package, handler)?;
            },
            _ => {
                handler
                    .handle_dmx(package, &mut self.rdm_receiver_handler.get_context())
                    .map_err(|error| PollingError::HandlerError(error))?;
            },
        }

        Ok(true)
    }

    fn handle_rdm<HandlerError>(
        &mut self,
        package: DmxFrame,
        handler: &mut dyn DmxResponderHandler<Error = HandlerError>,
    ) -> Result<(), PollingError<D::DriverError, HandlerError>> {
        struct DmxRdmHandlerWrapper<'a, HandlerError> {
            dmx: &'a mut dyn DmxResponderHandler<Error = HandlerError>,
        }

        impl<HandlerError> RdmResponderHandlerFunc for DmxRdmHandlerWrapper<'_, HandlerError> {
            type Error = HandlerError;
            fn handle_rdm(
                &mut self,
                request: &RdmRequestData,
                context: &mut DmxReceiverContext,
            ) -> Result<RdmResult, Self::Error> {
                self.dmx.handle_rdm(request, context)
            }
        }

        let rdm_data =
            RdmData::deserialize(&package).map_err(PollingError::DeserializationError)?;

        let request = match rdm_data {
            RdmData::Request(request) => request,
            _ => return Err(PollingError::NotMatching),
        };

        let response = self
            .rdm_receiver_handler
            .handle_rdm_request(request, &mut DmxRdmHandlerWrapper { dmx: handler })
            .map_err(PollingError::HandlerError)?;

        match response {
            RdmAnswer::Response(response_data) => {
                self.driver
                    .send_rdm(RdmData::Response(response_data))
                    .map_err(|error| match error {
                        DmxError::UartOverflow => PollingError::UartOverflow,
                        DmxError::TimeoutError => PollingError::TimeoutError,
                        DmxError::DeserializationError(deserialization_error) => {
                            PollingError::DeserializationError(deserialization_error)
                        },
                        DmxError::DriverError(driver_error) => {
                            PollingError::DriverError(driver_error)
                        },
                    })?;
            },
            RdmAnswer::DiscoveryResponse(uid) => {
                self.driver.send_rdm_discovery_response(uid)?;
            },
            RdmAnswer::NoResponse => {},
        }

        Ok(())
    }

    /// Get the message queue that contains the results of [RdmResult::AcknowledgedTimer] packages.
    pub fn get_message_queue(&self) -> &heapless::Deque<RdmResponseData, MQ_SIZE> {
        self.rdm_receiver_handler.get_message_queue()
    }

    /// Get the message queue to add the results of [RdmResult::AcknowledgedTimer] packages to.
    pub fn get_message_queue_mut(&mut self) -> &mut heapless::Deque<RdmResponseData, MQ_SIZE> {
        self.rdm_receiver_handler.get_message_queue_mut()
    }

    /// Get the amount of queued messages.
    pub fn get_message_count(&self) -> u8 {
        self.rdm_receiver_handler.get_message_count()
    }

    /// Get the status queue that contains the current status messages.
    pub fn get_status_vec(&self) -> &heapless::Vec<StatusMessage, MQ_SIZE> {
        self.rdm_receiver_handler.get_status_vec()
    }

    /// Get the status queue to add or remove status messages.
    pub fn get_status_vec_mut(&mut self) -> &mut heapless::Vec<StatusMessage, MQ_SIZE> {
        self.rdm_receiver_handler.get_status_vec_mut()
    }
}
