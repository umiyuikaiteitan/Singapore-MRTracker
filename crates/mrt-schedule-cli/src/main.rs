//! The `mrt-schedule-cli` program.
//!
//! ```sh
//! # Generate a station timetable from a local feed.
//! mrt-schedule-cli timetable \
//!   --feed cache/current.zip \
//!   --station NS1 \
//!   --date 2026-08-10 \
//!   --config config/singapore.yaml \
//!   --out dist/ns1-2026-08-10.html
//! ```
//!
//! Run with `--help` for the full usage, and see `docs/CLI.md` for the
//! reference.

fn main() {
    std::process::exit(mrt_schedule_cli::run(std::env::args().skip(1)));
}
