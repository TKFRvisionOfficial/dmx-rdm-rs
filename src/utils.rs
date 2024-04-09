use crate::dmx_controller::{DmxController, RdmResponseError};
use crate::dmx_driver::{DiscoveryOption, DmxError, RdmControllerDriver};
use crate::unique_identifier::{PackageAddress, UniqueIdentifier};

/// Blocking recursive discovery.
///
/// It will find and mute all devices until it captured all of them
/// or the provided uid_array is full.
///
/// Before the first time this function gets called, one should unmute all rdm responders
/// using a DISC_UN_MUTE broadcast. This can be done using the [DmxController::rdm_disc_un_mute]
/// method.
///
/// The returned value is the amount of devices found. If the length of the provided array
/// equals the amount of devices found it is advisable to run this function again with additional
/// array space.
///
/// <div class="warning">Since this function is blocking and does not make polled approaches
/// possible, it is not suitable for embedded. Use this function as a starting point only and create
/// a custom solution that fits your platform and use-case best. Refer to Section 7 of the
/// ANSI E1.20 specifications for this.</div>
pub fn run_full_discovery<Driver: RdmControllerDriver>(
    manager: &mut DmxController<Driver>,
    uid_array: &mut [UniqueIdentifier],
) -> Result<usize, RdmResponseError<Driver::DriverError>> {
    let addresses_found = discover_range(manager, 0x00000001, 0xFFFFFFFFFFFE, uid_array)?;

    Ok(addresses_found)
}

fn discover_range<Driver: RdmControllerDriver>(
    manager: &mut DmxController<Driver>,
    lower_bound: u64,
    upper_bound: u64,
    uid_array: &mut [UniqueIdentifier],
) -> Result<usize, RdmResponseError<Driver::DriverError>> {
    let discovery_option = manager.rdm_discover(lower_bound, upper_bound)?;

    if uid_array.is_empty() {
        return Ok(0);
    }

    match discovery_option {
        DiscoveryOption::Collision => {
            if upper_bound - lower_bound <= 1 {
                return Ok(0);
            }

            let first_lower_bound = lower_bound;
            let first_upper_bound = (upper_bound + lower_bound) / 2;

            let second_lower_bound = first_upper_bound + 1;
            let second_upper_bound = upper_bound;

            let upper_addresses_found =
                discover_range(manager, second_lower_bound, second_upper_bound, uid_array)?;

            let lower_address_found = discover_range(
                manager,
                first_lower_bound,
                first_upper_bound,
                &mut uid_array[upper_addresses_found..],
            )?;

            Ok(upper_addresses_found + lower_address_found)
        },
        DiscoveryOption::NoDevice => Ok(0),
        DiscoveryOption::Found(uid) => {
            match manager.rdm_disc_mute(PackageAddress::Device(uid)) {
                Err(RdmResponseError::DmxError(DmxError::TimeoutError)) => return Ok(0),
                result => result,
            }?;
            uid_array[0] = uid;

            Ok(1)
        },
    }
}

#[inline]
pub(crate) fn calculate_checksum(data: &[u8]) -> u16 {
    let mut checksum = 0u16;

    for byte in data {
        checksum = checksum.wrapping_add(*byte as u16);
    }

    checksum
}
