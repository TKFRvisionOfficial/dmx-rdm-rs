pub const DMX_NULL_START: u8 = 0x00;
/// start code + 512 byte package
pub const DMX_MAX_PACKAGE_SIZE: usize = 513;
pub const SC_RDM: u8 = 0xCC;
pub const SC_SUB_MESSAGE: u8 = 0x01;

pub const PREAMBLE_BYTE: u8 = 0xFE;
pub const SEPARATOR_BYTE: u8 = 0xAA;

pub const BROADCAST_UID: u64 = 0xFFFF_FFFFFFFF;

pub const DMX_BAUD: u32 = 250_000;

pub const BREAK_MICROS: u64 = 200;
pub const MAB_MICROS: u64 = 48;
pub const MAXIMUM_DMX512_MILLIS: usize = 1250;
pub const INTER_SLOT_TIME_MILLIS: usize = 2;

pub const RDM_MIN_PACKAGE_SIZE: usize = 22;
pub const RDM_MAX_PACKAGE_SIZE: usize = 257;
/// Excluding preamble and separator
pub const RDM_DISCOVERY_RESPONSE_SIZE: usize = 16;
/// Including 7 bytes preamble + 1 byte separator
pub const RDM_MAX_DISCOVERY_RESPONSE_SIZE: usize = RDM_DISCOVERY_RESPONSE_SIZE + 8;

pub const RDM_MAX_PARAMETER_DATA_LENGTH: usize = 231;
pub const RDM_MAX_STATUS_PACKAGES_PER_REQUEST: usize = 25;
pub const RDM_STATUS_MESSAGE_SIZE: usize = 9;
pub const RDM_DEVICE_INFO_SIZE: usize = 0x13;

pub const RDM_MAX_SUPPORTED_PARAMETERS_PER_REQUEST: usize = 128;
