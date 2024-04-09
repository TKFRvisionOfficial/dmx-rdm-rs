use crate::consts::{
    RDM_DEVICE_INFO_SIZE, RDM_MAX_STATUS_PACKAGES_PER_REQUEST,
    RDM_MAX_SUPPORTED_PARAMETERS_PER_REQUEST, RDM_STATUS_MESSAGE_SIZE,
};
use crate::layouts::{rdm_device_info_layout, rdm_status_message_layout};
use crate::types::DataPack;
use crate::unique_identifier::{PackageAddress, UniqueIdentifier};
use modular_bitfield::bitfield;
use modular_bitfield::prelude::B12;

#[derive(Debug)]
pub struct DeserializationError;

impl core::fmt::Display for DeserializationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "There was a deserialization error.")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DeserializationError {}

#[derive(Debug, Clone)]
pub enum DmxStartAddress {
    /// The requested device has a dmx footprint of 0.
    NoAddress,
    /// The requested device does allocate dmx addresses.
    Address(u16),
}

impl DmxStartAddress {
    pub fn as_u16(&self) -> u16 {
        match self {
            DmxStartAddress::Address(address) => *address,
            DmxStartAddress::NoAddress => 0xFFFF,
        }
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, DeserializationError> {
        let start_address = u16::from_be_bytes(data.try_into().map_err(|_| DeserializationError)?);

        start_address.try_into().map_err(|_| DeserializationError)
    }

    pub fn serialize(&self) -> DataPack {
        DataPack::from_slice(&self.as_u16().to_be_bytes()).unwrap()
    }
}

impl TryFrom<u16> for DmxStartAddress {
    type Error = DeserializationError;

    fn try_from(start_address: u16) -> Result<Self, Self::Error> {
        if start_address == 0xFFFF {
            return Ok(Self::NoAddress);
        }

        if !(1..=512).contains(&start_address) {
            return Err(DeserializationError);
        }

        Ok(Self::Address(start_address))
    }
}

/// Response to discovery mute/unmute requests.
pub struct DiscoveryMuteResponse {
    /// The responder is a proxy device.
    pub managed_proxy: bool,
    /// The responder supports sub devices.
    pub sub_device: bool,
    /// The responder is not operational before receiving a firmware update.
    pub boot_loader: bool,
    /// A proxy device has responded on behalf of another device.
    pub proxy_device: bool,
    /// Included if the responding device contains multiple responder ports.
    /// It is the UUID to the primary port of the device.
    pub binding_uid: Option<UniqueIdentifier>,
}

#[bitfield]
struct DiscControlField {
    /// The responder is a proxy device.
    pub managed_proxy: bool,
    /// The responder supports sub devices.
    pub sub_device: bool,
    /// The responder is not operational before receiving a firmware update.
    pub boot_loader: bool,
    /// A proxy device has responded on behalf of another device.
    pub proxy_device: bool,
    #[skip]
    reserved: B12,
}

impl DiscoveryMuteResponse {
    pub fn deserialize(data: &[u8]) -> Result<Self, DeserializationError> {
        if data.len() < 2 {
            return Err(DeserializationError);
        }

        let control_field = DiscControlField::from_bytes((&data[0..2]).try_into().unwrap());
        let mut discovery_mute_response = Self {
            managed_proxy: control_field.managed_proxy(),
            sub_device: control_field.sub_device(),
            boot_loader: control_field.boot_loader(),
            proxy_device: control_field.proxy_device(),
            binding_uid: None,
        };

        if data.len() == 8 {
            let binding_uuid = match PackageAddress::from_bytes((&data[2..8]).try_into().unwrap()) {
                PackageAddress::Device(uid) => uid,
                _ => return Err(DeserializationError),
            };

            discovery_mute_response.binding_uid = Some(binding_uuid);
        }

        Ok(discovery_mute_response)
    }

    pub fn serialize(&self) -> DataPack {
        let mut data_pack = DataPack::new();
        let disc_control_field = DiscControlField::new()
            .with_managed_proxy(self.managed_proxy)
            .with_sub_device(self.sub_device)
            .with_boot_loader(self.boot_loader)
            .with_proxy_device(self.proxy_device);

        data_pack
            .extend_from_slice(&disc_control_field.into_bytes())
            .unwrap();

        match self.binding_uid {
            None => {},
            Some(uid) => data_pack.extend_from_slice(&uid.to_bytes()).unwrap(),
        }

        data_pack
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
pub enum StatusType {
    StatusNone = 0x00,
    StatusGetLastMessage = 0x01,
    StatusAdvisory = 0x02,
    StatusWarning = 0x03,
    StatusError = 0x04,
    StatusAdvisoryCleared = 0x12,
    StatusWarningCleared = 0x13,
    StatusErrorCleared = 0x14,
}

impl StatusType {
    pub fn deserialize(data: &[u8]) -> Result<Self, DeserializationError> {
        if data.len() != 1 {
            return Err(DeserializationError);
        }

        let status_type_requested: StatusType = match data[0].try_into() {
            Ok(status_type) => status_type,
            Err(_) => {
                return Err(DeserializationError);
            },
        };

        Ok(status_type_requested)
    }
}

impl TryFrom<u8> for StatusType {
    type Error = DeserializationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0x00 => Self::StatusNone,
            0x01 => Self::StatusGetLastMessage,
            0x02 => Self::StatusAdvisory,
            0x03 => Self::StatusWarning,
            0x04 => Self::StatusError,
            0x12 => Self::StatusAdvisoryCleared,
            0x13 => Self::StatusWarningCleared,
            0x14 => Self::StatusWarningCleared,
            _ => return Err(DeserializationError),
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct StatusMessage {
    pub sub_device_id: u16,
    pub status_type: StatusType,
    // we should add proper deserialization
    pub status_message_id: u16,
    pub data_value_1: u16,
    pub data_value_2: u16,
}

impl StatusMessage {
    pub fn deserialize(buffer: &[u8]) -> Result<Self, DeserializationError> {
        let status_message_view = rdm_status_message_layout::View::new(buffer);

        Ok(Self {
            sub_device_id: status_message_view.sub_device_id().read(),
            status_type: status_message_view.status_type().read().try_into()?,
            status_message_id: status_message_view.status_message_id().read(),
            data_value_1: status_message_view.data_value_1().read(),
            data_value_2: status_message_view.data_value_2().read(),
        })
    }

    pub fn serialize(&self) -> [u8; RDM_STATUS_MESSAGE_SIZE] {
        let mut resp_buffer = [0u8; RDM_STATUS_MESSAGE_SIZE];
        let mut status_message_view = rdm_status_message_layout::View::new(&mut resp_buffer);

        status_message_view
            .sub_device_id_mut()
            .write(self.sub_device_id);
        status_message_view
            .status_type_mut()
            .write(self.status_type as u8);
        status_message_view
            .status_message_id_mut()
            .write(self.status_message_id);
        status_message_view
            .data_value_1_mut()
            .write(self.data_value_1);
        status_message_view
            .data_value_2_mut()
            .write(self.data_value_2);

        resp_buffer
    }
}

pub type StatusMessages = heapless::Vec<StatusMessage, RDM_MAX_STATUS_PACKAGES_PER_REQUEST>;
pub type SupportedParameters = heapless::Vec<u16, RDM_MAX_SUPPORTED_PARAMETERS_PER_REQUEST>;

pub struct DeviceInfo {
    pub device_model_id: u16,
    pub product_category: u16,
    pub software_version: u32,
    pub dmx_footprint: u16,
    pub dmx_personality: u16,
    pub dmx_start_address: DmxStartAddress,
    pub sub_device_count: u16,
    pub sensor_count: u8,
}

impl DeviceInfo {
    pub fn deserialize(buffer: &[u8]) -> Result<Self, DeserializationError> {
        if buffer.len() != rdm_device_info_layout::SIZE.unwrap() {
            return Err(DeserializationError);
        }

        let device_info_view = rdm_device_info_layout::View::new(buffer);
        Ok(DeviceInfo {
            device_model_id: device_info_view.device_model_id().read(),
            product_category: device_info_view.product_category().read(),
            software_version: device_info_view.software_version_id().read(),
            dmx_footprint: device_info_view.dmx_footprint().read(),
            dmx_personality: device_info_view.dmx_personality().read(),
            dmx_start_address: device_info_view.dmx_start_address().read().try_into()?,
            sub_device_count: device_info_view.sub_device_count().read(),
            sensor_count: device_info_view.sensor_count().read(),
        })
    }

    pub fn serialize(&self) -> DataPack {
        let mut resp_buffer = [0u8; RDM_DEVICE_INFO_SIZE];
        let mut device_info_view = rdm_device_info_layout::View::new(&mut resp_buffer);

        device_info_view.protocol_version_mut().write(0x01_00);
        device_info_view
            .device_model_id_mut()
            .write(self.device_model_id);
        device_info_view
            .product_category_mut()
            .write(self.product_category);
        device_info_view
            .software_version_id_mut()
            .write(self.software_version);
        device_info_view
            .dmx_footprint_mut()
            .write(self.dmx_footprint);
        device_info_view
            .dmx_personality_mut()
            .write(self.dmx_personality);
        device_info_view
            .dmx_start_address_mut()
            .write(self.dmx_start_address.as_u16());
        device_info_view
            .sub_device_count_mut()
            .write(self.sub_device_count);
        device_info_view.sensor_count_mut().write(self.sensor_count);

        DataPack::from_slice(&resp_buffer).unwrap()
    }
}

/// Returned by parameter packages where the response might not fit into one package.
pub enum OverflowMessageResp<T> {
    /// Has received the complete message.
    Complete(T),
    /// Has not received the complete message.
    /// Request the same pid to get the next part until you receive [OverflowMessageResp::Complete].
    Incomplete(T),
}
