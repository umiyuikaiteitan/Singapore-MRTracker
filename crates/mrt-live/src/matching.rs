//! Matching a GTFS-Realtime trip update to one run.
//!
//! # Which trip update belongs to which run
//!
//! A `trip_id` names a trip in the schedule, not one run of it. The
//! same `trip_id` runs again tomorrow, and a trip that a headway block
//! expands runs several times in one day. A GTFS-Realtime
//! `TripDescriptor` says which run it means with `start_date` and
//! `start_time`, and this module reads both: an update reaches a run
//! only where it can be shown to belong to it.
//!
//! | The update names | The run | The update |
//! | --- | --- | --- |
//! | a `start_date` equal to the run's service date | any | may apply |
//! | a `start_date` on another day | any | never applies |
//! | no readable `start_date` | any | applies on whichever of the two scanned days carries the trip — the documented fallback |
//! | a `start_time` equal to the run's start | comes from a headway block | may apply |
//! | a `start_time` at another minute | comes from a headway block | never applies |
//! | no readable `start_time` | comes from a headway block | applies to none of the sibling runs, and says so |
//! | any `start_time` | is a fixed trip | ignored: the `trip_id` and the service date already name one run |
//!
//! The ambiguous case — a headway trip and an update with no
//! `start_time` — is the one that has to invent something either way.
//! Applying the delay to every sibling states that four trains are all
//! four minutes late when the operator said that one of them is; the
//! matcher applies it to none of them instead and says so, with
//! [`UpdateMatch::ambiguous`]. It refuses to pick "the only sibling in
//! the window" either: which siblings are in the window is a property
//! of the query the caller asked for, not of the operator's statement.
//!
//! # One implementation, two views
//!
//! Both views of the realtime layer read these rules through
//! [`TripUpdateIndex::lookup`]: the live map ([`crate::map`]) matches an
//! update to a placed run, and the live destination board
//! ([`crate::LiveBoardBuilder`]) matches one to a departure of that run.
//! The two describe the same instant of the same network, so a delay,
//! a cancellation, or a skipped call must land on the same run in both.
//! There is one decision table and one implementation of it, so they
//! cannot drift apart.
//!
//! The two views name a run in different ways — the map from a
//! [`TripInstance`], the board from a [`mrt_gtfs::BoardEntry`] — so they
//! meet at [`RunKey`], which carries the three facts the table reads.

use std::collections::HashMap;

use mrt_gtfs::{Diagnostic, GtfsTime, ServiceDate, TripInstance};
use mrt_gtfs_rt::{RailRtFeed, TripUpdate};

/// The identity of one run, as far as matching is concerned.
///
/// A `trip_id` alone is not one: it names a trip in the schedule, and
/// the same trip runs on every service day it is active on, several
/// times over where a headway block expands it. The service date and
/// the start of the run complete the identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunKey<'a> {
    /// The GTFS `trip_id` that the run comes from.
    pub(crate) trip_id: &'a str,
    /// The service date that the run belongs to.
    pub(crate) service_date: ServiceDate,
    /// The start of the run, for a run that a headway block expanded.
    ///
    /// `None` for a fixed trip, which its `trip_id` and its service
    /// date already name.
    pub(crate) run_start: Option<GtfsTime>,
}

impl<'a> RunKey<'a> {
    /// Name one run by its trip, its service date, and — where a
    /// headway block expanded it — its start.
    pub(crate) fn new(
        trip_id: &'a str,
        service_date: ServiceDate,
        run_start: Option<GtfsTime>,
    ) -> Self {
        RunKey {
            trip_id,
            service_date,
            run_start,
        }
    }

    /// Name the run that one placed trip instance represents.
    pub(crate) fn for_instance(trip: &'a TripInstance) -> Self {
        RunKey::new(
            trip.source_trip_id.as_str(),
            trip.service_date,
            expanded_start(trip),
        )
    }
}

/// Get the start time that identifies one run of a headway block.
///
/// The result is `None` for a fixed trip, which its `trip_id` and its
/// service date already identify.
///
/// [`mrt_gtfs::RailNetwork::query_trip_instances`] writes the
/// `instance_id` of a run as `<date>:<trip_id>` for a fixed trip and as
/// `<date>:<trip_id>@<HH:MM:SS>` for a run that a headway block
/// expanded, where the time is the first call of that run. The suffix
/// therefore decides, and the `trip_id` test keeps a `trip_id` that
/// contains an `@` of its own from reading as a suffix.
fn expanded_start(trip: &TripInstance) -> Option<GtfsTime> {
    let (head, suffix) = trip.instance_id.rsplit_once('@')?;
    if !head.ends_with(trip.source_trip_id.as_str()) {
        return None;
    }
    suffix.parse().ok().or_else(|| trip.first_time())
}

/// One trip update of the feed with its identifying fields read once.
#[derive(Copy, Clone)]
struct TripMatch<'a> {
    /// The update itself.
    update: &'a TripUpdate,
    /// The service date the update names, where it names a readable
    /// one.
    start_date: Option<ServiceDate>,
    /// The start time the update names, where it names a readable one.
    start_time: Option<GtfsTime>,
}

/// The trip updates of one feed, indexed for matching.
///
/// The key is the `trip_id`, which names a trip and not a run of it;
/// [`TripUpdateIndex::lookup`] does the rest.
#[derive(Default)]
pub(crate) struct TripUpdateIndex<'a> {
    by_trip: HashMap<&'a str, Vec<TripMatch<'a>>>,
}

/// What the realtime feed says about one run.
#[derive(Copy, Clone, Default)]
pub(crate) struct UpdateMatch<'a> {
    /// The update that belongs to the run, where one does.
    pub(crate) update: Option<&'a TripUpdate>,
    /// `true` when the update names the service date of the run, and
    /// the start time of the run where it comes from a headway block.
    /// Such an update is about this run and no sibling of it, which is
    /// the bar a cancellation must clear to outlive a stale feed.
    pub(crate) targeted: bool,
    /// `true` when an update names the trip of the run and cannot be
    /// attached to one of the runs the headway block expands.
    pub(crate) ambiguous: bool,
}

impl<'a> TripUpdateIndex<'a> {
    /// Index the trip updates of one realtime feed by `trip_id`.
    ///
    /// The index is built once per view rather than searched once per
    /// run: the feed carries an update for every running trip and the
    /// query returns a run for every running trip, so a search per run
    /// was quadratic in the size of the network.
    ///
    /// A `trip_id` is not a key on its own — one `trip_id` covers
    /// yesterday's run and today's, and every run a headway block
    /// expands — so an entry holds the updates that name that trip and
    /// the identifying fields of each, read once here rather than once
    /// per run. A feed carries one update per running trip, so the
    /// entry is one element long and the per-run lookup stays a hash
    /// lookup and a filter over it. Where several updates fit one run,
    /// the first of them wins.
    ///
    /// A `start_date` or a `start_time` that does not parse is reported
    /// and then treated as absent, so an unreadable field falls back to
    /// the behaviour of a field the feed never sent rather than
    /// silently detaching the update from every run. A caller that
    /// publishes no diagnostics — the destination board — passes a
    /// scratch vector and drops it.
    pub(crate) fn from_feed(
        feed: Option<&'a RailRtFeed>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Self {
        let mut index = TripUpdateIndex::default();
        let Some(feed) = feed else {
            return index;
        };
        for update in &feed.trip_updates {
            let Some(trip_id) = update.trip_id.as_deref() else {
                continue;
            };
            let start_date = update.start_date.as_deref().and_then(|raw| {
                raw.parse::<ServiceDate>().ok().or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "realtime-unreadable-start-date",
                            format!(
                                "the trip update names the start date \"{raw}\", which is not a \
                                 GTFS date, so the update is read as naming no date at all"
                            ),
                        )
                        .about(trip_id.to_string()),
                    );
                    None
                })
            });
            let start_time = update.start_time.as_deref().and_then(|raw| {
                raw.parse::<GtfsTime>().ok().or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "realtime-unreadable-start-time",
                            format!(
                                "the trip update names the start time \"{raw}\", which is not a \
                                 GTFS time, so the update is read as naming no start time at all"
                            ),
                        )
                        .about(trip_id.to_string()),
                    );
                    None
                })
            });
            index.by_trip.entry(trip_id).or_default().push(TripMatch {
                update,
                start_date,
                start_time,
            });
        }
        index
    }

    /// Find the update that belongs to one run.
    ///
    /// The module documentation carries the rules as a table. The cost
    /// is one hash lookup and a scan of the updates that name that
    /// `trip_id`, which a feed carries one of.
    pub(crate) fn lookup(&self, run: RunKey<'_>) -> UpdateMatch<'a> {
        let Some(candidates) = self.by_trip.get(run.trip_id) else {
            return UpdateMatch::default();
        };
        let mut ambiguous = false;
        for candidate in candidates {
            // A dated update belongs to that service day alone. Both
            // views scan two of them, so this is what keeps an update
            // for yesterday's run off today's.
            if candidate
                .start_date
                .is_some_and(|date| date != run.service_date)
            {
                continue;
            }
            if let Some(start) = run.run_start {
                match candidate.start_time {
                    Some(time) if time == start => {}
                    // Another run of the same headway block.
                    Some(_) => continue,
                    // Some run of the headway block, and the feed does
                    // not say which.
                    None => {
                        ambiguous = true;
                        continue;
                    }
                }
            }
            return UpdateMatch {
                update: Some(candidate.update),
                targeted: candidate.start_date.is_some(),
                ambiguous: false,
            };
        }
        UpdateMatch {
            update: None,
            targeted: false,
            ambiguous,
        }
    }
}
