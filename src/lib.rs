//! Rust library for communicating DMX512 (ANSI E1.11) and DMX-RDM (ANSI E1.20) over a RS485 bus by
//! using interchangeable drivers. This library features no-std as well as no-alloc support
//! (no heap allocation) to target embedded as well as os platforms.
//!
//! Please refer to the [official specifications](https://tsp.esta.org/) published by the ESTA.
//!
//! <div class="warning">This library is wip, it has not yet received extensive testing and the api
//! might not be final.</div>
//!
//! # Usage
//! These examples show the basic usage using the dmx-rdm-ftdi driver.
//! These examples work together.
//!
//! ## Controller
//!
//! ```rust
//! use dmx_rdm::dmx_controller::{DmxController, DmxControllerConfig};
//! use dmx_rdm::unique_identifier::{PackageAddress, UniqueIdentifier};
//! use dmx_rdm::utils::run_full_discovery;
//! use dmx_rdm_ftdi::{FtdiDriver, FtdiDriverConfig};
//!
//! let dmx_driver = FtdiDriver::new(FtdiDriverConfig::default()).unwrap();
//! let mut dmx_controller = DmxController::new(dmx_driver, &DmxControllerConfig::default());
//!
//! let mut devices_found = vec![];
//!
//! // Unmute all dmx responders.
//! dmx_controller
//!     .rdm_disc_un_mute(PackageAddress::Broadcast)
//!     .unwrap();
//!
//! let mut uid_array = [UniqueIdentifier::new(1, 1).unwrap(); 512];
//! loop {
//!     // Search for devices.
//!     let amount_devices_found = run_full_discovery(&mut dmx_controller, &mut uid_array).unwrap();
//!
//!     // Add found devices to vector.
//!     devices_found.extend_from_slice(&uid_array[..amount_devices_found]);
//!
//!     // Have all devices been found and muted?
//!     if amount_devices_found != uid_array.len() {
//!         break;
//!     }
//! }
//!
//! for device in devices_found {
//!     match dmx_controller.rdm_set_identify(PackageAddress::Device(device), true) {
//!         Ok(_) => println!("Activated identify for device_uid {device}"),
//!         Err(error) => {
//!             println!("Activating identify for device_uid {device} failed with {error}")
//!         },
//!     }
//! }
//! ```
//!
//! ## Responder
//!
//! ```rust
//! use dmx_rdm::command_class::RequestCommandClass;
//! use dmx_rdm::dmx_receiver::{DmxResponderHandler, RdmResponder};
//! use dmx_rdm::rdm_data::RdmRequestData;
//! use dmx_rdm::rdm_responder::{DmxReceiverContext, RdmResponderConfig, RdmResult};
//! use dmx_rdm::types::{DataPack, NackReason};
//! use dmx_rdm::unique_identifier::UniqueIdentifier;
//! use dmx_rdm_ftdi::{FtdiDriver, FtdiDriverConfig};
//!
//! struct RdmHandler {
//!     identify: bool,
//! }
//!
//! const PID_IDENTIFY_DEVICE: u16 = 0x1000;
//!
//! impl RdmHandler {
//!     fn handle_get_identify(&self) -> RdmResult {
//!         RdmResult::Acknowledged(DataPack::from_slice(&[self.identify as u8]).unwrap())
//!     }
//!
//!     fn handle_set_identify(&mut self, parameter_data: &[u8]) -> Result<RdmResult, std::fmt::Error> {
//!         // Check if the parameter data has the correct size
//!         if parameter_data.len() != 1 {
//!             return Ok(RdmResult::NotAcknowledged(
//!                 NackReason::DataOutOfRange as u16,
//!             ));
//!         }
//!
//!         // Convert identify flag to bool and set that in the state.
//!         self.identify = parameter_data[0] != 0;
//!
//!         println!("Current identify is {}", self.identify);
//!
//!         // Acknowledge request with an empty response.
//!         Ok(RdmResult::Acknowledged(DataPack::new()))
//!     }
//! }
//!
//! impl DmxResponderHandler for RdmHandler {
//!     type Error = std::fmt::Error;
//!
//!     fn handle_rdm(
//!         &mut self,
//!         request: &RdmRequestData,
//!         _: &mut DmxReceiverContext,
//!     ) -> Result<RdmResult, Self::Error> {
//!         match request.parameter_id {
//!             PID_IDENTIFY_DEVICE => match request.command_class {
//!                 RequestCommandClass::GetCommand => Ok(self.handle_get_identify()),
//!                 RequestCommandClass::SetCommand => {
//!                     self.handle_set_identify(&request.parameter_data)
//!                 },
//!                 _ => Ok(RdmResult::NotAcknowledged(
//!                     NackReason::UnsupportedCommandClass as u16,
//!                 )),
//!             },
//!             _ => Ok(RdmResult::NotAcknowledged(NackReason::UnknownPid as u16)),
//!         }
//!     }
//! }
//!
//!
//! let dmx_driver = FtdiDriver::new(FtdiDriverConfig::default()).unwrap();
//!
//! // Create rdm_responder with space for 32 queued messages.
//! let mut dmx_responder = RdmResponder::<_, 32>::new(
//!     dmx_driver,
//!     RdmResponderConfig {
//!         uid: UniqueIdentifier::new(0x7FF0, 1).unwrap(),
//!         // Won't add PID_IDENTIFY_DEVICE since this is a required pid.
//!         supported_pids: &[],
//!         rdm_receiver_metadata: Default::default(),
//!     },
//! );
//!
//! let mut rdm_handler = RdmHandler { identify: false };
//!
//! loop {
//!     // poll for new packages using our handler
//!     match dmx_responder.poll(&mut rdm_handler) {
//!         Ok(_) => (),
//!         Err(error) => println!("'{error}' during polling"),
//!     }
//! }
//! ```
//!

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod command_class;
pub mod consts;
/// Module for building dmx-rdm controllers.
pub mod dmx_controller;
/// Module for implementing more sophisticated dmx-rdm devices like Enttec DMX Pro, as well as
/// mockups, where a simple uart implementation won't suffice.
pub mod dmx_driver;
/// Module for building dmx-rdm receivers.
pub mod dmx_receiver;
/// Module for simplifying the implementation of new drivers/hardware that behave like direct uart devices.
pub mod dmx_uart_driver;
mod layouts;
mod pids;
pub mod rdm_data;
pub mod rdm_packages;
/// Parser for handling rdm requests without an underlying driver.
/// Mainly for highly interrupt driven applications.
pub mod rdm_responder;
pub mod rdm_types;
pub mod types;
pub mod unique_identifier;
pub mod utils;
