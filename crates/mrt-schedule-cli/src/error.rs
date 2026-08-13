//! Errors and process exit codes.
//!
//! The exit codes are part of the command-line contract, so a script
//! can tell a bad option from a bad feed from a policy refusal.

/// The exit codes of `mrt-schedule-cli`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Everything worked.
    Success = 0,
    /// The command line or the configuration is not usable.
    Usage = 2,
    /// The feed could not be fetched or read.
    SourceFailure = 3,
    /// The feed is not a valid GTFS feed.
    InvalidFeed = 4,
    /// A station, line, pattern, or corridor could not be resolved.
    Unresolved = 5,
    /// The requested output cannot be represented under the selected
    /// policy.
    Unrepresentable = 6,
    /// Rendering or writing a file failed.
    OutputFailure = 7,
}

impl ExitCode {
    /// Get the numeric code.
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// A command-line failure with the exit code it should produce.
#[derive(Debug, Clone)]
pub struct CliError {
    /// The exit code.
    pub exit: ExitCode,
    /// The message for standard error.
    pub message: String,
}

impl CliError {
    /// Make an error.
    pub fn new(exit: ExitCode, message: impl Into<String>) -> Self {
        CliError {
            exit,
            message: message.into(),
        }
    }

    /// Make a usage error.
    pub fn usage(message: impl Into<String>) -> Self {
        CliError::new(ExitCode::Usage, message)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<mrt_publication::PublicationError> for CliError {
    fn from(error: mrt_publication::PublicationError) -> Self {
        let exit = match error.exit_code() {
            2 => ExitCode::Usage,
            4 => ExitCode::InvalidFeed,
            5 => ExitCode::Unresolved,
            6 => ExitCode::Unrepresentable,
            _ => ExitCode::OutputFailure,
        };
        CliError::new(exit, error.to_string())
    }
}

impl From<mrt_gtfs::GtfsError> for CliError {
    fn from(error: mrt_gtfs::GtfsError) -> Self {
        let exit = match &error {
            mrt_gtfs::GtfsError::PolicyViolation(_) => ExitCode::Unrepresentable,
            mrt_gtfs::GtfsError::Io { .. } => ExitCode::SourceFailure,
            _ => ExitCode::InvalidFeed,
        };
        CliError::new(exit, error.to_string())
    }
}

impl From<mrt_datamall::DataMallError> for CliError {
    fn from(error: mrt_datamall::DataMallError) -> Self {
        CliError::new(ExitCode::SourceFailure, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_match_the_documented_contract() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::Usage.code(), 2);
        assert_eq!(ExitCode::SourceFailure.code(), 3);
        assert_eq!(ExitCode::InvalidFeed.code(), 4);
        assert_eq!(ExitCode::Unresolved.code(), 5);
        assert_eq!(ExitCode::Unrepresentable.code(), 6);
        assert_eq!(ExitCode::OutputFailure.code(), 7);
    }

    #[test]
    fn publication_errors_keep_their_meaning() {
        let policy = mrt_publication::PublicationError::Gtfs(mrt_gtfs::GtfsError::PolicyViolation(
            "headway".into(),
        ));
        assert_eq!(CliError::from(policy).exit, ExitCode::Unrepresentable);

        let station =
            mrt_publication::PublicationError::UnresolvedStation("no such station".into());
        assert_eq!(CliError::from(station).exit, ExitCode::Unresolved);
    }

    #[test]
    fn a_broken_feed_is_not_a_usage_error() {
        assert_eq!(
            CliError::from(mrt_gtfs::GtfsError::NoCalendar).exit,
            ExitCode::InvalidFeed
        );
    }
}
