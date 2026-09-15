//! # mrt-datamall
//!
//! A client for the LTA DataMall rail APIs.
//!
//! This crate is the live-data layer of the Singapore-MRTracker
//! project. It downloads the official GTFS datasets for trains and
//! reads the live rail status APIs:
//!
//! | Method | DataMall endpoint | Data |
//! |--------|-------------------|------|
//! | [`DataMallClient::gtfs_schedule_link`] | `GTFSScheduleTrain` | Link to the GTFS Schedule zip archive |
//! | [`DataMallClient::gtfs_trip_updates_link`] | `GTFSRealtimeTrainTripUpdates` | Link to the GTFS-Realtime trip updates file |
//! | [`DataMallClient::gtfs_service_alerts_link`] | `GTFSRealTimeTrainServiceAlerts` | Link to the GTFS-Realtime service alerts file |
//! | [`DataMallClient::train_service_alerts`] | `TrainServiceAlerts` | Legacy JSON service alerts |
//! | [`DataMallClient::platform_crowd`] | `PCDRealTime` | Live platform crowd density |
//! | [`DataMallClient::platform_crowd_forecast`] | `PCDForecast` | Platform crowd density forecast |
//! | [`DataMallClient::train_passenger_volume_link`] | `PV/Train` | Link to monthly passenger volume data |
//!
//! # The account key
//!
//! Every DataMall request carries your account key in the
//! `AccountKey` header. LTA issues the key when you register at
//! DataMall. The key is a secret:
//!
//! - Do not write the key into source code.
//! - Do not commit the key to the repository.
//! - Supply the key through the `LTA_DATAMALL_ACCOUNT_KEY`
//!   environment variable, or construct an [`AccountKey`] from your
//!   own secret store.
//!
//! # Design notes
//!
//! - The [`Transport`] trait isolates the HTTP stack. The unit tests
//!   use a mock transport and never touch the network.
//! - Every fetch of an external link goes through
//!   [`DataMallClient::download_limited`]: the link must use HTTPS,
//!   and the body must stay within [`MAX_DATASET_BYTES`]. An
//!   oversized response is an error, never a truncated file.
//! - The crate is synchronous and has a small dependency set. This
//!   keeps a future port to another language simple.
//! - Timestamps stay as ISO 8601 strings. Parse them with the
//!   date-time library of your choice.

#![warn(missing_docs)]

mod client;
mod error;
mod key;
mod model;
mod snapshot;
mod transport;

pub use client::{DataMallClient, DEFAULT_BASE_URL};
pub use error::DataMallError;
pub use key::{AccountKey, ACCOUNT_KEY_ENV};
pub use model::{
    AffectedSegment, AlertMessage, CrowdForecastDay, CrowdInterval, CrowdLevel, DatasetLink,
    PlatformCrowd, ServiceStatus, StationCrowdForecast, TrainLine, TrainServiceAlerts,
};
pub use snapshot::{redact_url, sha256_hex, DataMallSnapshot};
#[cfg(feature = "http-ureq")]
pub use transport::UreqTransport;
pub use transport::{Response, Transport, TransportError, MAX_DATASET_BYTES};
