use crate::consts::RDM_MAX_PARAMETER_DATA_LENGTH;

pub type DataPack = heapless::Vec<u8, RDM_MAX_PARAMETER_DATA_LENGTH>;

/// Response status of a rdm package
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum ResponseType {
    /// The request was acknowledged.
    ResponseTypeAck = 0x00,
    /// The request was acknowledged but the result isn't ready yet.
    ResponseTypeAckTimer = 0x01,
    /// The request was not acknowledged.
    ResponseTypeNackReason = 0x02,
    /// The request was acknowledged but the response does not fit into a single responds.
    ResponseTypeAckOverflow = 0x03,
}

impl TryFrom<u8> for ResponseType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        Ok(match value {
            0x00 => Self::ResponseTypeAck,
            0x01 => Self::ResponseTypeAckTimer,
            0x02 => Self::ResponseTypeNackReason,
            0x03 => Self::ResponseTypeAckOverflow,
            _ => {
                return Err(());
            },
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u16)]
pub enum NackReason {
    UnknownPid = 0x0000,
    FormatError = 0x0001,
    HardwareFault = 0x0002,
    ProxyReject = 0x0003,
    WriteProtect = 0x0004,
    UnsupportedCommandClass = 0x0005,
    DataOutOfRange = 0x0006,
    BufferFull = 0x0007,
    PacketSizeUnsupported = 0x0008,
    SubDeviceOutOfRange = 0x0009,
    ProxyBufferFull = 0x000A,
}

impl NackReason {
    pub fn serialize(&self) -> DataPack {
        DataPack::from_slice(&(*self as u16).to_be_bytes()).unwrap()
    }
}

impl TryFrom<u16> for NackReason {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, ()> {
        match value {
            0x0000 => Ok(Self::UnknownPid),
            0x0001 => Ok(Self::FormatError),
            0x0002 => Ok(Self::HardwareFault),
            0x0003 => Ok(Self::ProxyReject),
            0x0004 => Ok(Self::WriteProtect),
            0x0005 => Ok(Self::UnsupportedCommandClass),
            0x0006 => Ok(Self::DataOutOfRange),
            0x0007 => Ok(Self::BufferFull),
            0x0008 => Ok(Self::PacketSizeUnsupported),
            0x0009 => Ok(Self::SubDeviceOutOfRange),
            0x000A => Ok(Self::ProxyBufferFull),
            _ => Err(()),
        }
    }
}
