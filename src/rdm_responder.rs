use crate::command_class::RequestCommandClass;
use crate::consts::{RDM_MAX_PARAMETER_DATA_LENGTH, RDM_MAX_STATUS_PACKAGES_PER_REQUEST};
use crate::pids;
use crate::rdm_data::{IsBroadcastError, RdmRequestData, RdmResponseData};
use crate::rdm_types::{
    DeviceInfo, DiscoveryMuteResponse, DmxStartAddress, StatusMessage, StatusType,
};
use crate::types::{DataPack, NackReason, ResponseType};
use crate::unique_identifier::{PackageAddress, UniqueIdentifier};

const INTERNALLY_SUPPORTED_PIDS: [u16; 2] = [pids::QUEUED_MESSAGE, pids::STATUS_MESSAGES];

/// The result object of an RDM handler.
pub enum RdmResult {
    /// The package was acknowledged. The [DataPack] contains the response data.
    Acknowledged(DataPack),
    /// The package was acknowledged, but it does not fit into one [DataPack].
    /// The [DataPack] contains part of the response.
    /// If the RDM-controller requests the same pid and the rest of the message still doesn't fit
    /// doesn't fit into one [DataPack], send the next part as an [RdmResult::AcknowledgedOverflow].
    /// If the rest finally does fit into one [DataPack] send the rest as an [RdmResult::Acknowledged].
    AcknowledgedOverflow(DataPack),
    /// The message was not acknowledged. The [u16] is the [NackReason].
    NotAcknowledged(u16),
    /// The message was acknowledged but a result can not be delivered immediately. The [u16]
    /// contains the amount of time the controller has to wait in 100ms steps.
    AcknowledgedTimer(u16),
    /// The receiver does not respond with anything.
    NoResponse,
    /// A custom response.
    Custom(RdmResponseData),
}

/// A context object for accessing the state of a [RdmResponder] from a [crate::dmx_receiver::DmxResponderHandler].
pub struct DmxReceiverContext<'a> {
    /// The start address of the dmx space.
    pub dmx_start_address: &'a mut DmxStartAddress,
    /// The amount of dmx address allocated.
    pub dmx_footprint: &'a mut u16,
    /// true if the device won't respond to discovery requests.
    pub discovery_muted: &'a mut bool,
    /// The amount of messages in the message queue.
    pub message_count: u8,
}

/// A handler for dmx and custom rdm packages.
pub trait RdmResponderHandlerFunc {
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
}

struct UnfinishedRequest {
    pid: u16,
    iteration: u16,
}

pub enum RdmAnswer {
    /// Has to be sent with an uart break
    Response(RdmResponseData),
    /// Has to be sent without an uart break
    DiscoveryResponse(UniqueIdentifier),
    /// No response to send
    NoResponse,
}

macro_rules! build_nack {
    ($request:path, $nack_reason:path, $message_count:path) => {
        $request.build_response(
            ResponseType::ResponseTypeNackReason,
            $nack_reason.serialize(),
            $message_count,
        )
    };
}

macro_rules! verify_get_request {
    ($request:path, $funcer:path) => {
        if $request.destination_uid.is_broadcast() {
            return None;
        }

        let message_count = $funcer.get_message_count();

        if $request.command_class != RequestCommandClass::GetCommand {
            return build_nack!($request, NackReason::UnsupportedCommandClass, message_count).ok();
        }

        if $request.sub_device != 0 {
            return build_nack!($request, NackReason::SubDeviceOutOfRange, message_count).ok();
        }
    };
}

macro_rules! verify_disc_request {
    ($request:path, $funcer:path) => {
        if $request.command_class != RequestCommandClass::DiscoveryCommand {
            let message_count = $funcer.get_message_count();
            return build_nack!($request, NackReason::UnsupportedCommandClass, message_count).ok();
        }
    };
}

pub struct RdmReceiverMetadata {
    pub device_model_id: u16,
    pub product_category: u16,
    pub software_version_id: u32,
    pub software_version_label: &'static str,
}

impl Default for RdmReceiverMetadata {
    fn default() -> Self {
        Self {
            device_model_id: 0,
            product_category: 0,
            software_version_id: 0,
            software_version_label: "dmx-rdm-rs device",
        }
    }
}

pub struct RdmResponderConfig {
    /// The unique id that is used as a source id in the packages.
    pub uid: UniqueIdentifier,
    /// An array that contains all the supported pids excluding once that are required by the standard.
    pub supported_pids: &'static [u16],
    /// Additional metadata of the RDM-receiver.
    pub rdm_receiver_metadata: RdmReceiverMetadata,
}

pub struct RdmResponderPackageHandler<const MQ_SIZE: usize> {
    /// The start of the dmx address space.
    pub dmx_start_address: DmxStartAddress,
    /// The amount of addresses the dmx device allocates.
    pub dmx_footprint: u16,
    supported_pids: &'static [u16],
    rdm_receiver_metadata: RdmReceiverMetadata,
    uid: UniqueIdentifier,
    discovery_muted: bool,
    unfinished_request: Option<UnfinishedRequest>,
    message_queue: heapless::Deque<RdmResponseData, MQ_SIZE>,
    status_vec: heapless::Vec<StatusMessage, MQ_SIZE>,
    last_queued_message: Option<RdmResponseData>,
    last_status_vec_message: DataPack,
}

impl<const MQ_SIZE: usize> RdmResponderPackageHandler<MQ_SIZE> {
    /// Creates a new [RdmResponderPackageHandler].
    pub fn new(config: RdmResponderConfig) -> Self {
        Self {
            supported_pids: config.supported_pids,
            dmx_start_address: DmxStartAddress::NoAddress,
            dmx_footprint: 1,
            rdm_receiver_metadata: config.rdm_receiver_metadata,
            uid: config.uid,
            discovery_muted: false,
            unfinished_request: None,
            message_queue: heapless::Deque::new(),
            status_vec: heapless::Vec::new(),
            last_queued_message: None,
            last_status_vec_message: DataPack::new(),
        }
    }

    /// Get the uid of the rdm responder.
    pub fn get_uid(&self) -> UniqueIdentifier {
        self.uid
    }

    /// Get the message queue that contains the results of [RdmResult::AcknowledgedTimer] packages.
    pub fn get_message_queue(&self) -> &heapless::Deque<RdmResponseData, MQ_SIZE> {
        &self.message_queue
    }

    /// Get the message queue to add the results of [RdmResult::AcknowledgedTimer] packages to.
    pub fn get_message_queue_mut(&mut self) -> &mut heapless::Deque<RdmResponseData, MQ_SIZE> {
        &mut self.message_queue
    }

    /// Get the amount of queued messages.
    pub fn get_message_count(&self) -> u8 {
        self.message_queue.len() as u8
    }

    /// Get the status queue that contains the current status messages.
    pub fn get_status_vec(&self) -> &heapless::Vec<StatusMessage, MQ_SIZE> {
        &self.status_vec
    }

    /// Get the status queue to add or remove status messages.
    pub fn get_status_vec_mut(&mut self) -> &mut heapless::Vec<StatusMessage, MQ_SIZE> {
        &mut self.status_vec
    }

    pub fn handle_rdm_request<HandlerError>(
        &mut self,
        request: RdmRequestData,
        handler: &mut dyn RdmResponderHandlerFunc<Error = HandlerError>,
    ) -> Result<RdmAnswer, HandlerError> {
        match request.destination_uid {
            PackageAddress::ManufacturerBroadcast(manufacturer_uid) => {
                if manufacturer_uid != self.uid.manufacturer_id() {
                    return Ok(RdmAnswer::NoResponse);
                }
            },
            PackageAddress::Device(device_uid) => {
                if self.uid != device_uid {
                    return Ok(RdmAnswer::NoResponse);
                }
            },
            _ => {},
        }

        if request.command_class == RequestCommandClass::DiscoveryCommand
            && ![
                pids::DISC_UNIQUE_BRANCH,
                pids::DISC_MUTE,
                pids::DISC_UN_MUTE,
            ]
            .contains(&request.parameter_id)
        {
            return Ok(RdmAnswer::NoResponse);
        }

        let response = match request.parameter_id {
            pids::DISC_UNIQUE_BRANCH => return Ok(self.handle_disc_unique_branch(&request)),
            pids::DISC_MUTE => self.handle_disc_mute(&request),
            pids::DISC_UN_MUTE => self.handle_disc_unmute(&request),
            pids::SUPPORTED_PARAMETERS => self.handle_supported_parameters(&request),
            pids::DEVICE_INFO => self.handle_device_info(&request),
            pids::SOFTWARE_VERSION_LABEL => self.handle_get_software_version_label(&request),
            pids::DMX_START_ADDRESS => self.handle_dmx_start_address(&request),
            pids::QUEUED_MESSAGE => self.handle_queued_message(&request),
            pids::STATUS_MESSAGES => self.handle_status_messages(&request),
            _ => match handler.handle_rdm(&request, &mut self.get_context())? {
                RdmResult::Acknowledged(response_data) => request.build_response(
                    ResponseType::ResponseTypeAck,
                    response_data,
                    self.get_message_count(),
                ),
                RdmResult::AcknowledgedOverflow(response_data) => request.build_response(
                    ResponseType::ResponseTypeAckOverflow,
                    response_data,
                    self.get_message_count(),
                ),
                RdmResult::NotAcknowledged(nack_reason) => request.build_response(
                    ResponseType::ResponseTypeNackReason,
                    DataPack::from_slice(&nack_reason.to_be_bytes()).unwrap(),
                    self.get_message_count(),
                ),
                RdmResult::AcknowledgedTimer(timer) => request.build_response(
                    ResponseType::ResponseTypeAckTimer,
                    DataPack::from_slice(&timer.to_be_bytes()).unwrap(),
                    self.get_message_count(),
                ),
                RdmResult::NoResponse => {
                    return Ok(RdmAnswer::NoResponse);
                },
                RdmResult::Custom(response_data) => Ok(response_data),
            }
            .ok(),
        };

        // Was this a broadcast?
        if let Some(response_data) = response {
            return Ok(RdmAnswer::Response(response_data));
        }

        // No response since the request is a broadcast
        Ok(RdmAnswer::NoResponse)
    }

    fn handle_disc_unique_branch(&self, request: &RdmRequestData) -> RdmAnswer {
        if request.command_class != RequestCommandClass::DiscoveryCommand {
            let message_count = self.get_message_count();
            return match build_nack!(request, NackReason::UnsupportedCommandClass, message_count) {
                Ok(response) => RdmAnswer::Response(response),
                Err(_) => RdmAnswer::NoResponse,
            };
        }

        if request.parameter_data.len() != 12 {
            return RdmAnswer::NoResponse;
        }

        let lower_bound: u64 =
            PackageAddress::from_bytes(&request.parameter_data[..6].try_into().unwrap()).into();
        let upper_bound: u64 =
            PackageAddress::from_bytes(&request.parameter_data[6..].try_into().unwrap()).into();
        let own_uid: u64 = self.uid.into();

        if !self.discovery_muted && own_uid >= lower_bound && own_uid <= upper_bound {
            return RdmAnswer::DiscoveryResponse(self.uid);
        }

        RdmAnswer::NoResponse
    }

    fn handle_disc_mute(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_disc_request!(request, self);

        if !request.parameter_data.is_empty() {
            return None;
        }

        self.discovery_muted = true;
        self.build_disc_mute_response(request).ok()
    }

    fn handle_disc_unmute(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_disc_request!(request, self);

        if !request.parameter_data.is_empty() {
            return None;
        }

        self.discovery_muted = false;
        self.build_disc_mute_response(request).ok()
    }

    fn handle_get_software_version_label(
        &self,
        request: &RdmRequestData,
    ) -> Option<RdmResponseData> {
        verify_get_request!(request, self);

        let software_version_label = self.rdm_receiver_metadata.software_version_label;

        request
            .build_response(
                ResponseType::ResponseTypeAck,
                DataPack::from_slice(
                    &software_version_label.as_bytes()[..software_version_label.len().min(32)],
                )
                .unwrap(),
                self.get_message_count(),
            )
            .ok()
    }

    fn handle_supported_parameters(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_get_request!(request, self);

        let current_iteration = match &self.unfinished_request {
            Some(UnfinishedRequest {
                pid: pids::SUPPORTED_PARAMETERS,
                iteration,
            }) => *iteration,
            _ => 0,
        };

        // one pid is u16
        const MAX_PIDS_PER_RESPONSE: usize = RDM_MAX_PARAMETER_DATA_LENGTH / 2;
        let current_parameter_index = MAX_PIDS_PER_RESPONSE * (current_iteration as usize);

        let amount_pids = self.supported_pids.len() + INTERNALLY_SUPPORTED_PIDS.len();
        let end_parameter_index = amount_pids.min(current_parameter_index + MAX_PIDS_PER_RESPONSE);

        let mut response_package = DataPack::new();

        for supported_pid in INTERNALLY_SUPPORTED_PIDS
            .iter()
            .chain(self.supported_pids.iter())
        {
            response_package
                .extend_from_slice(&supported_pid.to_be_bytes())
                .unwrap();
        }

        if end_parameter_index != amount_pids {
            self.unfinished_request = Some(UnfinishedRequest {
                pid: pids::SUPPORTED_PARAMETERS,
                iteration: current_iteration + 1,
            });

            request
                .build_response(
                    ResponseType::ResponseTypeAckOverflow,
                    response_package,
                    self.get_message_count(),
                )
                .ok()
        } else {
            self.unfinished_request = None;

            request
                .build_response(
                    ResponseType::ResponseTypeAck,
                    response_package,
                    self.get_message_count(),
                )
                .ok()
        }
    }

    fn handle_dmx_start_address(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        let message_count = self.get_message_count();

        match request.command_class {
            RequestCommandClass::GetCommand => request.build_response(
                ResponseType::ResponseTypeAck,
                self.dmx_start_address.serialize(),
                self.message_queue.len() as u8,
            ),
            RequestCommandClass::SetCommand => 'set_command: {
                if request.parameter_data.len() != 2 {
                    break 'set_command build_nack!(
                        request,
                        NackReason::FormatError,
                        message_count
                    );
                }

                let dmx_start_address = match DmxStartAddress::deserialize(&request.parameter_data)
                {
                    Ok(start_address) => start_address,
                    Err(_) => {
                        break 'set_command build_nack!(
                            request,
                            NackReason::DataOutOfRange,
                            message_count
                        );
                    },
                };

                self.dmx_start_address = dmx_start_address;

                request.build_response(
                    ResponseType::ResponseTypeAck,
                    DataPack::new(),
                    self.message_queue.len() as u8,
                )
            },
            RequestCommandClass::DiscoveryCommand => {
                build_nack!(request, NackReason::UnsupportedCommandClass, message_count)
            },
        }
        .ok()
    }

    fn handle_device_info(&self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_get_request!(request, self);

        request
            .build_response(
                ResponseType::ResponseTypeAck,
                DeviceInfo {
                    device_model_id: self.rdm_receiver_metadata.device_model_id,
                    product_category: self.rdm_receiver_metadata.product_category,
                    software_version: self.rdm_receiver_metadata.software_version_id,
                    dmx_footprint: self.dmx_footprint,
                    dmx_personality: 1,
                    dmx_start_address: self.dmx_start_address.clone(),
                    sub_device_count: 0,
                    sensor_count: 0,
                }
                .serialize(),
                self.get_message_count(),
            )
            .ok()
    }

    fn build_disc_mute_response(
        &self,
        request: &RdmRequestData,
    ) -> Result<RdmResponseData, IsBroadcastError> {
        request.build_response(
            ResponseType::ResponseTypeAck,
            DiscoveryMuteResponse {
                managed_proxy: false,
                sub_device: false,
                boot_loader: false,
                proxy_device: false,
                binding_uid: None,
            }
            .serialize(),
            self.get_message_count(),
        )
    }

    fn handle_queued_message(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_get_request!(request, self);

        let message_count = self.get_message_count();

        let status_type_requested = match StatusType::deserialize(&request.parameter_data) {
            Ok(status) => status,
            Err(_) => return build_nack!(request, NackReason::DataOutOfRange, message_count).ok(),
        };

        if status_type_requested == StatusType::StatusNone {
            return build_nack!(request, NackReason::DataOutOfRange, message_count).ok();
        }

        if status_type_requested == StatusType::StatusGetLastMessage {
            return match self.last_queued_message {
                None => request
                    .build_response(
                        ResponseType::ResponseTypeAck,
                        DataPack::new(),
                        message_count,
                    )
                    .ok(),
                Some(ref mut response) => {
                    response.message_count = message_count;
                    response.transaction_number = request.transaction_number;
                    Some(response.clone())
                },
            };
        }

        match status_type_requested {
            StatusType::StatusWarning | StatusType::StatusError | StatusType::StatusAdvisory => {},
            _ => return build_nack!(request, NackReason::DataOutOfRange, message_count).ok(),
        }

        let response = match self.message_queue.pop_back() {
            None => {
                let response_data = self.pop_filtered_statuses(status_type_requested);

                let status_message_response = RdmResponseData {
                    destination_uid: PackageAddress::Device(request.source_uid),
                    source_uid: self.uid,
                    transaction_number: request.transaction_number,
                    response_type: ResponseType::ResponseTypeAck,
                    message_count: 0,
                    sub_device: 0,
                    command_class: request.command_class.get_response_class(),
                    parameter_id: pids::STATUS_MESSAGES,
                    parameter_data: response_data,
                };
                self.last_status_vec_message = status_message_response.parameter_data.clone();

                status_message_response
            },
            Some(mut response_data) => {
                response_data.message_count = self.get_message_count();
                response_data.transaction_number = request.transaction_number;
                response_data
            },
        };

        self.last_queued_message = Some(response.clone());
        Some(response)
    }

    fn handle_status_messages(&mut self, request: &RdmRequestData) -> Option<RdmResponseData> {
        verify_get_request!(request, self);

        let message_count = self.get_message_count();

        let status_type_requested = match StatusType::deserialize(&request.parameter_data) {
            Ok(status) => status,
            Err(_) => return build_nack!(request, NackReason::FormatError, message_count).ok(),
        };

        match status_type_requested {
            StatusType::StatusNone => request.build_response(
                ResponseType::ResponseTypeAck,
                DataPack::new(),
                message_count,
            ),
            StatusType::StatusGetLastMessage => request.build_response(
                ResponseType::ResponseTypeAck,
                self.last_status_vec_message.clone(),
                message_count,
            ),
            StatusType::StatusWarning | StatusType::StatusError | StatusType::StatusAdvisory => {
                let response_vec = self.pop_filtered_statuses(status_type_requested);

                self.last_status_vec_message = response_vec.clone();
                request.build_response(ResponseType::ResponseTypeAck, response_vec, message_count)
            },
            _ => build_nack!(request, NackReason::DataOutOfRange, message_count),
        }
        .ok()
    }

    fn pop_filtered_statuses(&mut self, status_filter: StatusType) -> DataPack {
        let mut indexes_to_remove =
            heapless::Vec::<usize, RDM_MAX_STATUS_PACKAGES_PER_REQUEST>::new();
        let mut parameter_data = DataPack::new();

        self.status_vec
            .iter()
            .take(RDM_MAX_STATUS_PACKAGES_PER_REQUEST)
            .filter(|item| ((item.status_type as u8) & 0x0F) >= status_filter as u8)
            .map(|item| item.serialize())
            .enumerate()
            .for_each(|(index, data_pack)| {
                parameter_data.extend_from_slice(&data_pack).unwrap();
                indexes_to_remove.push(index).unwrap();
            });

        for index_to_remove in indexes_to_remove {
            self.status_vec.remove(index_to_remove);
        }

        parameter_data
    }

    pub fn get_context(&mut self) -> DmxReceiverContext {
        let message_count = self.get_message_count();

        DmxReceiverContext {
            dmx_start_address: &mut self.dmx_start_address,
            dmx_footprint: &mut self.dmx_footprint,
            discovery_muted: &mut self.discovery_muted,
            message_count,
        }
    }
}
