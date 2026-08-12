//! Errors from the publication projections.

/// An error that stops a projection from producing a document.
///
/// Everything that a projection *can* work around becomes a
/// `mrt_gtfs::Diagnostic` in the document metadata instead.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PublicationError {
    /// The configuration cannot work.
    #[error("the configuration is not usable: {0}")]
    Configuration(String),

    /// The requested station is not in the feed.
    #[error("{0}")]
    UnresolvedStation(String),

    /// The requested line is not in the feed.
    #[error("{0}")]
    UnresolvedLine(String),

    /// The requested corridor cannot be built.
    #[error("{0}")]
    UnresolvedCorridor(String),

    /// The schedule query failed, for example because the frequency
    /// policy rejects the service of the requested output.
    #[error(transparent)]
    Gtfs(#[from] mrt_gtfs::GtfsError),
}

impl PublicationError {
    /// Get the process exit code that the command line uses for this
    /// error.
    ///
    /// | Code | Meaning |
    /// |------|---------|
    /// | 2 | invalid command or configuration |
    /// | 4 | invalid GTFS feed |
    /// | 5 | unresolved station, line, pattern, or corridor |
    /// | 6 | the output cannot be represented under the policy |
    pub fn exit_code(&self) -> i32 {
        match self {
            PublicationError::Configuration(_) => 2,
            PublicationError::UnresolvedStation(_)
            | PublicationError::UnresolvedLine(_)
            | PublicationError::UnresolvedCorridor(_) => 5,
            PublicationError::Gtfs(mrt_gtfs::GtfsError::PolicyViolation(_)) => 6,
            PublicationError::Gtfs(_) => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_command_line_contract() {
        assert_eq!(PublicationError::Configuration("x".into()).exit_code(), 2);
        assert_eq!(
            PublicationError::UnresolvedStation("x".into()).exit_code(),
            5
        );
        assert_eq!(
            PublicationError::Gtfs(mrt_gtfs::GtfsError::PolicyViolation("x".into())).exit_code(),
            6
        );
        assert_eq!(
            PublicationError::Gtfs(mrt_gtfs::GtfsError::NoCalendar).exit_code(),
            4
        );
    }
}
