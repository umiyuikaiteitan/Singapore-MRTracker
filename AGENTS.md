# Instructions for future agents

Before changing visual design, layout, typography, colour, imagery, route/station presentation, or UI copy, read `docs/STYLE_GUIDE.md`.

This guide applies recursively to the whole repository and its generated or hosted pages. Later explicit user instructions take precedence. Preserve application behaviour, data contracts, semantic route colours, operational notices, accessibility, and attribution while applying the shared aesthetic direction.

This documentation transfer does not itself restyle or deploy the application. Follow the repository's existing build, data, testing, and hosting documentation for implementation work. Report the checks actually performed.

The scope includes the board at `/`, `/fantasy-map/`, `/map/`, `/timetables/`, and every nested page, including Rust generators under `crates/`. Read `docs/FANTASY-MAP.md` before changing the pinned editor. Keep shared guidance outside `web/fantasy-map/`, which the snapshot updater replaces, and keep it aligned with OpenFantasyMap's copy.
