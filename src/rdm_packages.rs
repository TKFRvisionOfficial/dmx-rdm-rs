use crate::consts::RDM_STATUS_MESSAGE_SIZE;
use crate::pids;
use crate::rdm_types::{
    DeserializationError, DmxStartAddress, StatusMessage, StatusMessages, SupportedParameters,
};
use crate::types::DataPack;

pub fn deserialize_identify(buffer: &[u8]) -> Result<bool, DeserializationError> {
    if buffer.len() != 1 {
        return Err(DeserializationError);
    }

    Ok(buffer[0] != 0)
}

pub fn deserialize_software_version_label(
    buffer: &[u8],
) -> Result<heapless::String<32>, DeserializationError> {
    heapless::String::from_utf8(
        heapless::Vec::<_, 32>::from_slice(buffer).or(Err(DeserializationError))?,
    )
    .or(Err(DeserializationError))
}

pub fn deserialize_status_messages(buffer: &[u8]) -> Result<StatusMessages, DeserializationError> {
    if buffer.len() % RDM_STATUS_MESSAGE_SIZE != 0 {
        return Err(DeserializationError);
    }

    let mut status_messages = heapless::Vec::new();
    for package_bytes in buffer.chunks(RDM_STATUS_MESSAGE_SIZE) {
        status_messages
            .push(StatusMessage::deserialize(package_bytes)?)
            .map_err(|_| DeserializationError)?;
    }

    Ok(status_messages)
}

pub fn deserialize_supported_parameters(
    buffer: &[u8],
) -> Result<SupportedParameters, DeserializationError> {
    if buffer.len() % 2 != 0 {
        return Err(DeserializationError);
    }

    let mut supported_parameters = heapless::Vec::new();
    for package_bytes in buffer.chunks(2) {
        supported_parameters
            .push(u16::from_be_bytes(package_bytes.try_into().unwrap()))
            .map_err(|_| DeserializationError)?;
    }

    Ok(supported_parameters)
}

#[derive(Debug)]
pub struct RdmResponseInfo {
    pub parameter_id: u16,
    pub message_count: u8,
    pub data: DataPack,
}

#[derive(Debug)]
pub enum RdmResponsePackage {
    IdentifyDevice(bool),
    SoftwareVersionLabel(heapless::String<32>),
    DmxStartAddress(DmxStartAddress),
    StatusMessages(StatusMessages),
    SupportedParameters(SupportedParameters),
    Custom(RdmResponseInfo),
}

impl RdmResponsePackage {
    pub fn from_response_info(
        response_info: RdmResponseInfo,
    ) -> Result<Self, DeserializationError> {
        Ok(match response_info.parameter_id {
            pids::IDENTIFY_DEVICE => {
                RdmResponsePackage::IdentifyDevice(deserialize_identify(&response_info.data)?)
            },
            pids::SOFTWARE_VERSION_LABEL => RdmResponsePackage::SoftwareVersionLabel(
                deserialize_software_version_label(&response_info.data)?,
            ),
            pids::DMX_START_ADDRESS => RdmResponsePackage::DmxStartAddress(
                DmxStartAddress::deserialize(&response_info.data)?,
            ),
            pids::STATUS_MESSAGES => RdmResponsePackage::StatusMessages(
                deserialize_status_messages(&response_info.data)?,
            ),
            pids::SUPPORTED_PARAMETERS => RdmResponsePackage::SupportedParameters(
                deserialize_supported_parameters(&response_info.data)?,
            ),
            _ => Self::Custom(response_info),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::rdm_packages::deserialize_identify;

    #[test]
    fn test_deserialize_identify_success() {
        assert_eq!(deserialize_identify(&[0]).unwrap(), false);
        assert_eq!(deserialize_identify(&[1]).unwrap(), true);

        // should this work 🤣
        assert_eq!(deserialize_identify(&[3]).unwrap(), true);
    }

    #[test]
    fn test_deserialize_identify_failure() {
        deserialize_identify(&[2, 1]).unwrap_err();
        deserialize_identify(&[0, 0]).unwrap_err();
    }
}
