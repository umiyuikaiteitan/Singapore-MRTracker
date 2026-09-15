# Singapore MRT station coverage

`network-inventory.json` is the curated station authority for this hardware
revision. It contains **146 distinct MRT stations**, represented by **174 current
MRT station codes**, plus the two legacy Circle Line aliases `CE1` and `CE2`.
Each interchange appears once, so a one-pixel-per-station carrier requires 146
station LEDs. The inventory was reviewed on **15 September 2026** against the
[LTA July 2026 system map](https://www.lta.gov.sg/content/dam/ltagov/getting_around/public_transport/rail_network/pdf/SM_EN_%28Ver210726%29_CCL6.pdf).
The source PDF SHA-256 is recorded in the JSON for reproducibility.

| Line | Operating station codes | Stations on line |
| --- | --- | ---: |
| North-South (NSL) | NS1-NS5, NS7-NS28 | 27 |
| East-West (EWL), including airport branch | EW1-EW33, CG1-CG2 | 35 |
| North East (NEL) | NE1, NE3-NE18 | 17 |
| Circle (CCL), including Dhoby Ghaut branch | CC1-CC17, CC19-CC34 | 33 |
| Downtown (DTL) | DT1-DT35 | 35 |
| Thomson-East Coast (TEL) | TE1-TE9, TE11-TE20, TE22-TE29 | 27 |
| Total line memberships | Interchanges count on every served line | 174 |
| Unique physical station records | 25 interchanges, including 3 serving three lines | **146** |

The inventory includes Hume (DT4), opened on
[28 February 2025](https://www.lta.gov.sg/content/ltagov/en/newsroom/2025/1/news-releases/hume_station_to_open.html),
and Punggol Coast (NE18), opened on
[10 December 2024](https://www.lta.gov.sg/content/ltagov/en/newsroom/2024/12/news-releases/punggol_coast_station_welcomes_commuters.html).

## Circle Line changes

Keppel (CC30), Cantonment (CC31), and Prince Edward Road (CC32) are included.
LTA announced passenger service from
[12 July 2026](https://www.lta.gov.sg/content/ltagov/en/newsroom/2026/5/news-releases/circle-line-stage-6-to-open-for-public-preview-on-4-july-2026.html).
The July 2026 system map uses CC33 for Marina Bay and CC34 for Bayfront.
Older feed identifiers remain accepted as aliases; they never allocate another
LED. The older codes are visible in LTA's
[January 2025 map](https://www.lta.gov.sg/content/dam/ltagov/news/press/2025/250124_DTL_system_map_AnnexA.pdf).

| Physical station | Current MRT codes | Accepted legacy code |
| --- | --- | --- |
| Marina Bay | NS27, CC33, TE20 | CE2 |
| Bayfront | CC34, DT16 | CE1 |

## Exclusions and the date boundary

The July 2026 map explicitly marks Bedok South (TE30), Sungei Bedok
(TE31/DT37), and Xilin (DT36) as under construction. LTA's
[TEL project page](https://www.lta.gov.sg/content/ltagov/en/upcoming_projects/rail_expansion/thomson_east_coast_line.html)
and [DTL extension page](https://www.lta.gov.sg/content/ltagov/en/upcoming_projects/rail_expansion/downtown_line_2_and_3_extensions.html)
still describe their openings in 2026 without a precise passenger opening date.
Its [integration works announcement](https://www.lta.gov.sg/content/ltagov/en/newsroom/2026/4/news-releases/train-service-adjustments-tel-and-dtl-to-facilitate-rail-expansion-works.html)
describes testing ahead of opening, with DTL service adjustments through
5 September 2026. No primary-source opening date was found during this review.
These three physical stations remain explicitly excluded pending confirmation.
Thus `as_of` records the review date; it does not assert continuous verification
of every service change after the cited map revision.

Also excluded: unopened Mount Pleasant (TE10), Marina South (TE21), Founders'
Memorial (TE22A), Bukit Brown (CC18), reserved NE2, and future NS6. Future rail
projects and other future infill stations are outside this revision. The JSON's
`excluded_stations` list records the most easily mistaken station codes; it is
not an inventory of every planned future project.

LRT is outside the requested MRT network: Bukit Panjang, Sengkang and Punggol
LRT-only stations have no assigned LED. Their MRT interchanges still have one
LED each, with MRT codes only. The Sentosa Express and RTS Link are also outside
this inventory.

## Identity, adjacency and hardware revisions

`station` is the stable canonical identity for a physical LED. `codes` holds all
accepted MRT identities for that location; `lines` holds each served line once.
Stations are ordered by first encounter in NSL, EWL, NEL, CCL, DTL, TEL code
order. Do not sort this array after fabricating a carrier: its order is consumed
by hardware generation to allocate pixel indices. Append future stations or
issue a coordinated hardware/map revision, rather than silently reassigning
existing indices.

`line_paths` describes schematic adjacency. The EWL airport branch starts at
Tanah Merah; it is not connected to Tuas Link. The CCL loop returns from Bayfront
to Promenade, with Dhoby Ghaut on its branch. These paths are not claims about
current train routing or geographic coordinates. Interchanges such as Newton
and Tampines follow LTA's named grouping even where transfers require leaving
the paid area. Separate nearby stations retain separate LEDs.

Validation performed for this revision: unique station IDs and names; 176
globally unique accepted codes; canonical IDs present among their aliases;
all six line counts; every adjacency code mapped to the corresponding line;
Marina Bay/Bayfront legacy alias equivalence; major interchange equivalence;
and absence of every explicitly excluded code. Every physical station remains
in the carrier inventory even when a loaded data feed has no timetable or
prediction for it. Missing service data must not remove a station LED or imply
a real-time observation.
