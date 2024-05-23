use crate::command_class::RequestCommandClass;
use crate::dmx_driver::{
    ControllerDriverErrorDef, CustomStartCodeControllerDriver, DiscoveryOption,
    DmxControllerDriver, DmxError, RdmControllerDriver,
};
use crate::rdm_data::{RdmData, RdmRequestData};
use crate::rdm_packages::{
    deserialize_identify, deserialize_status_messages, deserialize_supported_parameters,
    RdmResponseInfo, RdmResponsePackage,
};
use crate::rdm_types::{
    DeviceInfo, DiscoveryMuteResponse, DmxStartAddress, OverflowMessageResp, StatusMessages,
    StatusType, SupportedParameters,
};
use crate::types::{DataPack, NackReason, ResponseType};
use crate::unique_identifier::{PackageAddress, UniqueIdentifier};
use crate::{pids, rdm_packages, rdm_types};

#[derive(Debug)]
pub struct DmxControllerConfig {
    pub rdm_uid: UniqueIdentifier,
}

impl Default for DmxControllerConfig {
    fn default() -> Self {
        Self {
            rdm_uid: UniqueIdentifier::new(0x7FF0, 0).unwrap(), // prototyping id
        }
    }
}

#[derive(Debug)]
pub struct RdmRequest {
    /// The unique id of the recipient of the request.
    pub destination_uid: PackageAddress,
    /// The id that specifies the type of the package.
    pub parameter_id: u16,
    /// The parameter data.
    pub data: DataPack,
}

impl RdmRequest {
    /// Creates an RdmRequest with empty parameter data.
    pub fn empty(uid: PackageAddress, pid: u16) -> Self {
        Self {
            destination_uid: uid,
            parameter_id: pid,
            data: heapless::Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum RdmResponse {
    /// The message data of the response.
    Response(RdmResponseInfo),
    /// The request has been excepted but the message data is too big to fit into one response.
    /// Use the get command on the same pid to receive the rest of it until you just receive a Response.
    IncompleteResponse(RdmResponseInfo),
    /// No response was received since the request was a broadcast.
    RequestWasBroadcast,
}

/// An RDM controller
pub struct DmxController<C: ControllerDriverErrorDef> {
    driver: C,
    uid: UniqueIdentifier,
    current_transaction_id: u8,
    last_message_count: u8,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RdmResponseError<E> {
    /// The received package doesn't match the request.
    NotMatching,
    /// The parameter data couldn't be deserialized.
    ParameterDataNotDeserializable,
    /// The response has an error status but the contents aren't deserializable.
    ErrorNotDeserializable,
    /// The response isn't ready yet. The value is the estimated time in 100ms steps.
    NotReady(u16),
    /// The responder didn't acknowledge the request.
    NotAcknowledged(NackReason),
    /// The underlying dmx controller raised an error.
    DmxError(DmxError<E>),
}

impl<E: core::fmt::Debug> core::fmt::Display for RdmResponseError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl<E> From<DmxError<E>> for RdmResponseError<E> {
    fn from(value: DmxError<E>) -> Self {
        Self::DmxError(value)
    }
}

#[cfg(feature = "std")]
impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for RdmResponseError<E> {}

impl<E> From<rdm_types::DeserializationError> for RdmResponseError<E> {
    fn from(_: rdm_types::DeserializationError) -> Self {
        Self::ParameterDataNotDeserializable
    }
}

impl<D: ControllerDriverErrorDef> DmxController<D> {
    /// Creates a new DmxManager instance.
    pub fn new(driver: D, config: &DmxControllerConfig) -> Self {
        Self {
            driver,
            uid: config.rdm_uid,
            current_transaction_id: 0,
            last_message_count: 0,
        }
    }

    /// Get a reference to the underlying driver.
    pub fn get_driver(&mut self) -> &mut D {
        &mut self.driver
    }
}

impl<D: CustomStartCodeControllerDriver> DmxController<D> {
    /// Sends a package with a custom start code.
    pub fn send_custom_package(
        &mut self,
        start_code: u8,
        package: &[u8],
    ) -> Result<(), RdmResponseError<D::DriverError>> {
        self.driver
            .send_custom_package(start_code, package)
            .map_err(RdmResponseError::DmxError)
    }
}

impl<D: DmxControllerDriver> DmxController<D> {
    /// Sends a dmx package. Package can't be bigger than 512 bytes.
    pub fn send_dmx_package(
        &mut self,
        package: &[u8],
    ) -> Result<(), RdmResponseError<D::DriverError>> {
        self.driver
            .send_dmx_package(package)
            .map_err(RdmResponseError::DmxError)
    }
}

impl<D: RdmControllerDriver> DmxController<D> {
    fn rdm_request(
        &mut self,
        command_class: RequestCommandClass,
        request: RdmRequest,
    ) -> Result<RdmResponse, RdmResponseError<D::DriverError>> {
        self.current_transaction_id = self.current_transaction_id.wrapping_add(1);

        self.driver.send_rdm(RdmData::Request(RdmRequestData {
            destination_uid: request.destination_uid,
            source_uid: self.uid,
            transaction_number: self.current_transaction_id,
            port_id: 0,
            message_count: 0,
            sub_device: 0,
            command_class,
            parameter_id: request.parameter_id,
            parameter_data: request.data,
        }))?;

        if request.destination_uid.is_broadcast() {
            return Ok(RdmResponse::RequestWasBroadcast);
        }

        let response = loop {
            let response = match self.driver.receive_rdm()? {
                RdmData::Request(_) => {
                    return Err(RdmResponseError::NotMatching);
                },
                RdmData::Response(response) => response,
            };

            if self.current_transaction_id == response.transaction_number {
                break response;
            }
        };

        if response.destination_uid != PackageAddress::Device(self.uid) {
            return Err(RdmResponseError::NotMatching);
        }

        let response_info = RdmResponseInfo {
            parameter_id: response.parameter_id,
            message_count: response.message_count,
            data: response.parameter_data,
        };

        self.last_message_count = response.message_count;

        match response.response_type {
            ResponseType::ResponseTypeAck => Ok(RdmResponse::Response(response_info)),
            ResponseType::ResponseTypeAckTimer => {
                if response_info.data.len() != 2 {
                    return Err(RdmResponseError::ErrorNotDeserializable);
                }

                Err(RdmResponseError::NotReady(u16::from_be_bytes(
                    response_info.data[..2].try_into().unwrap(),
                )))
            },
            ResponseType::ResponseTypeNackReason => {
                if response_info.data.len() != 2 {
                    return Err(RdmResponseError::ErrorNotDeserializable);
                }

                let nack_reason = u16::from_be_bytes(response_info.data[..2].try_into().unwrap())
                    .try_into()
                    .or(Err(RdmResponseError::ErrorNotDeserializable))?;

                Err(RdmResponseError::NotAcknowledged(nack_reason))
            },
            ResponseType::ResponseTypeAckOverflow => {
                Ok(RdmResponse::IncompleteResponse(response_info))
            },
        }
    }

    /// Sends a get request.
    pub fn rdm_get(
        &mut self,
        request: RdmRequest,
    ) -> Result<RdmResponse, RdmResponseError<D::DriverError>> {
        self.rdm_request(RequestCommandClass::GetCommand, request)
    }

    /// Sends a set request.
    pub fn rdm_set(
        &mut self,
        request: RdmRequest,
    ) -> Result<RdmResponse, RdmResponseError<D::DriverError>> {
        self.rdm_request(RequestCommandClass::SetCommand, request)
    }

    /// Sends a discovery request to a range of device ids and returns the found uid
    /// if there is no collision and the device does not have its discovery muted.
    pub fn rdm_discover(
        &mut self,
        first_uid: u64,
        last_uid: u64,
    ) -> Result<DiscoveryOption, RdmResponseError<D::DriverError>> {
        let mut parameter_data = heapless::Vec::new();

        parameter_data
            .extend_from_slice(&first_uid.to_be_bytes()[2..8])
            .unwrap();
        parameter_data
            .extend_from_slice(&last_uid.to_be_bytes()[2..8])
            .unwrap();

        self.driver.send_rdm(RdmData::Request(RdmRequestData {
            destination_uid: PackageAddress::Broadcast,
            source_uid: self.uid,
            transaction_number: self.current_transaction_id,
            port_id: 0,
            message_count: 0,
            sub_device: 0,
            command_class: RequestCommandClass::DiscoveryCommand,
            parameter_id: pids::DISC_UNIQUE_BRANCH,
            parameter_data,
        }))?;

        Ok(self.driver.receive_rdm_discovery_response()?)
    }

    /// Mute device from discovery. It will not respond to discovery requests anymore.
    /// Returns None if the request was a broadcast.
    pub fn rdm_disc_mute(
        &mut self,
        uid: PackageAddress,
    ) -> Result<Option<DiscoveryMuteResponse>, RdmResponseError<D::DriverError>> {
        let response = self.rdm_request(
            RequestCommandClass::DiscoveryCommand,
            RdmRequest::empty(uid, pids::DISC_MUTE),
        )?;

        deserialize_discovery_mute_response::<D>(&response)
    }

    /// Unmute device from discovery. It will respond to discovery requests again.
    /// Returns None if the request was a broadcast.
    pub fn rdm_disc_un_mute(
        &mut self,
        uid: PackageAddress,
    ) -> Result<Option<DiscoveryMuteResponse>, RdmResponseError<D::DriverError>> {
        let response = self.rdm_request(
            RequestCommandClass::DiscoveryCommand,
            RdmRequest::empty(uid, pids::DISC_UN_MUTE),
        )?;

        deserialize_discovery_mute_response::<D>(&response)
    }

    /// Get the identify state in the rdm device (led for searching)
    pub fn rdm_get_identify(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<bool, RdmResponseError<D::DriverError>> {
        let response = self.rdm_get(RdmRequest::empty(
            PackageAddress::Device(uid),
            pids::IDENTIFY_DEVICE,
        ))?;

        match response {
            RdmResponse::Response(response_info) => Ok(deserialize_identify(&response_info.data)?),
            _ => Err(RdmResponseError::ParameterDataNotDeserializable),
        }
    }

    /// Set the identify state in the rdm device (led for searching)
    pub fn rdm_set_identify(
        &mut self,
        uid: PackageAddress,
        enabled: bool,
    ) -> Result<(), RdmResponseError<D::DriverError>> {
        self.rdm_set(RdmRequest {
            destination_uid: uid,
            parameter_id: pids::IDENTIFY_DEVICE,
            data: heapless::Vec::from_slice(&[enabled as u8]).unwrap(),
        })?;

        Ok(())
    }

    /// Get the software version label.
    pub fn rdm_get_software_version_label(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<heapless::String<32>, RdmResponseError<D::DriverError>> {
        let response_info = match self.rdm_get(RdmRequest::empty(
            PackageAddress::Device(uid),
            pids::SOFTWARE_VERSION_LABEL,
        ))? {
            RdmResponse::Response(response_info) => response_info,
            _ => return Err(RdmResponseError::ParameterDataNotDeserializable),
        };

        Ok(rdm_packages::deserialize_software_version_label(
            &response_info.data,
        )?)
    }

    /// Get the current start address of the dmx slave.
    pub fn rdm_get_dmx_start_address(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<DmxStartAddress, RdmResponseError<D::DriverError>> {
        let response = match self.rdm_get(RdmRequest::empty(
            PackageAddress::Device(uid),
            pids::DMX_START_ADDRESS,
        ))? {
            RdmResponse::Response(response) => response,
            _ => return Err(RdmResponseError::ParameterDataNotDeserializable),
        };

        Ok(DmxStartAddress::deserialize(&response.data)?)
    }

    /// Set the current start address of the dmx slave. The address has to be between 1 and 512.
    pub fn rdm_set_dmx_start_address(
        &mut self,
        uid: PackageAddress,
        start_address: u16,
    ) -> Result<(), RdmResponseError<D::DriverError>> {
        assert!(
            (1..=512).contains(&start_address),
            "The requested start address is not valid."
        );

        self.rdm_set(RdmRequest {
            destination_uid: uid,
            parameter_id: pids::DMX_START_ADDRESS,
            data: DataPack::from_slice(&start_address.to_be_bytes()).unwrap(),
        })?;

        Ok(())
    }

    /// Get the last queued message.
    ///
    /// Use [DmxController::rdm_get_last_message_count]
    /// to receive the message count from the last request.
    ///
    /// If no messages are queued this will return the current [StatusMessages].
    /// You can use the [StatusType] to filter these [StatusMessages].
    ///
    /// Note that you can only use [StatusType::StatusAdvisory], [StatusType::StatusWarning],
    /// [StatusType::StatusError] and [StatusType::StatusGetLastMessage].
    ///
    /// If you want to receive the previous response use [StatusType::StatusGetLastMessage].
    pub fn rdm_get_queued_message(
        &mut self,
        uid: UniqueIdentifier,
        status_requested: StatusType,
    ) -> Result<RdmResponsePackage, RdmResponseError<D::DriverError>> {
        let response = self.rdm_get(RdmRequest {
            destination_uid: PackageAddress::Device(uid),
            parameter_id: pids::QUEUED_MESSAGE,
            data: DataPack::from_slice(&[status_requested as u8]).unwrap(),
        })?;

        match response {
            RdmResponse::Response(response_info) => {
                Ok(RdmResponsePackage::from_response_info(response_info)?)
            },
            _ => Err(RdmResponseError::ParameterDataNotDeserializable),
        }
    }

    /// Get status messages. Filter severity by using the [StatusType::StatusAdvisory], [StatusType::StatusWarning]
    /// and [StatusType::StatusError].
    ///
    /// If you want to receive the previously set of status messages again use [StatusType::StatusGetLastMessage].
    /// To perform an availability test use [StatusType::StatusNone].
    ///
    /// If this parameter message is properly implemented on the slave you
    /// should never get a [OverflowMessageResp::Incomplete] back, since STATUS_MESSAGE uses
    /// its own queuing.
    pub fn rdm_get_status_messages(
        &mut self,
        uid: UniqueIdentifier,
        status_requested: StatusType,
    ) -> Result<OverflowMessageResp<StatusMessages>, RdmResponseError<D::DriverError>> {
        let response = self.rdm_get(RdmRequest {
            destination_uid: PackageAddress::Device(uid),
            parameter_id: pids::STATUS_MESSAGES,
            data: DataPack::from_slice(&[status_requested as u8]).unwrap(),
        })?;

        match response {
            RdmResponse::Response(response_info) => Ok(OverflowMessageResp::Complete(
                deserialize_status_messages(&response_info.data)?,
            )),
            RdmResponse::IncompleteResponse(response_info) => Ok(OverflowMessageResp::Incomplete(
                deserialize_status_messages(&response_info.data)?,
            )),
            _ => Err(RdmResponseError::ParameterDataNotDeserializable),
        }
    }

    /// Get the parameter ids that are supported by the responder.
    ///
    /// <div class="warning">Note that this only includes optional parameter ids that are not
    /// required to be compliant with ANSI E1.20.</div>
    pub fn rdm_get_supported_parameters(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<OverflowMessageResp<SupportedParameters>, RdmResponseError<D::DriverError>> {
        let response = self.rdm_get(RdmRequest::empty(
            PackageAddress::Device(uid),
            pids::SUPPORTED_PARAMETERS,
        ))?;

        match response {
            RdmResponse::Response(response_info) => Ok(OverflowMessageResp::Complete(
                deserialize_supported_parameters(&response_info.data)?,
            )),
            RdmResponse::IncompleteResponse(response_info) => Ok(OverflowMessageResp::Incomplete(
                deserialize_supported_parameters(&response_info.data)?,
            )),
            _ => Err(RdmResponseError::ParameterDataNotDeserializable),
        }
    }

    /// Get the device info from the rdm device.
    pub fn rdm_get_device_info(
        &mut self,
        uid: UniqueIdentifier,
    ) -> Result<DeviceInfo, RdmResponseError<D::DriverError>> {
        let response = self.rdm_get(RdmRequest::empty(
            PackageAddress::Device(uid),
            pids::DEVICE_INFO,
        ))?;
        match response {
            RdmResponse::Response(response_info) => {
                Ok(DeviceInfo::deserialize(&response_info.data)?)
            },
            _ => Err(RdmResponseError::ParameterDataNotDeserializable),
        }
    }

    /// Returns the message count that was received on the last request using this instance.
    pub fn rdm_get_last_message_count(&self) -> u8 {
        self.last_message_count
    }
}

fn deserialize_discovery_mute_response<D: RdmControllerDriver>(
    response: &RdmResponse,
) -> Result<Option<DiscoveryMuteResponse>, RdmResponseError<D::DriverError>> {
    Ok(match response {
        RdmResponse::Response(response_info) => Some(
            DiscoveryMuteResponse::deserialize(&response_info.data)
                .map_err(|_| RdmResponseError::ParameterDataNotDeserializable)?,
        ),
        RdmResponse::RequestWasBroadcast => None,
        RdmResponse::IncompleteResponse(_) => {
            return Err(RdmResponseError::ParameterDataNotDeserializable)
        },
    })
}
