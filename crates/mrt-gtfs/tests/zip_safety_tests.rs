//! Security tests for the zip feed source.
//!
//! A downloaded GTFS archive is untrusted input. These tests build
//! hostile archives and check that the loader refuses them before it
//! reads any content.

#![cfg(feature = "zip-source")]

use std::io::{Cursor, Write as _};

use mrt_gtfs::{GtfsError, GtfsFeed, ZipLimits, ZipOptions, ZipSource, ZipStrictness};

/// The smallest set of files that makes a valid feed.
const FILES: [(&str, &str); 5] = [
    ("stops.txt", "stop_id,stop_name\nS1,Alpha\nS2,Beta\n"),
    (
        "routes.txt",
        "route_id,route_short_name,route_type\nR1,NS,1\n",
    ),
    ("trips.txt", "route_id,service_id,trip_id\nR1,WK,T1\n"),
    (
        "stop_times.txt",
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n\
         T1,06:00:00,06:00:30,S1,1\nT1,06:10:00,06:10:00,S2,2\n",
    ),
    (
        "calendar.txt",
        "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n\
         WK,1,1,1,1,1,0,0,20250101,20271231\n",
    ),
];

/// Pack the feed with a name prefix and optional extra entries.
fn pack(prefix: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for (name, body) in FILES.iter().chain(extra.iter()) {
        let full = if FILES.iter().any(|(f, _)| f == name) {
            format!("{prefix}{name}")
        } else {
            (*name).to_string()
        };
        writer.start_file(full, options).unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    buffer.into_inner()
}

fn open(bytes: Vec<u8>) -> Result<ZipSource<Cursor<Vec<u8>>>, GtfsError> {
    ZipSource::from_reader(Cursor::new(bytes))
}

fn message(error: &GtfsError) -> String {
    error.to_string()
}

#[test]
fn a_clean_archive_loads() {
    let mut source = open(pack("", &[])).unwrap();
    let feed = GtfsFeed::load(&mut source).unwrap();
    assert_eq!(feed.stops.len(), 2);
}

#[test]
fn path_traversal_entries_are_refused() {
    for hostile in ["../evil.txt", "gtfs/../../evil.txt", "a/../../b/evil.txt"] {
        let error = open(pack("", &[(hostile, "x")])).unwrap_err();
        assert!(
            matches!(error, GtfsError::UnsafeZip(_)),
            "{hostile} was accepted: {}",
            message(&error)
        );
        assert!(message(&error).contains(".."));
    }
}

#[test]
fn absolute_entries_are_refused() {
    for hostile in ["/etc/passwd", "C:\\Windows\\system.ini"] {
        let error = open(pack("", &[(hostile, "x")])).unwrap_err();
        assert!(
            matches!(error, GtfsError::UnsafeZip(_)),
            "{hostile} was accepted"
        );
        assert!(message(&error).contains("absolute path"));
    }
}

#[test]
fn a_duplicated_feed_file_is_refused() {
    // Two stops.txt entries make the feed ambiguous, so the loader
    // refuses to guess which one is the real table.
    let mut buffer = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for (name, body) in FILES {
        writer.start_file(name, options).unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.start_file("nested/stops.txt", options).unwrap();
    writer.write_all(b"stop_id,stop_name\nX,Other\n").unwrap();
    writer.finish().unwrap();

    let error = open(buffer.into_inner()).unwrap_err();
    assert!(matches!(error, GtfsError::UnsafeZip(_)));
    assert!(message(&error).contains("twice"), "{}", message(&error));
}

#[test]
fn an_archive_with_too_many_entries_is_refused() {
    let options = ZipOptions {
        limits: ZipLimits {
            max_entries: 3,
            ..ZipLimits::default()
        },
        strictness: ZipStrictness::Lenient,
    };
    let error = ZipSource::from_reader_with(Cursor::new(pack("", &[])), &options).unwrap_err();
    assert!(matches!(error, GtfsError::UnsafeZip(_)));
    assert!(message(&error).contains("entries"));
}

#[test]
fn an_oversized_expansion_is_refused() {
    // A decompression bomb: a small archive that expands to much more
    // than the limit allows.
    let bomb = "0".repeat(200_000);
    let options = ZipOptions {
        limits: ZipLimits {
            max_total_bytes: 4096,
            max_entry_bytes: 4096,
            ..ZipLimits::default()
        },
        strictness: ZipStrictness::Lenient,
    };
    let error =
        ZipSource::from_reader_with(Cursor::new(pack("", &[("bomb.txt", &bomb)])), &options)
            .unwrap_err();
    assert!(matches!(error, GtfsError::UnsafeZip(_)));
    assert!(
        message(&error).contains("expands to"),
        "{}",
        message(&error)
    );
}

#[test]
fn lenient_mode_accepts_a_feed_in_a_subdirectory() {
    let mut source = open(pack("gtfs/", &[])).unwrap();
    let feed = GtfsFeed::load(&mut source).unwrap();
    assert_eq!(feed.stops.len(), 2);
}

#[test]
fn strict_mode_expects_the_feed_at_the_archive_root() {
    let options = ZipOptions {
        limits: ZipLimits::default(),
        strictness: ZipStrictness::Strict,
    };
    let error = ZipSource::from_reader_with(Cursor::new(pack("gtfs/", &[])), &options).unwrap_err();
    assert!(matches!(error, GtfsError::UnsafeZip(_)));
    assert!(
        message(&error).contains("root of the archive"),
        "{}",
        message(&error)
    );

    // The same archive without the prefix passes strict mode.
    let mut source = ZipSource::from_reader_with(Cursor::new(pack("", &[])), &options).unwrap();
    assert_eq!(GtfsFeed::load(&mut source).unwrap().stops.len(), 2);
}
