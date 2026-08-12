//! The command implementations.

use std::io::Write;
use std::path::{Path, PathBuf};

use mrt_datamall::{sha256_hex, AccountKey, DataMallClient};
use mrt_gtfs::{
    validate_feed, Diagnostic, GtfsFeed, RailNetwork, ServiceDate, Severity, ValidationMode,
    ZipOptions, ZipSource, ZipStrictness,
};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, Language, PublicationConfig,
};
use mrt_publication_html::{render_diagram, render_diagram_svg, render_timetable};

use crate::args::{self, Args, Command, Format, Parsed};
use crate::cache::FeedCache;
use crate::error::{CliError, ExitCode};
use crate::fsutil::write_atomic;
use crate::manifest::{self, ArtifactRecord, Manifest, MANIFEST_VERSION};

/// Run the program and return the exit code.
///
/// The function prints its own messages, so `main` only needs to pass
/// the code to the process.
pub fn run<I, S>(arguments: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let parsed = match args::parse(arguments) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("run with --help for the usage.");
            return error.exit.code();
        }
    };
    match parsed {
        Parsed::Help => {
            print!("{}", args::USAGE);
            ExitCode::Success.code()
        }
        Parsed::Version => {
            println!("{}", crate::generator_version());
            ExitCode::Success.code()
        }
        Parsed::Run(args) => match dispatch(&args) {
            Ok(code) => code.code(),
            Err(error) => {
                eprintln!("error: {error}");
                error.exit.code()
            }
        },
    }
}

fn dispatch(args: &Args) -> Result<ExitCode, CliError> {
    match args.command {
        Command::Fetch => fetch(args),
        Command::Timetable => timetable(args),
        Command::Diagram => diagram(args),
        Command::Validate => validate(args),
        Command::Stations => stations(args),
    }
}

// ----------------------------------------------------------------------
// Source acquisition
// ----------------------------------------------------------------------

/// The bytes of a feed and where they came from.
struct FeedSourceResult {
    bytes: Option<Vec<u8>>,
    directory: Option<PathBuf>,
    sha256: String,
    timestamp: Option<String>,
    origin: String,
    from_cache: bool,
}

/// Acquire the feed that the arguments name.
fn acquire(args: &Args) -> Result<FeedSourceResult, CliError> {
    if let Some(path) = &args.feed {
        if path.is_dir() {
            // A directory has no single archive to fingerprint, so the
            // fingerprint covers the feed files themselves, sorted, so
            // it stays stable across runs and machines.
            let sha256 = directory_fingerprint(path)?;
            return Ok(FeedSourceResult {
                bytes: None,
                directory: Some(path.clone()),
                sha256,
                timestamp: None,
                origin: path.display().to_string(),
                from_cache: false,
            });
        }
        let bytes = std::fs::read(path).map_err(|e| {
            CliError::new(
                ExitCode::SourceFailure,
                format!("cannot read the feed {}: {e}", path.display()),
            )
        })?;
        return Ok(FeedSourceResult {
            sha256: sha256_hex(&bytes),
            bytes: Some(bytes),
            directory: None,
            timestamp: None,
            origin: path.display().to_string(),
            from_cache: false,
        });
    }

    let cache = FeedCache::open(&args.cache_dir)?;
    match download(args, &cache) {
        Ok(result) => Ok(result),
        Err(error) if args.allow_stale => {
            let Some(entry) = cache.current() else {
                return Err(CliError::new(
                    ExitCode::SourceFailure,
                    format!("{error}; and the cache holds no earlier feed"),
                ));
            };
            if !args.quiet {
                eprintln!("warning: {error}");
                eprintln!(
                    "warning: using the cached feed {} from the cache; \
                     every generated page will say so",
                    &entry.sha256[..12]
                );
            }
            Ok(FeedSourceResult {
                bytes: Some(cache.read(&entry.sha256)?),
                directory: None,
                sha256: entry.sha256,
                timestamp: entry.dataset_timestamp,
                origin: entry.source_endpoint,
                from_cache: true,
            })
        }
        Err(error) => Err(error),
    }
}

/// Download the current feed and store it in the cache.
fn download(args: &Args, cache: &FeedCache) -> Result<FeedSourceResult, CliError> {
    let key = std::env::var(&args.account_key_env).map_err(|_| {
        CliError::new(
            ExitCode::SourceFailure,
            format!(
                "the environment variable {} is not set; \
                 set it, or name a local feed with --feed",
                args.account_key_env
            ),
        )
    })?;
    let key = AccountKey::new(key).map_err(CliError::from)?;
    if !args.quiet {
        eprintln!("Requesting the train GTFS Schedule dataset from DataMall ...");
    }
    let client = DataMallClient::with_key(key);
    let snapshot = client.fetch_gtfs_schedule_snapshot()?;
    let entry = cache.store(
        &snapshot.bytes,
        snapshot.dataset_timestamp.clone(),
        &snapshot.source_endpoint,
        manifest::unix_now(),
    )?;
    if !args.quiet {
        eprintln!(
            "Cached {} bytes as {} in {}.",
            entry.bytes,
            entry.sha256,
            cache.root().display()
        );
    }
    Ok(FeedSourceResult {
        bytes: Some(snapshot.bytes),
        directory: None,
        sha256: snapshot.sha256,
        timestamp: snapshot.dataset_timestamp,
        origin: snapshot.source_endpoint,
        from_cache: false,
    })
}

/// Fingerprint a feed directory from the contents of its files.
fn directory_fingerprint(path: &Path) -> Result<String, CliError> {
    let fail = |e: std::io::Error| {
        CliError::new(
            ExitCode::SourceFailure,
            format!("cannot read the feed directory {}: {e}", path.display()),
        )
    };
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map_err(fail)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    let mut joined = Vec::new();
    for name in names {
        joined.extend_from_slice(name.as_bytes());
        joined.push(0);
        let bytes = std::fs::read(path.join(&name)).map_err(fail)?;
        joined.extend_from_slice(&sha256_hex(&bytes).into_bytes());
        joined.push(b'\n');
    }
    Ok(sha256_hex(&joined))
}

/// Load the feed and build the rail network.
fn load_network(
    args: &Args,
    source: &FeedSourceResult,
) -> Result<(GtfsFeed, RailNetwork), CliError> {
    let options = ZipOptions {
        limits: Default::default(),
        strictness: if args.strict {
            ZipStrictness::Strict
        } else {
            ZipStrictness::Lenient
        },
    };
    let feed = match (&source.bytes, &source.directory) {
        (_, Some(directory)) => GtfsFeed::from_dir(directory)?,
        (Some(bytes), _) => {
            let mut zip =
                ZipSource::from_reader_with(std::io::Cursor::new(bytes.clone()), &options)?;
            GtfsFeed::load(&mut zip)?
        }
        _ => {
            return Err(CliError::new(
                ExitCode::SourceFailure,
                "no feed bytes and no feed directory",
            ))
        }
    };
    let network = RailNetwork::from_feed(&feed)?;
    Ok((feed, network))
}

/// Read the configuration, or use the defaults.
fn load_config(args: &Args) -> Result<(PublicationConfig, String), CliError> {
    let mut config = match &args.config {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| {
                CliError::usage(format!(
                    "cannot read the configuration {}: {e}",
                    path.display()
                ))
            })?;
            let value = crate::yaml::parse(&text)
                .map_err(|e| CliError::usage(format!("{}: {e}", path.display())))?;
            serde_json::from_value::<PublicationConfig>(value)
                .map_err(|e| CliError::usage(format!("{}: {e}", path.display())))?
        }
        None => PublicationConfig::default(),
    };

    if let Some(language) = &args.language {
        config.language = language.parse::<Language>().map_err(CliError::usage)?;
    }
    if let Some(policy) = &args.frequency_policy {
        config.frequency_policy = match policy.as_str() {
            "bands" => mrt_gtfs::FrequencyPolicy::Bands,
            "expand-approximate" => mrt_gtfs::FrequencyPolicy::ExpandApproximate,
            "reject-non-exact" => mrt_gtfs::FrequencyPolicy::RejectNonExact,
            other => {
                return Err(CliError::usage(format!(
                    "--frequency-policy {other} is not supported; \
                     use bands, expand-approximate, or reject-non-exact"
                )))
            }
        };
    }
    config.check().map_err(CliError::usage)?;

    // The fingerprint covers the effective configuration, including
    // the command-line overrides, so two runs that produce the same
    // document also report the same configuration hash.
    let effective = serde_json::to_string(&config).unwrap_or_default();
    Ok((config, sha256_hex(effective.as_bytes())))
}

/// Build the document seed from the source and the configuration.
fn seed_of(
    source: &FeedSourceResult,
    feed: &GtfsFeed,
    config: &PublicationConfig,
    configuration_sha256: String,
) -> DocumentSeed {
    let timezone = config
        .timezone
        .clone()
        .or_else(|| mrt_gtfs::feed_timezone(feed).map(str::to_string))
        .unwrap_or_else(|| "UTC".to_string());
    DocumentSeed {
        generator_version: crate::generator_version(),
        feed_sha256: source.sha256.clone(),
        feed_timestamp: source.timestamp.clone(),
        timezone,
        generated_from_cache: source.from_cache,
        configuration_sha256,
    }
}

// ----------------------------------------------------------------------
// Commands
// ----------------------------------------------------------------------

fn fetch(args: &Args) -> Result<ExitCode, CliError> {
    let cache = FeedCache::open(&args.cache_dir)?;
    let source = download(args, &cache)?;
    let bytes = source.bytes.clone().unwrap_or_default();

    let mut artifacts = Vec::new();
    if let Some(out) = &args.out {
        let path = PathBuf::from(out);
        write_atomic(&path, &bytes)?;
        artifacts.push(manifest::record(out, "feed", "zip", &bytes));
        if !args.quiet {
            eprintln!("Wrote {} ({} bytes).", path.display(), bytes.len());
        }
    }

    // A downloaded archive must be readable before the run reports
    // success, otherwise a broken download poisons the cache silently.
    let mut zip = ZipSource::from_reader_with(std::io::Cursor::new(bytes), &ZipOptions::default())?;
    let feed = GtfsFeed::load(&mut zip)?;
    if !args.quiet {
        eprintln!(
            "The archive holds {} routes, {} stops, and {} trips.",
            feed.routes.len(),
            feed.stops.len(),
            feed.trips.len()
        );
    }

    finish(
        args,
        &source,
        &feed,
        None,
        "fetch",
        String::new(),
        None,
        artifacts,
        Vec::new(),
    )
}

fn timetable(args: &Args) -> Result<ExitCode, CliError> {
    let source = acquire(args)?;
    let (feed, network) = load_network(args, &source)?;
    let (config, config_sha) = load_config(args)?;
    let seed = seed_of(&source, &feed, &config, config_sha.clone());

    let station_key = args.station.as_deref().unwrap_or_default();
    let station = network
        .station_by_alias(station_key)
        .or_else(|| network.station_by_code(station_key))
        .or_else(|| network.station_by_gtfs_id(station_key))
        .or_else(|| network.station_by_name(station_key))
        .ok_or_else(|| {
            CliError::new(
                ExitCode::Unresolved,
                format!(
                    "no station matches \"{station_key}\"; \
                     run the stations command to list them"
                ),
            )
        })?;
    let line = match &args.line {
        Some(name) => Some(resolve_line(&network, name)?),
        None => None,
    };
    let date = args::parse_date(args.date.as_deref().unwrap_or_default())?;

    let document = build_timetable(&network, station, date, line, &config, &seed)?;
    let body = match args.format {
        Format::Json => to_json(&document)?,
        _ => render_timetable(&document, &config),
    };
    let artifacts = emit(args, &body, "timetable", format_name(args.format))?;

    finish(
        args,
        &source,
        &feed,
        Some(date),
        "timetable",
        config_sha,
        Some(&config),
        artifacts,
        document.metadata.diagnostics.clone(),
    )
}

fn diagram(args: &Args) -> Result<ExitCode, CliError> {
    let source = acquire(args)?;
    let (feed, network) = load_network(args, &source)?;
    let (config, config_sha) = load_config(args)?;
    let seed = seed_of(&source, &feed, &config, config_sha.clone());

    let target = if let Some(id) = &args.corridor {
        if config.corridor(id).is_none() {
            return Err(CliError::new(
                ExitCode::Unresolved,
                format!("the configuration defines no corridor \"{id}\""),
            ));
        }
        DiagramTarget::Corridor(id.clone())
    } else if let Some(index) = args.pattern {
        if index >= network.patterns().len() {
            return Err(CliError::new(
                ExitCode::Unresolved,
                format!(
                    "the feed has {} stop patterns, so --pattern {index} does not exist",
                    network.patterns().len()
                ),
            ));
        }
        DiagramTarget::Pattern(mrt_gtfs::PatternId(index))
    } else {
        DiagramTarget::Line(resolve_line(&network, args.line.as_deref().unwrap_or(""))?)
    };

    let date = args::parse_date(args.date.as_deref().unwrap_or_default())?;
    let from = match &args.from {
        Some(value) => args::parse_time("from", value)?,
        None => config.day_start,
    };
    let until = match &args.until {
        Some(value) => args::parse_time("until", value)?,
        None => config.day_end(),
    };

    let document = build_diagram(&network, &target, date, from, until, &config, &seed)?;
    let body = match args.format {
        Format::Json => to_json(&document)?,
        Format::Svg => render_diagram_svg(&document, &config),
        Format::Html => render_diagram(&document, &config),
    };
    let artifacts = emit(args, &body, "diagram", format_name(args.format))?;

    finish(
        args,
        &source,
        &feed,
        Some(date),
        "diagram",
        config_sha,
        Some(&config),
        artifacts,
        document.metadata.diagnostics.clone(),
    )
}

fn validate(args: &Args) -> Result<ExitCode, CliError> {
    let source = acquire(args)?;
    let options = ZipOptions {
        limits: Default::default(),
        strictness: if args.strict {
            ZipStrictness::Strict
        } else {
            ZipStrictness::Lenient
        },
    };
    let feed = match (&source.bytes, &source.directory) {
        (_, Some(directory)) => GtfsFeed::from_dir(directory)?,
        (Some(bytes), _) => {
            let mut zip =
                ZipSource::from_reader_with(std::io::Cursor::new(bytes.clone()), &options)?;
            GtfsFeed::load(&mut zip)?
        }
        _ => {
            return Err(CliError::new(
                ExitCode::SourceFailure,
                "no feed to validate",
            ))
        }
    };
    let mode = if args.strict {
        ValidationMode::Strict
    } else {
        ValidationMode::Lenient
    };
    let report = validate_feed(&feed, mode);

    for diagnostic in report.iter() {
        println!("{diagnostic}");
    }
    if !args.quiet {
        eprintln!(
            "{} error(s), {} warning(s), {} note(s).",
            report.count(Severity::Error),
            report.count(Severity::Warning),
            report.count(Severity::Info)
        );
    }
    let diagnostics = report.into_diagnostics();
    let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);

    let code = finish(
        args,
        &source,
        &feed,
        None,
        "validate",
        String::new(),
        None,
        Vec::new(),
        diagnostics,
    )?;
    if has_errors {
        return Ok(ExitCode::InvalidFeed);
    }
    Ok(code)
}

fn stations(args: &Args) -> Result<ExitCode, CliError> {
    let source = acquire(args)?;
    let (_, network) = load_network(args, &source)?;

    println!("# lines");
    for line in network.lines() {
        println!(
            "{}\t{}\t{}",
            line.route_id,
            line.name,
            line.long_name.as_deref().unwrap_or("")
        );
    }
    println!("# stations");
    for station in network.stations() {
        println!(
            "{}\t{}\t{}",
            station.codes.join(","),
            station.name,
            station.gtfs_id
        );
    }
    Ok(ExitCode::Success)
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

fn resolve_line(network: &RailNetwork, name: &str) -> Result<mrt_gtfs::LineId, CliError> {
    network
        .line_by_route_id(name)
        .or_else(|| {
            network
                .lines()
                .iter()
                .position(|line| line.name.eq_ignore_ascii_case(name))
                .map(mrt_gtfs::LineId)
        })
        .or_else(|| {
            network
                .lines()
                .iter()
                .position(|line| {
                    line.long_name
                        .as_deref()
                        .is_some_and(|long| long.eq_ignore_ascii_case(name))
                })
                .map(mrt_gtfs::LineId)
        })
        .ok_or_else(|| {
            let names: Vec<&str> = network.lines().iter().map(|l| l.name.as_str()).collect();
            CliError::new(
                ExitCode::Unresolved,
                format!(
                    "no line matches \"{name}\"; the feed carries {}",
                    names.join(", ")
                ),
            )
        })
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::Html => "html",
        Format::Svg => "svg",
        Format::Json => "json",
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, CliError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| {
        CliError::new(
            ExitCode::OutputFailure,
            format!("cannot serialize the view model: {e}"),
        )
    })?;
    text.push('\n');
    Ok(text)
}

/// Write the artifact, to a file or to standard output.
fn emit(
    args: &Args,
    body: &str,
    kind: &str,
    format: &str,
) -> Result<Vec<ArtifactRecord>, CliError> {
    match args.out.as_deref() {
        None | Some("-") => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(body.as_bytes()).map_err(|e| {
                CliError::new(ExitCode::OutputFailure, format!("cannot write output: {e}"))
            })?;
            Ok(Vec::new())
        }
        Some(path) => {
            write_atomic(Path::new(path), body.as_bytes())?;
            if !args.quiet {
                eprintln!("Wrote {path} ({} bytes).", body.len());
            }
            Ok(vec![manifest::record(path, kind, format, body.as_bytes())])
        }
    }
}

/// Write the manifest and decide the exit code.
#[allow(clippy::too_many_arguments)]
fn finish(
    args: &Args,
    source: &FeedSourceResult,
    feed: &GtfsFeed,
    service_date: Option<ServiceDate>,
    command: &str,
    configuration_sha256: String,
    config: Option<&PublicationConfig>,
    artifacts: Vec<ArtifactRecord>,
    diagnostics: Vec<Diagnostic>,
) -> Result<ExitCode, CliError> {
    let timezone = config
        .and_then(|c| c.timezone.clone())
        .or_else(|| mrt_gtfs::feed_timezone(feed).map(str::to_string))
        .unwrap_or_else(|| "UTC".to_string());

    let manifest = Manifest {
        manifest_version: MANIFEST_VERSION.to_string(),
        generator_version: crate::generator_version(),
        generated_at: manifest::unix_now(),
        command: command.to_string(),
        feed_sha256: source.sha256.clone(),
        feed_timestamp: source.timestamp.clone(),
        feed_source: source.origin.clone(),
        feed_from_cache: source.from_cache,
        configuration_sha256,
        configuration_path: args.config.as_ref().map(|p| p.display().to_string()),
        service_date: service_date.map(|d| d.to_string()),
        timezone,
        schema_version: mrt_publication::SCHEMA_VERSION.to_string(),
        artifacts,
        diagnostics,
    };
    if let Some(path) = &args.manifest {
        manifest.write(path)?;
        if !args.quiet {
            eprintln!("Wrote {}.", path.display());
        }
    }
    if !args.quiet {
        for diagnostic in manifest
            .diagnostics
            .iter()
            .filter(|d| d.severity >= Severity::Warning)
        {
            eprintln!("warning: {diagnostic}");
        }
    }
    if args.warnings_as_errors && manifest.has_warnings() {
        return Err(CliError::new(
            ExitCode::InvalidFeed,
            "the run produced warnings and --warnings-as-errors is set",
        ));
    }
    Ok(ExitCode::Success)
}
