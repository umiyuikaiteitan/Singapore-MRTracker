//! The DataMall API client.

use serde::de::DeserializeOwned;

use crate::error::DataMallError;
use crate::key::AccountKey;
use crate::model::{
    CrowdForecastDay, DatasetLink, Envelope, PlatformCrowd, RawLink, TrainLine, TrainServiceAlerts,
};
use crate::transport::{Response, Transport};

/// The production base URL of the DataMall OData service.
pub const DEFAULT_BASE_URL: &str = "https://datamall2.mytransport.sg/ltaodataservice";

/// A client for the LTA DataMall rail APIs.
///
/// The client is generic over a [`Transport`], so tests and special
/// environments can supply their own HTTP stack. With the default
/// `http-ureq` feature, [`DataMallClient::from_env`] gives a ready
/// client.
///
/// # Examples
///
/// ```no_run
/// use mrt_datamall::DataMallClient;
///
/// // Reads LTA_DATAMALL_ACCOUNT_KEY from the environment.
/// let client = DataMallClient::from_env().unwrap();
/// let alerts = client.train_service_alerts().unwrap();
/// println!("Rail status: {:?}", alerts.status);
/// ```
#[derive(Debug)]
pub struct DataMallClient<T: Transport> {
    key: AccountKey,
    base_url: String,
    transport: T,
}

#[cfg(feature = "http-ureq")]
impl DataMallClient<crate::transport::UreqTransport> {
    /// Make a client with the default transport and the key from the
    /// `LTA_DATAMALL_ACCOUNT_KEY` environment variable.
    pub fn from_env() -> Result<Self, DataMallError> {
        Ok(Self::new(
            AccountKey::from_env()?,
            crate::transport::UreqTransport::new(),
        ))
    }

    /// Make a client with the default transport and the given key.
    pub fn with_key(key: AccountKey) -> Self {
        Self::new(key, crate::transport::UreqTransport::new())
    }
}

impl<T: Transport> DataMallClient<T> {
    /// Make a client from a key and a transport.
    pub fn new(key: AccountKey, transport: T) -> Self {
        DataMallClient {
            key,
            base_url: DEFAULT_BASE_URL.to_string(),
            transport,
        }
    }

    /// Change the base URL. Useful for tests and proxies.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Get the base URL of this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ------------------------------------------------------------------
    // GTFS datasets
    // ------------------------------------------------------------------

    /// Get the download link for the GTFS Schedule feed for trains.
    ///
    /// The API endpoint is `GTFSScheduleTrain`. The link points to a
    /// GTFS zip archive and expires after a short time.
    pub fn gtfs_schedule_link(&self) -> Result<DatasetLink, DataMallError> {
        self.dataset_link("GTFSScheduleTrain")
    }

    /// Get the download link for the GTFS-Realtime trip updates feed
    /// for trains.
    ///
    /// The API endpoint is `GTFSRealtimeTrainTripUpdates`. The link
    /// points to a Protocol Buffer file and expires after a short
    /// time.
    pub fn gtfs_trip_updates_link(&self) -> Result<DatasetLink, DataMallError> {
        self.dataset_link("GTFSRealtimeTrainTripUpdates")
    }

    /// Get the download link for the GTFS-Realtime service alerts feed
    /// for trains.
    ///
    /// The API endpoint is `GTFSRealTimeTrainServiceAlerts`. The link
    /// points to a Protocol Buffer file and expires after a short
    /// time.
    pub fn gtfs_service_alerts_link(&self) -> Result<DatasetLink, DataMallError> {
        self.dataset_link("GTFSRealTimeTrainServiceAlerts")
    }

    /// Download the GTFS Schedule feed for trains.
    ///
    /// The function gets the link and downloads the zip archive in one
    /// step. Feed the bytes to `mrt_gtfs::ZipSource::from_reader`
    /// through a `std::io::Cursor`.
    pub fn fetch_gtfs_schedule(&self) -> Result<Vec<u8>, DataMallError> {
        let link = self.gtfs_schedule_link()?;
        self.download(&link.url)
    }

    /// Download the GTFS-Realtime trip updates message for trains.
    ///
    /// Decode the bytes with `mrt_gtfs_rt::RailRtFeed::decode`.
    pub fn fetch_trip_updates(&self) -> Result<Vec<u8>, DataMallError> {
        let link = self.gtfs_trip_updates_link()?;
        self.download(&link.url)
    }

    /// Download the GTFS-Realtime service alerts message for trains.
    ///
    /// Decode the bytes with `mrt_gtfs_rt::RailRtFeed::decode`.
    pub fn fetch_service_alerts(&self) -> Result<Vec<u8>, DataMallError> {
        let link = self.gtfs_service_alerts_link()?;
        self.download(&link.url)
    }

    // ------------------------------------------------------------------
    // Live rail status
    // ------------------------------------------------------------------

    /// Get the legacy train service alerts.
    ///
    /// The API endpoint is `TrainServiceAlerts`. It reports disrupted
    /// line segments and free bridging services in a simple JSON
    /// format.
    pub fn train_service_alerts(&self) -> Result<TrainServiceAlerts, DataMallError> {
        let envelope: Envelope<TrainServiceAlerts> = self.get_json("TrainServiceAlerts")?;
        Ok(envelope.value)
    }

    /// Get the current platform crowd density for one line.
    ///
    /// The API endpoint is `PCDRealTime`. LTA updates the data every
    /// 10 minutes.
    pub fn platform_crowd(&self, line: TrainLine) -> Result<Vec<PlatformCrowd>, DataMallError> {
        let path = format!("PCDRealTime?TrainLine={}", line.code());
        let envelope: Envelope<Vec<PlatformCrowd>> = self.get_json(&path)?;
        Ok(envelope.value)
    }

    /// Get the platform crowd density forecast for one line.
    ///
    /// The API endpoint is `PCDForecast`. It returns 30-minute
    /// forecast intervals for the whole day.
    pub fn platform_crowd_forecast(
        &self,
        line: TrainLine,
    ) -> Result<Vec<CrowdForecastDay>, DataMallError> {
        let path = format!("PCDForecast?TrainLine={}", line.code());
        let envelope: Envelope<Vec<CrowdForecastDay>> = self.get_json(&path)?;
        Ok(envelope.value)
    }

    // ------------------------------------------------------------------
    // Statistics
    // ------------------------------------------------------------------

    /// Get the download link for the passenger volume by train
    /// stations dataset.
    ///
    /// The API endpoint is `PV/Train`. The optional `month` value uses
    /// the `YYYYMM` format and selects one of the last three months.
    pub fn train_passenger_volume_link(
        &self,
        month: Option<&str>,
    ) -> Result<DatasetLink, DataMallError> {
        let path = match month {
            Some(month) => format!("PV/Train?Date={month}"),
            None => "PV/Train".to_string(),
        };
        self.dataset_link(&path)
    }

    // ------------------------------------------------------------------
    // Low-level access
    // ------------------------------------------------------------------

    /// Get any DataMall path as raw JSON.
    ///
    /// Use this escape hatch for endpoints that this crate does not
    /// model yet. The `path_and_query` value is relative to the base
    /// URL, for example `FacilitiesMaintenance?StationCode=NS1`.
    pub fn get_raw(&self, path_and_query: &str) -> Result<serde_json::Value, DataMallError> {
        self.get_json(path_and_query)
    }

    /// Download a file from a pre-signed dataset link.
    ///
    /// The request goes to the given URL without the account key,
    /// because the link carries its own signature.
    pub fn download(&self, url: &str) -> Result<Vec<u8>, DataMallError> {
        let response = self.transport.get(url, &[])?;
        if !(200..300).contains(&response.status) {
            return Err(DataMallError::Http {
                status: response.status,
                url: url.to_string(),
            });
        }
        Ok(response.body)
    }

    /// Send an authenticated GET request to a DataMall path.
    fn request(&self, path_and_query: &str) -> Result<(String, Response), DataMallError> {
        let url = format!("{}/{}", self.base_url, path_and_query);
        let headers = [
            ("AccountKey", self.key.as_str()),
            ("accept", "application/json"),
        ];
        let response = self.transport.get(&url, &headers)?;
        match response.status {
            200..=299 => Ok((url, response)),
            401 => Err(DataMallError::InvalidKey),
            429 => Err(DataMallError::RateLimited),
            status => Err(DataMallError::Http { status, url }),
        }
    }

    fn get_json<M: DeserializeOwned>(&self, path_and_query: &str) -> Result<M, DataMallError> {
        let (url, response) = self.request(path_and_query)?;
        serde_json::from_slice(&response.body).map_err(|e| DataMallError::Decode {
            url,
            message: e.to_string(),
        })
    }

    fn dataset_link(&self, path_and_query: &str) -> Result<DatasetLink, DataMallError> {
        let (url, response) = {
            let (url, response) = self.request(path_and_query)?;
            (url, response)
        };
        let envelope: Envelope<Vec<RawLink>> =
            serde_json::from_slice(&response.body).map_err(|e| DataMallError::Decode {
                url: url.clone(),
                message: e.to_string(),
            })?;
        envelope
            .value
            .into_iter()
            .find_map(|record| {
                record.link.map(|link| DatasetLink {
                    timestamp: record.timestamp,
                    url: link,
                })
            })
            .ok_or(DataMallError::NoLink { url })
    }
}
