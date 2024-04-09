#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum RequestCommandClass {
    DiscoveryCommand = 0x10,
    GetCommand = 0x20,
    SetCommand = 0x30,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum ResponseCommandClass {
    DiscoveryCommandResponse = 0x11,
    GetCommandResponse = 0x21,
    SetCommandResponse = 0x31,
}

impl RequestCommandClass {
    /// Returns the corresponding response class.
    pub fn get_response_class(&self) -> ResponseCommandClass {
        match self {
            Self::DiscoveryCommand => ResponseCommandClass::DiscoveryCommandResponse,
            Self::GetCommand => ResponseCommandClass::GetCommandResponse,
            Self::SetCommand => ResponseCommandClass::SetCommandResponse,
        }
    }
}

impl ResponseCommandClass {
    /// Returns the corresponding request class.
    pub fn get_request_class(&self) -> RequestCommandClass {
        match self {
            Self::DiscoveryCommandResponse => RequestCommandClass::DiscoveryCommand,
            Self::GetCommandResponse => RequestCommandClass::GetCommand,
            Self::SetCommandResponse => RequestCommandClass::SetCommand,
        }
    }
}

impl TryFrom<u8> for RequestCommandClass {
    type Error = ();

    /// Tries to parse RequestCommandClass from u8.
    /// Returns error if it can't find a matching class.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x10 => Ok(Self::DiscoveryCommand),
            0x20 => Ok(Self::GetCommand),
            0x30 => Ok(Self::SetCommand),
            _ => Err(()),
        }
    }
}

impl TryFrom<u8> for ResponseCommandClass {
    type Error = ();

    /// Tries to parse ResponseCommandClass from u8.
    /// Returns error if it can't find a matching class.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x11 => Ok(Self::DiscoveryCommandResponse),
            0x21 => Ok(Self::GetCommandResponse),
            0x31 => Ok(Self::SetCommandResponse),
            _ => Err(()),
        }
    }
}
