use crate::consts::BROADCAST_UID;
use crate::rdm_types::DeserializationError;

/// The unique id that is used as a source id in the packages.
/// There shouldn't be multiple devices with same unique id.
/// The manufacturer uids are assigned by the esta.
/// [more information](https://tsp.esta.org/tsp/working_groups/CP/mfctrIDs.php)
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct UniqueIdentifier {
    manufacturer_uid: u16,
    device_uid: u32,
}

impl core::fmt::Display for UniqueIdentifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:04X}:{:08X}", self.manufacturer_uid, self.device_uid)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for UniqueIdentifier {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{:04X}:{:08X}", self.manufacturer_uid, self.device_uid);
    }
}

impl UniqueIdentifier {
    pub fn new(manufacturer_uid: u16, device_uid: u32) -> Result<Self, DeserializationError> {
        if device_uid == u32::MAX || manufacturer_uid == u16::MAX {
            return Err(DeserializationError);
        }

        Ok(UniqueIdentifier {
            manufacturer_uid,
            device_uid,
        })
    }

    pub fn manufacturer_uid(&self) -> u16 {
        self.manufacturer_uid
    }

    pub fn device_uid(&self) -> u32 {
        self.device_uid
    }

    pub fn set_manufacturer_uid(
        &mut self,
        manufacturer_uid: u16,
    ) -> Result<(), DeserializationError> {
        if manufacturer_uid == u16::MAX {
            return Err(DeserializationError);
        }

        self.manufacturer_uid = manufacturer_uid;
        Ok(())
    }

    pub fn set_device_uid(&mut self, device_uid: u32) -> Result<(), DeserializationError> {
        if device_uid == u32::MAX {
            return Err(DeserializationError);
        }

        self.device_uid = device_uid;
        Ok(())
    }

    pub fn to_bytes(&self) -> [u8; 6] {
        let mut buffer = [0u8; 6];

        buffer[..2].copy_from_slice(&self.manufacturer_uid.to_be_bytes());
        buffer[2..].copy_from_slice(&self.device_uid.to_be_bytes());

        buffer
    }
}

impl TryFrom<u64> for UniqueIdentifier {
    type Error = DeserializationError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let manufacturer_uid = (value >> u32::BITS) as u16;
        let device_uid = (value & u32::MAX as u64) as u32;

        if device_uid == u32::MAX {
            return Err(DeserializationError);
        }

        Ok(Self {
            manufacturer_uid,
            device_uid,
        })
    }
}

impl From<UniqueIdentifier> for u64 {
    fn from(value: UniqueIdentifier) -> Self {
        ((value.manufacturer_uid as u64) << u32::BITS) | value.device_uid as u64
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PackageAddress {
    /// Broadcast to all devices.
    Broadcast,
    /// Broadcast to all devices from a specific manufacturer identified by the manufacturer id
    /// in the u16.
    ManufacturerBroadcast(u16),
    /// Send package to a specific device.
    Device(UniqueIdentifier),
}

impl PackageAddress {
    pub fn from_bytes(buffer: &[u8; 6]) -> Self {
        let manufacturer_uid = u16::from_be_bytes(buffer[0..2].try_into().unwrap());
        let device_uid = u32::from_be_bytes(buffer[2..].try_into().unwrap());

        if device_uid == u32::MAX {
            if manufacturer_uid == u16::MAX {
                Self::Broadcast
            } else {
                Self::ManufacturerBroadcast(manufacturer_uid)
            }
        } else {
            Self::Device(UniqueIdentifier {
                manufacturer_uid,
                device_uid,
            })
        }
    }

    pub fn to_bytes(&self) -> [u8; 6] {
        match self {
            Self::Broadcast => [0xFFu8; 6],
            Self::ManufacturerBroadcast(manufacturer_uid) => {
                let mut buffer = [0xFFu8; 6];
                buffer[..2].copy_from_slice(&manufacturer_uid.to_be_bytes());

                buffer
            },
            Self::Device(uid) => uid.to_bytes(),
        }
    }

    pub fn is_broadcast(&self) -> bool {
        match self {
            PackageAddress::Broadcast => true,
            PackageAddress::ManufacturerBroadcast(_) => true,
            PackageAddress::Device(_) => false,
        }
    }
}

impl TryFrom<u64> for PackageAddress {
    type Error = DeserializationError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value >> 6 > 0 {
            return Err(DeserializationError);
        }

        let manufacturer_uid = (value >> u32::BITS) as u16;
        let device_uid = (value & u32::MAX as u64) as u32;

        if device_uid == u32::MAX {
            if manufacturer_uid == u16::MAX {
                return Ok(Self::Broadcast);
            }

            return Ok(Self::ManufacturerBroadcast(manufacturer_uid));
        }

        Ok(Self::Device(UniqueIdentifier {
            manufacturer_uid: 0,
            device_uid: 0,
        }))
    }
}

impl From<PackageAddress> for u64 {
    fn from(value: PackageAddress) -> Self {
        match value {
            PackageAddress::Broadcast => BROADCAST_UID,
            PackageAddress::ManufacturerBroadcast(manufacturer_uid) => {
                ((manufacturer_uid as u64) << u32::BITS) | u32::MAX as u64
            },
            PackageAddress::Device(uid) => uid.into(),
        }
    }
}
