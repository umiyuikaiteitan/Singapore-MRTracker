//! Select the rail subset of a GTFS feed.

use std::collections::HashSet;

use crate::feed::GtfsFeed;
use crate::model::Stop;

/// A filter that selects rail routes by their GTFS `route_type`.
///
/// The default filter accepts these standard route types:
///
/// | Value | Meaning              | Singapore example |
/// |-------|----------------------|-------------------|
/// | 0     | Tram or light rail   | LRT (some feeds)  |
/// | 1     | Subway or metro      | MRT               |
/// | 2     | Heavy rail           | —                 |
/// | 12    | Monorail             | LRT (some feeds)  |
///
/// The default filter also accepts the extended route types for rail:
/// 100 to 117 (railway), 400 to 405 (urban railway and monorail), and
/// 900 to 906 (tram).
///
/// # Examples
///
/// ```
/// use mrt_gtfs::RailFilter;
///
/// let filter = RailFilter::default();
/// assert!(filter.is_rail(1));   // MRT
/// assert!(!filter.is_rail(3));  // Bus
///
/// // A custom filter for metro routes only.
/// let metro_only = RailFilter::with_route_types([1]);
/// assert!(!metro_only.is_rail(12));
/// ```
#[derive(Debug, Clone)]
pub struct RailFilter {
    route_types: HashSet<u16>,
}

impl Default for RailFilter {
    fn default() -> Self {
        let mut route_types: HashSet<u16> = [0, 1, 2, 12].into();
        route_types.extend(100..=117); // Extended: railway services.
        route_types.extend(400..=405); // Extended: urban railway and monorail.
        route_types.extend(900..=906); // Extended: tram services.
        RailFilter { route_types }
    }
}

impl RailFilter {
    /// Make a filter that accepts only the given route types.
    pub fn with_route_types(route_types: impl IntoIterator<Item = u16>) -> Self {
        RailFilter {
            route_types: route_types.into_iter().collect(),
        }
    }

    /// Add one more route type to the filter.
    pub fn allow(mut self, route_type: u16) -> Self {
        self.route_types.insert(route_type);
        self
    }

    /// Report whether the filter accepts the route type.
    pub fn is_rail(&self, route_type: u16) -> bool {
        self.route_types.contains(&route_type)
    }

    /// Make a new feed that contains only the rail subset of `feed`.
    ///
    /// The function keeps:
    /// - the routes that the filter accepts,
    /// - the trips, stop times, frequencies, and shapes of those routes,
    /// - the stops of those trips, with their parent stations,
    /// - the calendar records of the services of those trips,
    /// - the transfers between kept stops,
    /// - the agencies of the kept routes.
    pub fn apply(&self, feed: &GtfsFeed) -> GtfsFeed {
        let kept_routes: HashSet<&str> = feed
            .routes
            .iter()
            .filter(|r| self.is_rail(r.route_type))
            .map(|r| r.route_id.as_str())
            .collect();

        let trips: Vec<_> = feed
            .trips
            .iter()
            .filter(|t| kept_routes.contains(t.route_id.as_str()))
            .cloned()
            .collect();
        let kept_trips: HashSet<String> = trips.iter().map(|t| t.trip_id.clone()).collect();
        let kept_services: HashSet<String> = trips.iter().map(|t| t.service_id.clone()).collect();

        let stop_times: Vec<_> = feed
            .stop_times
            .iter()
            .filter(|st| kept_trips.contains(st.trip_id.as_str()))
            .cloned()
            .collect();

        // Keep every referenced stop and the full parent chain.
        let mut kept_stops: HashSet<String> =
            stop_times.iter().map(|st| st.stop_id.clone()).collect();
        let stop_by_id: std::collections::HashMap<&str, &Stop> =
            feed.stops.iter().map(|s| (s.stop_id.as_str(), s)).collect();
        let mut frontier: Vec<String> = kept_stops.iter().cloned().collect();
        while let Some(stop_id) = frontier.pop() {
            let Some(stop) = stop_by_id.get(stop_id.as_str()) else {
                continue;
            };
            if let Some(parent) = stop.parent_station_id() {
                if kept_stops.insert(parent.to_string()) {
                    frontier.push(parent.to_string());
                }
            }
        }

        let kept_shapes: HashSet<String> =
            trips.iter().filter_map(|t| t.shape_id.clone()).collect();
        let kept_agencies: HashSet<&str> = feed
            .routes
            .iter()
            .filter(|r| kept_routes.contains(r.route_id.as_str()))
            .filter_map(|r| r.agency_id.as_deref())
            .collect();

        GtfsFeed {
            agencies: feed
                .agencies
                .iter()
                .filter(|a| match a.agency_id.as_deref() {
                    Some(id) => kept_agencies.contains(id),
                    // An agency without an identifier stays. Feeds with
                    // one operator often omit the identifier.
                    None => true,
                })
                .cloned()
                .collect(),
            stops: feed
                .stops
                .iter()
                .filter(|s| kept_stops.contains(&s.stop_id))
                .cloned()
                .collect(),
            routes: feed
                .routes
                .iter()
                .filter(|r| kept_routes.contains(r.route_id.as_str()))
                .cloned()
                .collect(),
            trips,
            stop_times,
            calendar: feed
                .calendar
                .iter()
                .filter(|c| kept_services.contains(c.service_id.as_str()))
                .cloned()
                .collect(),
            calendar_dates: feed
                .calendar_dates
                .iter()
                .filter(|c| kept_services.contains(c.service_id.as_str()))
                .cloned()
                .collect(),
            frequencies: feed
                .frequencies
                .iter()
                .filter(|f| kept_trips.contains(f.trip_id.as_str()))
                .cloned()
                .collect(),
            transfers: feed
                .transfers
                .iter()
                .filter(|t| {
                    kept_stops.contains(&t.from_stop_id) && kept_stops.contains(&t.to_stop_id)
                })
                .cloned()
                .collect(),
            shapes: feed
                .shapes
                .iter()
                .filter(|s| kept_shapes.contains(s.shape_id.as_str()))
                .cloned()
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_accepts_rail_types() {
        let filter = RailFilter::default();
        for rail in [0, 1, 2, 12, 100, 109, 401, 405, 900] {
            assert!(filter.is_rail(rail), "route type {rail} must pass");
        }
        for other in [3, 4, 5, 6, 7, 11, 200, 700, 1000] {
            assert!(!filter.is_rail(other), "route type {other} must not pass");
        }
    }

    #[test]
    fn custom_filter_overrides_the_default() {
        let filter = RailFilter::with_route_types([1]).allow(12);
        assert!(filter.is_rail(1));
        assert!(filter.is_rail(12));
        assert!(!filter.is_rail(2));
    }
}
