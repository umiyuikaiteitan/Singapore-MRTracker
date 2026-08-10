# Data sources

This document lists the LTA DataMall resources that the library
uses, with their request and response formats. It applies the
ASD-STE100 writing rules where possible.

The authoritative reference is the LTA DataMall API User Guide:
<https://datamall.lta.gov.sg/content/dam/datamall/datasets/LTA_DataMall_API_User_Guide.pdf>.
The response examples below come from the official DataMall sample
files.

## Access

- Base URL: `https://datamall2.mytransport.sg/ltaodataservice`
- Every request carries the account key in the `AccountKey` header.
- Register at <https://datamall.lta.gov.sg> to receive a key.
- Responses use the OData envelope: the payload is in the `value`
  field.

## GTFS datasets for trains

Three endpoints return a link to a dataset file. The link is a
pre-signed URL and expires after approximately 15 minutes
(`X-Amz-Expires=900`). Download the file directly after you receive
the link. The download itself needs no account key.

| Endpoint | File behind the link |
|----------|----------------------|
| `GTFSScheduleTrain` | `gtfs_schedule.zip` — a GTFS Schedule feed |
| `GTFSRealtimeTrainTripUpdates` | `gtfs_trip_update.pb` — a GTFS-Realtime `FeedMessage` |
| `GTFSRealTimeTrainServiceAlerts` | `gtfs_realtime.pb` — a GTFS-Realtime `FeedMessage` |

Note the spelling: the trip updates endpoint uses `Realtime`, the
service alerts endpoint uses `RealTime`. The library uses the exact
paths.

Response shape (official sample, link shortened):

```json
{
    "odata.metadata": "https://datamall2.mytransport.sg/ltaodataservice/GTFSScheduleTrain",
    "value": [
        {
            "timestamp": "2026-07-31T17:14:35+08:00",
            "link": "https://dmprod-datasets.s3.ap-southeast-1.amazonaws.com/train-gtfs-schedule/gtfs_schedule.zip?X-Amz-..."
        }
    ]
}
```

Client methods: `gtfs_schedule_link`, `gtfs_trip_updates_link`,
`gtfs_service_alerts_link`, and the combined `fetch_gtfs_schedule`,
`fetch_trip_updates`, `fetch_service_alerts`.

## Train service alerts (legacy JSON)

- Endpoint: `TrainServiceAlerts`
- `Status`: `1` = normal service, `2` = disrupted service.
- `AffectedSegments` lists the disrupted line segments. The
  `Stations` field is one delimited string of station codes. The
  library accepts a comma or a hyphen as the delimiter.
- `Message` lists the public messages.

```json
{
    "value": {
        "Status": 2,
        "AffectedSegments": [
            {
                "Line": "NEL",
                "Direction": "HarbourFront",
                "Stations": "NE1,NE3,NE4",
                "FreePublicBus": "NE1,NE3,NE4",
                "FreeMRTShuttle": "NE1,NE3,NE4",
                "MRTShuttleDirection": "HarbourFront"
            }
        ],
        "Message": [
            { "Content": "...", "CreatedDate": "2026-01-21 17:17:11" }
        ]
    }
}
```

Client method: `train_service_alerts`.

## Platform crowd density

- Live endpoint: `PCDRealTime?TrainLine=<code>`
- Forecast endpoint: `PCDForecast?TrainLine=<code>`
- LTA updates the live data every 10 minutes.
- Crowd levels: `l` = low, `m` = moderate, `h` = high. Other values
  map to `Unknown`.

Line codes for the `TrainLine` parameter:
`CCL`, `CEL`, `CGL`, `DTL`, `EWL`, `NEL`, `NSL`, `BPL`, `SLRT`,
`PLRT`, `TEL`.

Live response (official sample, shortened):

```json
{
    "value": [
        {
            "Station": "BP11",
            "StartTime": "2021-11-02T13:20:00+08:00",
            "EndTime": "2021-11-02T13:30:00+08:00",
            "CrowdLevel": "l"
        }
    ]
}
```

The forecast response nests 30-minute intervals per station inside
one record per day.

Client methods: `platform_crowd`, `platform_crowd_forecast`.

## Passenger volume

- Endpoint: `PV/Train`, optional parameter `Date=YYYYMM`.
- The response contains a `Link` to a zip archive with CSV data for
  one month. LTA keeps the last three months.

Client method: `train_passenger_volume_link`.

## Other endpoints

DataMall has more rail endpoints, for example
`FacilitiesMaintenance`. Use `DataMallClient::get_raw` to call an
endpoint that the library does not model yet.

## Notes on the station code alphabet

- MRT codes: `NS`, `EW`, `CG`, `NE`, `CC`, `CE`, `DT`, `TE` plus a
  number, for example `NS1`.
- LRT codes: `BP`, `STC`, `SE`, `SW`, `PTC`, `PE`, `PW` plus a
  number.
- The alerts API can report LRT loops as `SEL`, `SWL`, `PEL`, and
  `PWL`. The `TrainLine` parser maps these codes to `SLRT` and
  `PLRT`.
