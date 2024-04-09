use crate::command_class::{RequestCommandClass, ResponseCommandClass};
use crate::consts::{
    RDM_DISCOVERY_RESPONSE_SIZE, RDM_MAX_PACKAGE_SIZE, RDM_MAX_PARAMETER_DATA_LENGTH,
    RDM_MIN_PACKAGE_SIZE, SC_RDM, SC_SUB_MESSAGE, SEPARATOR_BYTE,
};
use crate::layouts::rdm_request_layout;
use crate::types::{DataPack, ResponseType};
use crate::unique_identifier::{PackageAddress, UniqueIdentifier};
use crate::utils::calculate_checksum;

/// Binary representation of an RDM package.
pub type BinaryRdmPackage = heapless::Vec<u8, RDM_MAX_PACKAGE_SIZE>;

/// Error that gets raised when attempting to convert an [RdmRequestData] object
/// to a [RdmResponseData] object that contains a broadcast destination address.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct IsBroadcastError;

impl core::fmt::Display for IsBroadcastError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "tried to convert broadcast request to response")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for IsBroadcastError {}

/// An RDM Request package that does not have its parameter data deserialized.
#[derive(Debug)]
pub struct RdmRequestData {
    pub destination_uid: PackageAddress,
    pub source_uid: UniqueIdentifier,
    pub transaction_number: u8,
    pub port_id: u8,
    pub message_count: u8,
    pub sub_device: u16,
    pub command_class: RequestCommandClass,
    pub parameter_id: u16,
    pub parameter_data: DataPack,
}

impl RdmRequestData {
    pub fn build_response(
        &self,
        response_type: ResponseType,
        response: DataPack,
        message_count: u8,
    ) -> Result<RdmResponseData, IsBroadcastError> {
        Ok(RdmResponseData {
            destination_uid: PackageAddress::Device(self.source_uid),
            source_uid: match self.destination_uid {
                PackageAddress::Device(uid) => uid,
                _ => return Err(IsBroadcastError),
            },
            transaction_number: self.transaction_number,
            response_type,
            message_count,
            sub_device: self.sub_device,
            command_class: self.command_class.get_response_class(),
            parameter_id: self.parameter_id,
            parameter_data: response,
        })
    }
}

/// An RDM Response package that does not have its parameter data deserialized.
#[derive(Debug, Clone)]
pub struct RdmResponseData {
    pub destination_uid: PackageAddress,
    pub source_uid: UniqueIdentifier,
    pub transaction_number: u8,
    pub response_type: ResponseType,
    pub message_count: u8,
    pub sub_device: u16,
    pub command_class: ResponseCommandClass,
    pub parameter_id: u16,
    pub parameter_data: DataPack,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RdmDeserializationError {
    /// Buffer must be at least 22 bytes
    BufferTooSmall,
    /// Buffer must be at most 257 bytes
    BufferTooBig,
    /// The command class was not found; contains contents of command class field
    CommandClassNotFound(u8),
    /// The response type was not found; contains contents of response type field
    ResponseTypeNotFound(u8),
    /// The message length field is incorrect; contains result of parsing
    WrongMessageLength(usize),
    /// Wrong checksum; contains result of parsing
    WrongChecksum,
    /// Received wrong start code (0xCC) or sub start code (0x01); contains result of parsing
    WrongStartCode,
    /// The source uid is a broadcast address.
    SourceUidIsBroadcast,
}

impl core::fmt::Display for RdmDeserializationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RdmDeserializationError::BufferTooSmall => write!(f, "buffer too small"),
            RdmDeserializationError::BufferTooBig => write!(f, "buffer to big"),
            RdmDeserializationError::CommandClassNotFound(command_class) => {
                write!(f, "command class {} not found", command_class)
            },
            RdmDeserializationError::ResponseTypeNotFound(response_type) => {
                write!(f, "response type {} is unknown", response_type)
            },
            RdmDeserializationError::WrongMessageLength(message_length) => {
                write!(f, "message length {} is incorrect", message_length)
            },
            RdmDeserializationError::WrongChecksum => write!(f, "checksum is incorrect"),
            RdmDeserializationError::WrongStartCode => write!(f, "start code is incorrect"),
            RdmDeserializationError::SourceUidIsBroadcast => write!(f, "source uid is a broadcast"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RdmDeserializationError {}

#[derive(Debug)]
pub enum RdmData {
    Request(RdmRequestData),
    Response(RdmResponseData),
}

impl RdmData {
    pub fn deserialize(buf: &[u8]) -> Result<Self, RdmDeserializationError> {
        deserialize_rdm_data(buf)
    }

    pub fn serialize(&self) -> BinaryRdmPackage {
        serialize_rdm_data(self)
    }
}

/// Deserialize rdm data.
/// Buffer must be between 22 and 257 bytes.
pub fn deserialize_rdm_data(buffer: &[u8]) -> Result<RdmData, RdmDeserializationError> {
    let buffer_size = buffer.len();

    if buffer_size < RDM_MIN_PACKAGE_SIZE {
        return Err(RdmDeserializationError::BufferTooSmall);
    }

    if buffer_size > RDM_MAX_PACKAGE_SIZE {
        return Err(RdmDeserializationError::BufferTooBig);
    }

    // Exclude checksum field
    // Will evaluate correctness later
    let expected_checksum = calculate_checksum(&buffer[..buffer_size - 2]);
    let actual_checksum =
        u16::from_be_bytes(buffer[buffer_size - 2..buffer_size].try_into().unwrap());

    if expected_checksum != actual_checksum {
        return Err(RdmDeserializationError::WrongChecksum);
    }

    let request_data_view = rdm_request_layout::View::new(buffer);

    if request_data_view.start_code().read() != SC_RDM
        || request_data_view.sub_start_code().read() != SC_SUB_MESSAGE
    {
        return Err(RdmDeserializationError::WrongStartCode);
    }

    // exclude checksum
    let message_length = request_data_view.message_length().read() as usize;
    if message_length != buffer_size - 2 {
        return Err(RdmDeserializationError::WrongMessageLength(message_length));
    }

    let parameter_data_and_checksum = request_data_view.parameter_data_and_checksum();
    // Redundant check 😉
    let parameter_data =
        DataPack::from_slice(&parameter_data_and_checksum[..parameter_data_and_checksum.len() - 2])
            .map_err(|_| RdmDeserializationError::BufferTooBig)?;

    let command_class_field = request_data_view.command_class().read();
    let is_request = RequestCommandClass::try_from(command_class_field).is_ok();

    let rdm_data = if is_request {
        RdmData::Request(RdmRequestData {
            destination_uid: PackageAddress::from_bytes(request_data_view.destination_uid()),
            source_uid: match PackageAddress::from_bytes(request_data_view.source_uid()) {
                PackageAddress::Device(device_uid) => device_uid,
                _ => return Err(RdmDeserializationError::SourceUidIsBroadcast),
            },
            transaction_number: request_data_view.transaction_number().read(),
            port_id: request_data_view.port_id_response_type().read(),
            message_count: request_data_view.message_count().read(),
            sub_device: request_data_view.sub_device().read(),
            command_class: command_class_field
                .try_into()
                .map_err(|_| RdmDeserializationError::CommandClassNotFound(command_class_field))?,
            parameter_id: request_data_view.parameter_id().read(),
            parameter_data,
        })
    } else {
        let response_type_field = request_data_view.port_id_response_type().read();
        let response_type = response_type_field
            .try_into()
            .map_err(|_| RdmDeserializationError::ResponseTypeNotFound(response_type_field))?;

        RdmData::Response(RdmResponseData {
            destination_uid: PackageAddress::from_bytes(request_data_view.destination_uid()),
            source_uid: match PackageAddress::from_bytes(request_data_view.source_uid()) {
                PackageAddress::Device(uid) => uid,
                _ => return Err(RdmDeserializationError::SourceUidIsBroadcast),
            },
            transaction_number: request_data_view.transaction_number().read(),
            response_type,
            message_count: request_data_view.message_count().read(),
            sub_device: request_data_view.sub_device().read(),
            command_class: command_class_field
                .try_into()
                .map_err(|_| RdmDeserializationError::CommandClassNotFound(command_class_field))?,
            parameter_id: request_data_view.parameter_id().read(),
            parameter_data,
        })
    };

    Ok(rdm_data)
}

/// Serializes RDM data to a binary Vec.
pub fn serialize_rdm_data(rdm_data: &RdmData) -> BinaryRdmPackage {
    let mut dst = [0u8; RDM_MAX_PACKAGE_SIZE];

    let parameter_data_length = match rdm_data {
        RdmData::Request(ref request) => request.parameter_data.len(),
        RdmData::Response(ref response) => response.parameter_data.len(),
    };
    assert!(parameter_data_length <= RDM_MAX_PARAMETER_DATA_LENGTH);

    // parameter data length + all other fields including checksum
    let total_package_length = parameter_data_length + 26;
    let mut memory_view = rdm_request_layout::View::new(&mut dst[..total_package_length]);

    memory_view.start_code_mut().write(SC_RDM);
    memory_view.sub_start_code_mut().write(SC_SUB_MESSAGE);

    // 24 is the size of all the fields besides parameter_data except the checksum
    memory_view
        .message_length_mut()
        .write(parameter_data_length as u8 + 24);

    match rdm_data {
        RdmData::Request(request) => {
            memory_view
                .destination_uid_mut()
                .copy_from_slice(&request.destination_uid.to_bytes());
            memory_view
                .source_uid_mut()
                .copy_from_slice(&request.source_uid.to_bytes());

            memory_view
                .transaction_number_mut()
                .write(request.transaction_number);
            memory_view
                .port_id_response_type_mut()
                .write(request.port_id);
            memory_view.sub_device_mut().write(request.sub_device);
            memory_view
                .command_class_mut()
                .write(request.command_class as u8);
            memory_view.parameter_id_mut().write(request.parameter_id);
            memory_view
                .parameter_data_length_mut()
                .write(parameter_data_length as u8);

            memory_view.parameter_data_and_checksum_mut()[..parameter_data_length]
                .copy_from_slice(&request.parameter_data);
            let checksum = calculate_checksum(&dst[..total_package_length - 2]);
            dst[total_package_length - 2..total_package_length]
                .copy_from_slice(&checksum.to_be_bytes());
        },
        RdmData::Response(response) => {
            memory_view
                .destination_uid_mut()
                .copy_from_slice(&response.destination_uid.to_bytes());
            memory_view
                .source_uid_mut()
                .copy_from_slice(&response.source_uid.to_bytes());

            memory_view
                .transaction_number_mut()
                .write(response.transaction_number);
            memory_view
                .port_id_response_type_mut()
                .write(response.response_type as u8);
            memory_view.sub_device_mut().write(response.sub_device);
            memory_view
                .command_class_mut()
                .write(response.command_class as u8);
            memory_view.parameter_id_mut().write(response.parameter_id);
            memory_view
                .parameter_data_length_mut()
                .write(parameter_data_length as u8);

            memory_view.parameter_data_and_checksum_mut()[..parameter_data_length]
                .copy_from_slice(&response.parameter_data);
            let checksum = calculate_checksum(&dst[..total_package_length - 2]);
            dst[total_package_length - 2..total_package_length]
                .copy_from_slice(&checksum.to_be_bytes());
        },
    }

    // In the industry we call this a pro gamer move.
    heapless::Vec::from_slice(&dst[..total_package_length]).unwrap()
}

/// Returns received device id if there is no collision.
pub fn deserialize_discovery_response(
    buffer: &[u8],
) -> Result<UniqueIdentifier, RdmDeserializationError> {
    let index_of_separator_byte = match buffer.iter().position(|&x| x == SEPARATOR_BYTE) {
        None => {
            return Err(RdmDeserializationError::WrongStartCode); // idk
        },
        Some(index) => index,
    };

    let start_index = index_of_separator_byte + 1;
    let message_length = buffer.len() - start_index;
    if message_length < RDM_DISCOVERY_RESPONSE_SIZE {
        return Err(RdmDeserializationError::WrongMessageLength(message_length));
    }

    let calculated_checksum = calculate_checksum(&buffer[start_index..start_index + 12]);

    let mut device_id_buf = [0u8; 6];
    decode_disc_unique(&buffer[start_index..start_index + 12], &mut device_id_buf);
    let uid = match PackageAddress::from_bytes(&device_id_buf) {
        PackageAddress::Device(uid) => uid,
        _ => return Err(RdmDeserializationError::SourceUidIsBroadcast),
    };

    let mut checksum_buf = [0u8; 2];
    decode_disc_unique(
        &buffer[start_index + 12..start_index + 16],
        &mut checksum_buf,
    );
    let received_checksum = u16::from_be_bytes(checksum_buf);

    if calculated_checksum != received_checksum {
        return Err(RdmDeserializationError::WrongChecksum);
    }

    Ok(uid)
}

/// Decode a discovery package. The destination has to be at least half the source size.
fn decode_disc_unique(src: &[u8], dest: &mut [u8]) {
    assert!(
        dest.len() * 2 >= src.len(),
        "Destination buffer has to be at least half the size of the source buffer."
    );

    for (index, byte) in src.chunks(2).map(|chunk| chunk[0] & chunk[1]).enumerate() {
        dest[index] = byte;
    }
}
