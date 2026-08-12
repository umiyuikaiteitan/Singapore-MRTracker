//! Command-line parsing.
//!
//! The parser is hand-written, which keeps the dependency set of the
//! workspace small and keeps the messages under our control. It reads
//! `--name value` and `--name=value`, and it refuses an unknown
//! option rather than ignoring it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::CliError;

/// The usage text.
pub const USAGE: &str = "\
mrt-schedule-cli — generate station timetables and train diagrams from a GTFS feed

USAGE
  mrt-schedule-cli fetch     [OPTIONS]
  mrt-schedule-cli timetable [OPTIONS]
  mrt-schedule-cli diagram   [OPTIONS]
  mrt-schedule-cli validate  [OPTIONS]
  mrt-schedule-cli stations  [OPTIONS]

COMMANDS
  fetch       Download the current train GTFS Schedule archive and cache it.
  timetable   Build a departure timetable for one station.
  diagram     Build a time-distance train diagram for one corridor.
  validate    Check a feed and print its diagnostics.
  stations    List the stations and lines of a feed.

SOURCE
  --feed <PATH>          A GTFS zip archive or a directory of feed files.
  --source datamall      Download from LTA DataMall instead.
  --cache-dir <PATH>     The feed cache (default: cache).
  --allow-stale          After a failed download, use the cached feed and say
                         so on every generated page.
  --account-key-env <N>  The environment variable that holds the DataMall
                         account key (default: LTA_DATAMALL_ACCOUNT_KEY).

SELECTION
  --station <CODE>       A station code, name, or GTFS identifier.
  --line <NAME>          A route identifier or a route short name.
  --pattern <INDEX>      A stop pattern index, for a diagram.
  --corridor <ID>        A corridor from the configuration, for a diagram.
  --date <YYYY-MM-DD>    The service date. Also accepts YYYYMMDD.
  --from <HH:MM:SS>      The start of a diagram window.
  --until <HH:MM:SS>     The end of a diagram window, exclusive.

OUTPUT
  --out <PATH>           The artifact to write. \"-\" writes to standard output.
  --format <FORMAT>      html (default), svg, or json.
  --manifest <PATH>      Also write a generation manifest.
  --config <PATH>        A YAML configuration file.
  --language <en|ja>     Override the configured interface language.
  --frequency-policy <P> bands, expand-approximate, or reject-non-exact.
  --strict               Read the feed and validate it strictly.
  --warnings-as-errors   Exit with code 4 when the run produces a warning.
  --quiet                Print nothing but errors.
  -h, --help             Print this text.
  -V, --version          Print the version.

EXIT CODES
  0 success                       4 invalid GTFS feed
  2 invalid command or config     5 unresolved station, line, or corridor
  3 source acquisition failure    6 output not representable under the policy
                                  7 rendering or file-write failure
";

/// The subcommand.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Download and cache the feed.
    Fetch,
    /// Build a station timetable.
    Timetable,
    /// Build a train diagram.
    Diagram,
    /// Validate a feed.
    Validate,
    /// List stations and lines.
    Stations,
}

impl Command {
    fn parse(name: &str) -> Option<Command> {
        Some(match name {
            "fetch" => Command::Fetch,
            "timetable" => Command::Timetable,
            "diagram" => Command::Diagram,
            "validate" => Command::Validate,
            "stations" => Command::Stations,
            _ => return None,
        })
    }
}

/// The artifact format.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// A self-contained HTML page. The default.
    #[default]
    Html,
    /// A standalone SVG drawing. Diagrams only.
    Svg,
    /// The versioned JSON view model.
    Json,
}

impl Format {
    fn parse(value: &str) -> Result<Format, CliError> {
        Ok(match value {
            "html" => Format::Html,
            "svg" => Format::Svg,
            "json" => Format::Json,
            other => {
                return Err(CliError::usage(format!(
                    "--format {other} is not supported; use html, svg, or json"
                )))
            }
        })
    }
}

/// What the caller asked for.
#[derive(Clone, Debug)]
pub struct Args {
    /// The subcommand.
    pub command: Command,
    /// A local feed path.
    pub feed: Option<PathBuf>,
    /// Whether to download from DataMall.
    pub datamall: bool,
    /// The cache directory.
    pub cache_dir: PathBuf,
    /// Whether a cached feed may stand in for a failed download.
    pub allow_stale: bool,
    /// The environment variable that holds the account key.
    pub account_key_env: String,
    /// The station selector.
    pub station: Option<String>,
    /// The line selector.
    pub line: Option<String>,
    /// The stop pattern selector.
    pub pattern: Option<usize>,
    /// The corridor selector.
    pub corridor: Option<String>,
    /// The service date.
    pub date: Option<String>,
    /// The start of a diagram window.
    pub from: Option<String>,
    /// The end of a diagram window.
    pub until: Option<String>,
    /// The artifact path.
    pub out: Option<String>,
    /// The artifact format.
    pub format: Format,
    /// Where to write the manifest.
    pub manifest: Option<PathBuf>,
    /// The configuration file.
    pub config: Option<PathBuf>,
    /// An interface language override.
    pub language: Option<String>,
    /// A frequency policy override.
    pub frequency_policy: Option<String>,
    /// Whether to read and validate the feed strictly.
    pub strict: bool,
    /// Whether a warning should fail the run.
    pub warnings_as_errors: bool,
    /// Whether to suppress progress output.
    pub quiet: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            command: Command::Timetable,
            feed: None,
            datamall: false,
            cache_dir: PathBuf::from("cache"),
            allow_stale: false,
            account_key_env: mrt_datamall::ACCOUNT_KEY_ENV.to_string(),
            station: None,
            line: None,
            pattern: None,
            corridor: None,
            date: None,
            from: None,
            until: None,
            out: None,
            format: Format::Html,
            manifest: None,
            config: None,
            language: None,
            frequency_policy: None,
            strict: false,
            warnings_as_errors: false,
            quiet: false,
        }
    }
}

/// What `parse` decided to do.
#[derive(Debug)]
pub enum Parsed {
    /// Run a command.
    Run(Box<Args>),
    /// Print the usage text and exit successfully.
    Help,
    /// Print the version and exit successfully.
    Version,
}

/// Parse the arguments after the program name.
pub fn parse<I, S>(arguments: I) -> Result<Parsed, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let raw: Vec<String> = arguments.into_iter().map(Into::into).collect();
    if raw.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Parsed::Help);
    }
    if raw.iter().any(|a| a == "-V" || a == "--version") {
        return Ok(Parsed::Version);
    }
    let Some(first) = raw.first() else {
        return Ok(Parsed::Help);
    };
    let Some(command) = Command::parse(first) else {
        return Err(CliError::usage(format!(
            "\"{first}\" is not a command; run with --help for the list"
        )));
    };

    let mut args = Args {
        command,
        ..Args::default()
    };
    let mut flags: BTreeMap<String, String> = BTreeMap::new();
    let mut index = 1usize;
    while index < raw.len() {
        let item = &raw[index];
        let Some(name) = item.strip_prefix("--") else {
            return Err(CliError::usage(format!(
                "\"{item}\" is not an option; options start with --"
            )));
        };
        let (name, inline) = match name.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (name, None),
        };
        let name = name.to_string();
        if BOOLEAN_FLAGS.contains(&name.as_str()) {
            if let Some(value) = inline {
                return Err(CliError::usage(format!(
                    "--{name} takes no value, but got \"{value}\""
                )));
            }
            flags.insert(name, "true".to_string());
            index += 1;
            continue;
        }
        if !VALUE_FLAGS.contains(&name.as_str()) {
            return Err(CliError::usage(format!(
                "--{name} is not a known option; run with --help for the list"
            )));
        }
        let value = match inline {
            Some(value) => value,
            None => {
                index += 1;
                raw.get(index)
                    .cloned()
                    .ok_or_else(|| CliError::usage(format!("--{name} needs a value")))?
            }
        };
        if flags.insert(name.clone(), value).is_some() {
            return Err(CliError::usage(format!("--{name} appears twice")));
        }
        index += 1;
    }

    for (name, value) in flags {
        match name.as_str() {
            "feed" => args.feed = Some(PathBuf::from(value)),
            "source" => match value.as_str() {
                "datamall" => args.datamall = true,
                "local" => args.datamall = false,
                other => {
                    return Err(CliError::usage(format!(
                        "--source {other} is not supported; use datamall or local"
                    )))
                }
            },
            "cache-dir" => args.cache_dir = PathBuf::from(value),
            "allow-stale" => args.allow_stale = true,
            "account-key-env" => args.account_key_env = value,
            "station" => args.station = Some(value),
            "line" => args.line = Some(value),
            "pattern" => {
                args.pattern = Some(value.parse().map_err(|_| {
                    CliError::usage(format!("--pattern {value} is not a whole number"))
                })?)
            }
            "corridor" => args.corridor = Some(value),
            "date" => args.date = Some(value),
            "from" => args.from = Some(value),
            "until" => args.until = Some(value),
            "out" => args.out = Some(value),
            "format" => args.format = Format::parse(&value)?,
            "manifest" => args.manifest = Some(PathBuf::from(value)),
            "config" => args.config = Some(PathBuf::from(value)),
            "language" => args.language = Some(value),
            "frequency-policy" => args.frequency_policy = Some(value),
            "strict" => args.strict = true,
            "warnings-as-errors" => args.warnings_as_errors = true,
            "quiet" => args.quiet = true,
            other => {
                return Err(CliError::usage(format!("--{other} is not a known option")));
            }
        }
    }

    check(&args)?;
    Ok(Parsed::Run(Box::new(args)))
}

const BOOLEAN_FLAGS: [&str; 4] = ["allow-stale", "strict", "warnings-as-errors", "quiet"];

const VALUE_FLAGS: [&str; 17] = [
    "feed",
    "source",
    "cache-dir",
    "account-key-env",
    "station",
    "line",
    "pattern",
    "corridor",
    "date",
    "from",
    "until",
    "out",
    "format",
    "manifest",
    "config",
    "language",
    "frequency-policy",
];

/// Reject a combination that cannot work, before any file is read.
fn check(args: &Args) -> Result<(), CliError> {
    if args.command != Command::Fetch && args.feed.is_none() && !args.datamall {
        return Err(CliError::usage(
            "name a feed with --feed <PATH>, or use --source datamall".to_string(),
        ));
    }
    if args.feed.is_some() && args.datamall {
        return Err(CliError::usage(
            "--feed and --source datamall name two different feeds; choose one",
        ));
    }
    match args.command {
        Command::Timetable => {
            if args.station.is_none() {
                return Err(CliError::usage("timetable needs --station <CODE>"));
            }
            if args.date.is_none() {
                return Err(CliError::usage("timetable needs --date <YYYY-MM-DD>"));
            }
            if args.format == Format::Svg {
                return Err(CliError::usage(
                    "a timetable has no SVG form; use --format html or --format json",
                ));
            }
        }
        Command::Diagram => {
            let targets = [
                args.line.is_some(),
                args.pattern.is_some(),
                args.corridor.is_some(),
            ]
            .iter()
            .filter(|chosen| **chosen)
            .count();
            if targets == 0 {
                return Err(CliError::usage(
                    "diagram needs one of --line, --pattern, or --corridor",
                ));
            }
            if targets > 1 {
                return Err(CliError::usage(
                    "diagram takes only one of --line, --pattern, or --corridor",
                ));
            }
            if args.date.is_none() {
                return Err(CliError::usage("diagram needs --date <YYYY-MM-DD>"));
            }
        }
        Command::Fetch => {
            if args.feed.is_some() {
                return Err(CliError::usage(
                    "fetch downloads the feed; it takes no --feed",
                ));
            }
        }
        Command::Validate | Command::Stations => {}
    }
    Ok(())
}

/// Parse a date in `YYYY-MM-DD` or `YYYYMMDD` form.
pub fn parse_date(value: &str) -> Result<mrt_gtfs::ServiceDate, CliError> {
    let compact: String = value.chars().filter(|c| *c != '-').collect();
    compact.parse().map_err(|_| {
        CliError::usage(format!(
            "--date {value} is not a date; use YYYY-MM-DD or YYYYMMDD"
        ))
    })
}

/// Parse a time in `HH:MM:SS` or `HH:MM` form, allowing hours past 24.
pub fn parse_time(flag: &str, value: &str) -> Result<mrt_gtfs::GtfsTime, CliError> {
    let padded = if value.matches(':').count() == 1 {
        format!("{value}:00")
    } else {
        value.to_string()
    };
    padded.parse().map_err(|_| {
        CliError::usage(format!(
            "--{flag} {value} is not a time; use HH:MM or HH:MM:SS, and hours may pass 24"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arguments: &[&str]) -> Result<Args, CliError> {
        match parse(arguments.iter().map(|s| s.to_string()))? {
            Parsed::Run(args) => Ok(*args),
            other => Err(CliError::usage(format!(
                "expected a command, got {other:?}"
            ))),
        }
    }

    #[test]
    fn a_timetable_command_parses() {
        let args = run(&[
            "timetable",
            "--feed",
            "cache/current.zip",
            "--station",
            "NS1",
            "--date",
            "2026-08-10",
            "--out",
            "dist/ns1.html",
        ])
        .unwrap();
        assert_eq!(args.command, Command::Timetable);
        assert_eq!(args.feed.unwrap().to_str().unwrap(), "cache/current.zip");
        assert_eq!(args.station.as_deref(), Some("NS1"));
        assert_eq!(args.format, Format::Html);
    }

    #[test]
    fn inline_values_parse_too() {
        let args = run(&[
            "diagram",
            "--feed=feed.zip",
            "--line=EWL",
            "--date=20260810",
            "--format=json",
        ])
        .unwrap();
        assert_eq!(args.line.as_deref(), Some("EWL"));
        assert_eq!(args.format, Format::Json);
    }

    #[test]
    fn help_and_version_win_over_everything() {
        assert!(matches!(
            parse(["timetable", "--help"]).unwrap(),
            Parsed::Help
        ));
        assert!(matches!(parse(["-V"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(Vec::<String>::new()).unwrap(), Parsed::Help));
    }

    #[test]
    fn every_documented_option_is_accepted() {
        // The usage text and the flag tables must not drift apart: an
        // option that the help promises has to parse.
        for line in USAGE.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("--") else {
                continue;
            };
            let name = rest
                .split([' ', ',', '<'])
                .next()
                .unwrap_or_default()
                .trim_end_matches(',');
            if name.is_empty() || name == "help" || name == "version" {
                continue;
            }
            assert!(
                BOOLEAN_FLAGS.contains(&name) || VALUE_FLAGS.contains(&name),
                "the usage promises --{name}, but the parser does not know it"
            );
        }
    }

    #[test]
    fn policy_and_language_overrides_parse() {
        let args = run(&[
            "diagram",
            "--feed",
            "f",
            "--line",
            "BP",
            "--date",
            "20260810",
            "--frequency-policy",
            "reject-non-exact",
            "--language",
            "ja",
        ])
        .unwrap();
        assert_eq!(args.frequency_policy.as_deref(), Some("reject-non-exact"));
        assert_eq!(args.language.as_deref(), Some("ja"));
    }

    #[test]
    fn an_unknown_option_is_rejected() {
        let error = run(&["timetable", "--feed", "f", "--nope", "1"]).unwrap_err();
        assert!(error.message.contains("--nope"));
        assert_eq!(error.exit, crate::error::ExitCode::Usage);
    }

    #[test]
    fn an_unknown_command_is_rejected() {
        let error = run(&["explode"]).unwrap_err();
        assert!(error.message.contains("not a command"));
    }

    #[test]
    fn a_missing_value_is_rejected() {
        let error = run(&["timetable", "--station"]).unwrap_err();
        assert!(error.message.contains("needs a value"));
    }

    #[test]
    fn a_repeated_option_is_rejected() {
        let error = run(&["timetable", "--feed", "a", "--feed", "b"]).unwrap_err();
        assert!(error.message.contains("twice"));
    }

    #[test]
    fn a_boolean_flag_takes_no_value() {
        let error = run(&["timetable", "--feed", "f", "--strict=yes"]).unwrap_err();
        assert!(error.message.contains("takes no value"));
    }

    #[test]
    fn a_command_without_a_source_is_rejected() {
        let error = run(&["timetable", "--station", "NS1", "--date", "20260810"]).unwrap_err();
        assert!(error.message.contains("--feed"));
    }

    #[test]
    fn a_timetable_needs_a_station_and_a_date() {
        assert!(run(&["timetable", "--feed", "f", "--date", "20260810"])
            .unwrap_err()
            .message
            .contains("--station"));
        assert!(run(&["timetable", "--feed", "f", "--station", "NS1"])
            .unwrap_err()
            .message
            .contains("--date"));
    }

    #[test]
    fn a_timetable_has_no_svg_form() {
        let error = run(&[
            "timetable",
            "--feed",
            "f",
            "--station",
            "NS1",
            "--date",
            "20260810",
            "--format",
            "svg",
        ])
        .unwrap_err();
        assert!(error.message.contains("no SVG form"));
    }

    #[test]
    fn a_diagram_needs_exactly_one_target() {
        assert!(run(&["diagram", "--feed", "f", "--date", "20260810"])
            .unwrap_err()
            .message
            .contains("one of"));
        assert!(run(&[
            "diagram",
            "--feed",
            "f",
            "--date",
            "20260810",
            "--line",
            "EW",
            "--corridor",
            "c",
        ])
        .unwrap_err()
        .message
        .contains("only one"));
    }

    #[test]
    fn two_sources_at_once_are_rejected() {
        let error = run(&[
            "timetable",
            "--feed",
            "f",
            "--source",
            "datamall",
            "--station",
            "NS1",
            "--date",
            "20260810",
        ])
        .unwrap_err();
        assert!(error.message.contains("choose one"));
    }

    #[test]
    fn fetch_refuses_a_local_feed() {
        let error = run(&["fetch", "--feed", "f", "--out", "x.zip"]).unwrap_err();
        assert!(error.message.contains("no --feed"));
    }

    #[test]
    fn dates_accept_both_spellings() {
        assert_eq!(parse_date("2026-08-10").unwrap().to_string(), "20260810");
        assert_eq!(parse_date("20260810").unwrap().to_string(), "20260810");
        assert!(parse_date("10 August").is_err());
    }

    #[test]
    fn times_accept_hours_past_midnight() {
        assert_eq!(parse_time("from", "05:30").unwrap().to_string(), "05:30:00");
        assert_eq!(
            parse_time("until", "26:15:30").unwrap().to_string(),
            "26:15:30"
        );
        assert!(parse_time("from", "half past five").is_err());
    }
}
