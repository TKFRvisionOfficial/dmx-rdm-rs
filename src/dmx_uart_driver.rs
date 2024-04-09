#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DmxUartDriverError<E> {
    /// The request timed time out.
    /// IMPORTANT: If you implement a driver make sure this error gets raised instead
    /// of a driver specific error.
    TimeoutError,
    /// A driver specific error.
    DriverError(E),
}

impl<E: core::fmt::Display> core::fmt::Display for DmxUartDriverError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DmxUartDriverError::TimeoutError => write!(f, "timeout error occurred"),
            DmxUartDriverError::DriverError(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "std")]
impl<E: core::fmt::Display + core::fmt::Debug> std::error::Error for DmxUartDriverError<E> {}

impl<E> From<E> for DmxUartDriverError<E> {
    fn from(value: E) -> Self {
        Self::DriverError(value)
    }
}

pub trait DmxUartDriver {
    type DriverError;
}

/// Object to implement access to the uart.
/// It can read frames.
/// It has to communicate at 250000 baud.
pub trait DmxRecvUartDriver: DmxUartDriver {
    /// Read frames (used for rdm discovery response).
    /// Returns the number of bytes actually read.
    fn read_frames(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>>;

    /// Read frames without waiting for break.
    /// Returns the number of bytes actually read.
    fn read_frames_no_break(
        &mut self,
        buffer: &mut [u8],
        timeout_us: u32,
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>>;
}

/// Object to implement access to the uart.
/// It can write frames.
/// It has to communicate at 250000 baud.
pub trait DmxRespUartDriver: DmxUartDriver {
    /// Write dmx frames with break.
    /// Returns the number of bytes actually written.
    fn write_frames(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>>;

    /// Write dmx frames without break (used for rdm discovery response).
    /// Returns the number of bytes actually written.
    fn write_frames_no_break(
        &mut self,
        buffer: &[u8],
    ) -> Result<usize, DmxUartDriverError<Self::DriverError>>;
}
