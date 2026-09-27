# Nubila

A **Rust-only** weather application with a **Ratatui + Crossterm** TUI and a
structured CLI. One native executable; no interpreter, subprocess wrappers or
runtime language dependencies. Built on the core components and practices of `tapp-ui`; see the
[framework integration notes](framework-adoption.md).

```sh
cargo build --release --locked
./target/release/nubila                  # TUI in a terminal; JSON when piped
./target/release/nubila tui --demo       # labeled fixtures, no network
./target/release/nubila config --init    # create a sample city config
```

Requires Rust 1.89+ to build. `Cargo.lock` pins dependencies. Install the
local development build through a symlink to `target/release/nubila`:

```sh
cargo run --locked --manifest-path tools/release/Cargo.toml -- refresh-local
nubila --version
```

`refresh-local` builds the native release executable, verifies its version,
and atomically updates `~/.local/bin/nubila`. Run it again after changing the
package version. Public installer and mise installations use their own copies.

The CLI, domain logic, HTTP client, persistence,
rendering, fixtures and tests are all Rust. Linux x86-64 is tested locally. Other
release targets require runtime validation; see [installation](installation.md).
Source builds require the private tapp-ui sibling.

## Configuration

Defaults to `$XDG_CONFIG_HOME/nubila/config.toml` (`~/.config/nubila/config.toml`).
Use `--config PATH` to override. If missing, Tokyo, Paris and London are loaded
without writing a file. See [config.example.toml](../config.example.toml).

```toml
pinned = "tokyo"
source = "auto"       # auto, gfs, icon, blend
auto_location = true
history_years = 10

[[cities]]
id = "tokyo"
name = "Tokyo"
latitude = 35.6762
longitude = 139.6503
```

Coordinates avoid startup geocoding. `nubila search Kyoto` returns candidate
coordinates, country and administrative region for disambiguation.

```sh
nubila config --set pinned=paris --set source=blend
nubila config --set auto_location=false --set history_years=30
nubila config --add-city '{"id":"kyoto","name":"京都","latitude":35.0116,"longitude":135.7681}'
nubila config --remove-city kyoto
```

Config edits validate before atomic replacement and serialize the known TOML
schema, removing comments. Unknown keys are rejected. Keep at least one city and
a valid pinned city or an empty/omitted `pinned`. City comparison requires a pinned city;
period comparison compares each city with itself and needs no pinned city.
Concurrent config changes reread and validate under a shared file lock. `--init`
refuses to overwrite an existing file.

Auto-location uses **ipapi.co** and your public IP. It may locate an ISP or VPN.
A favorite within 25 km absorbs the current marker instead of creating a duplicate.
Disable with `--no-location` or `auto_location = false`. Location failure is
reported and never prevents favorite-city weather.

## TUI

One transparent bottom line shows mode and sort on the left, Help on the right. Graph tabs are centered and
clickable. Press `a` to search for a city, `Enter` to search, then choose a result
with `↑`/`↓` and press `Enter` to save. Press `d` on a focused city and `Enter`
to confirm removal; `Esc` cancels. Changes save to the same TOML config used by
the CLI. Removing its pinned city assigns another saved city; at least one saved
city is required. Removing the automatic location disables auto-location.
Demo changes stay in memory (search Kyoto, Osaka, or New York).

| Key | Action |
| --- | --- |
| Arrows / j / k, PgUp / PgDn, Home / End | Navigate |
| Enter / Esc | Inspect city / close detail or help |
| l (details) | Go to city list (also available in the command palette) |
| n / c | Normal / choose comparison: pinned city by name, or another period |
| p | Period picker: Now, 1950–1969, Recent 5 years, 2040–2049 |
| ← / → on climate list | Annual → January → … → December; session only |
| r | Pin focused city (`p` stays the period picker) |
| s | Cycle forecast source |
| f, / or Ctrl-F; Ctrl-U | Open live city-filter modal / clear filter |
| 0–9 | Sort: city, temperature, feels-like, rain, snow, wind, humidity, cloud, Sun chooser, pressure |
| t / w | Sort by local time / weather conditions |
| Same number again | Metric cycle: current ASC → current DESC → minimum ASC → maximum DESC |
| o | Reverse the current sort directly |
| ← / → / t (detail) | Previous graph / next graph / current hour |
| u | Refresh from network |
| Ctrl-K / F2 | Contextual command palette |
| Ctrl-? / F1 / ? | Help (plain ? only outside inputs) |
| Ctrl-I | Contextual Info |
| q / Ctrl-Q / Ctrl-C | Quit (plain q only outside inputs) |

Click a row to focus; click it again to inspect. Mouse wheel scrolls. Modals
isolate underlying actions while preserving the visible background and its state. The filter supports Unicode grapheme editing,
bracketed paste, Left/Right, Home/End, Delete and Backspace. Enter applies the live
filter; Esc restores its previous state. Sorting preserves focus by stable city ID. Headers are clickable: a new
column starts ASC; metric headers repeat the four-step cycle above. City, condition
and time toggle ASC/DESC. Default is city name ASC, with
missing values last in either direction. The pinned city is pinned above the
list with a separator and also appears at its sorted position. Without a pinned,
the current city is pinned instead. A single city never repeats. Click the pinned
row to inspect it. JSON/CSV records are never duplicated by presentation pinning.

At 32 columns, the table shows city and temperature. At 55 it adds feels-like and
rain; at 80, wind and humidity; at 110, pressure and clouds. Each value has a
compact **min/max** line underneath, without widening the table. History uses
typical daily lows/highs for the selected month. The city header is simply
“City”; `*` means pinned, `@` current and `!` unavailable/stale.
The table starts with no focused row; arrows or a click focus one, and Esc clears it.
Selection uses a quiet neutral row background, preserving metric colors and muted
secondary values. Search results use the same treatment, with a rule below the input.
Forecast condition symbols have their own narrow, headerless column immediately
before temperature. City pinned/current indicators are muted. Historical
mode has no condition symbols. Details show a temperature chart, cloud/humidity
indicators, and separate hourly and next-day tables. Each table displays at most
**8 entries** (fewer in short terminals). Tab/Shift-Tab switch hourly/daily focus only while viewing tables;
`i` opens/closes information. Tab stays in place in information/historical views.
`[` / `]` cycle to the previous/next city in the current filtered sort order,
wrapping at either end. The active pane is preserved; city-specific scroll
positions reset, with hours starting at now. The pinned copy is not a separate city.
Graphs follow the focused table, numbered in display order. Hourly: **1 Temperature / 2 Feels / 3 Rain / 4 Snow / 5 Wind / 6 Cloud**. Daily and monthly: **1 Mean / 2 Min / 3 Max / 4 Feels min / 5 Feels max / 6 Rain / 7 Snow / 8 Wind / 9 Cloud / 0 Daylight**. Daily wind is the daily maximum, matching the table.
**Left/Right** cycle, and tabs are clickable. The native Ratatui `Tabs` widget
uses compact dividers and an underlined active metric, without a background fill.
Switching keeps
the time position and active pane; cycling cities retains the chosen metric.
Temperature, wind and daylight use lines; rain and snow use bars; cloud coverage uses a filled
area fixed to 0–100%. Lines change color at the same thresholds as numeric values,
including crossings between samples. Each rain bar takes its peak value's color
throughout its fill; cloud coverage remains neutral. Missing samples leave gaps.
Historical graphs use the same metric across the year,
with monthly rain/snow totals labeled mm/month and cm/month. Temperature graphs show hourly temperatures with hourly focus, daily mean/min/max with daily focus, and monthly averages for climate periods; daylight is hours per day. Below 34 rows, Left/Right cycle Tables and the graphs available for its focused table,
wrapping in either direction. The Tables tab restores the last focused table.
Tab stays within the tables view and does nothing on graphs. Graphs replace the lower content while focused,
and Up/Down, Page Up/Down, Home/End and the wheel move through the graph's hourly or daily range.
Keys 1–8 reveal the selected graph directly. Historical views cycle the
monthly table and eight yearly graphs. Tabs abbreviate and then show a window around the selected metric on narrow screens; clickable edge arrows reveal hidden tabs. Details use one right-aligned shortcut line.
Alt+Left/Right, Shift+mouse wheel or a horizontal mouse wheel fully reveal the entering column in the direction of travel. Columns wider than half the scrolling region move in smaller steps so their contents remain readable. In the main list, city names, their local times, and weather icons stay fixed. Column widths stay fixed; scrolling stops at the content edge and resizing clamps the offset. This behavior comes from tapp-ui's shared native-table viewport. Plain arrows retain graph/period cycling, Tab retains table focus, and an unmodified vertical wheel retains row navigation.

Tables follow temperature → rain → snow → wind → RH → cloud → solar information → pressure, omitting unavailable columns. Pressure is last. The main table keeps rain and snow separate. Its Sun column shows sunrise/set without arrows above muted daylight duration; click its header to select sunrise, sunset or daylight sorting in either direction. The same Sun column is available at every width. All columns remain reachable by sideways scrolling. Individual solar sort commands remain available in the command palette. The details summary uses muted sunrise/sunset arrows.
Arrows, Page Up/Down,
Home/End or the mouse wheel scroll the active pane. Up to 16 forecast days are
requested; all available future hours and days are accessible. Hourly detail opens
at the current hour, with two past days loaded initially. Up/down scroll the list
and `t` returns to now. Scrolling backward beyond the oldest loaded hour
requests more past data for that city in the background, up to the API's 92-day
limit. Unsupported/empty ranges retain existing data and stop further attempts;
refresh retries. Past hours are model estimates, not observed climate normals.
Use `--past-days 0..92` in CLI or TUI to choose the initial range. `i` opens all
measurements, daily ranges and source information. Historical details include
cloud coverage and a scrollable twelve-month profile. CLI output is not paginated.
Details/help wrap and scroll. Minimum supported size: **32×9**; quit remains
available below it.

Ratatui inherits the terminal foreground/background and uses its ANSI palette.
`--monochrome` or `NO_COLOR` removes color accents. The command palette offers
a shared theme toggle (Terminal or Tokyo Night Omarchy); `nubila theme --set
terminal` and `nubila theme --set tokyo-night-omarchy` expose it to the CLI. No specialized font is needed. Input and
network work run separately. A stable priority queue starts the focused/new city
first, followed by pinned/current cities and other requested cities. Three workers
can publish cached forecasts independently; climate uses one worker. Actual HTTP
is serialized to avoid provider concurrency limits, with a reused connection
pool. The event loop blocks on input, resize, signals, worker completion, or a
visible animation/clock deadline. Known cities appear
immediately with local time under each name; pending measurements stay blank.
A shared full-height Braille glyph animates at 8 FPS only while loading weather.
Each city appears as its request completes, without waiting for slower cities;
loading ends when the batch finishes. Comparison values update as the pinned
becomes available. Superseded request generations cannot replace current rows.
First-launch clocks use a bundled offline timezone map until provider timezone
data arrives.
Online views automatically refresh when their earliest response expires;
failed sources retry after 30 minutes, or at the specified API/quota retry deadline.
Offline/demo views never auto-fetch.
Pending reload requests coalesce, and stale
worker results cannot overwrite the latest mode. Quit does not wait for network
workers. RAII and panic hooks restore terminal state, mouse capture and paste.

Mode, source, pinned, filter, sort, monochrome and last primary screen persist at
`$XDG_STATE_HOME/nubila/view.json` (default `~/.local/state/...`). Override with
`--state PATH`; ignore saved choices with `--reset-state`. Explicit CLI flags
win over saved preferences. A saved city details screen opens immediately with its
name, local clock and loading placeholders; its weather loads before geolocation
and the remaining cities. Removed cities fall back to the list. Details show the
shared loading indicator while data is being fetched, retaining existing values
during refresh. Use `l` or Esc to return to the list. List selection, dialogs and
unfinished edits are not restored.
State is saved both when quitting with `q` and when closing the terminal window
(including a terminal hangup or input/output failure).
On Windows, a console-close handler requests the same shutdown and waits up to
four seconds for saving and cleanup before returning to Windows. Forced process
termination cannot run this handler.
The live filter is a durable preference. CLI weather commands never depend on
TUI state. Invalid or future preferences are preserved; concurrent state writes
do not overwrite a newer file. Use `config --set pinned=ID` to set the default for both interfaces.

## Weather semantics

- **Normal:** current model estimates and absolute hourly forecasts in °C, mm,
  km/h and hPa. These are model estimates, not station observations. Forecast
  timestamps remain UTC in JSON/CSV so cross-city and model-blend hours align.
  Human-facing hourly tables, detail timestamps and chart labels use the city's
  local timezone. The initial hourly window and `t` use the current clock time,
  even when cached weather has an older update timestamp (demo time is fixed).
- **Daily ranges:** extrema of all hourly samples on each city's **current local
  date**, including elapsed hours. Timezone rules handle 23/25-hour DST days.
  These are hourly-model extrema, not measured instantaneous records. Rain
  ranges are hourly liquid-rain totals (mm); current rain is the provider's
  current-interval total. Missing samples stay missing; JSON exposes sample counts.
- **Comparison:** signed current values minus the pinned, daily minimum minus
  pinned minimum, and daily maximum minus pinned maximum. Low/high deltas
  retain this order even when the low difference exceeds the high difference. Humidity/cloud
  differences are percentage points. JSON also preserves `absolute_values`.
  `absolute_ranges`, `day_hourly` and future hourly values remain absolute.
  Null values remain null. An unavailable
  pinned fails the request; mismatched current timestamps emit a warning.
- **Climate periods:** ERA5 1950–1969 or the last five completed years; 2040–2049 projections use MRI-AGCM3-2-S, EC-Earth3P-HR and MPI-ESM1-2-XR with equal model weights. All are provided by Open-Meteo. These are reanalysis/model averages, not official station normals or weather predictions.
  The list defaults to Annual; left/right selects a month. Details show all twelve months. Month selection is session-only and survives period/city navigation; period itself persists.
  Means use valid-day weights. Low/high are mean daily extrema, not records. Rain is mean annual or monthly total; hourly rain and its ranges are not fetched. Missing model fields remain unavailable.
- **Sources:** automatic Open-Meteo best-match, NOAA GFS seamless, DWD ICON
  seamless, or a GFS/ICON blend. These models share **one API service**;
  this is not independent-service failover.
- **Blend:** equal-weight numerical means at matching UTC hours. Missing values
  use remaining contributors. JSON reports contributor counts for each field.
  Model failures mark a degraded blend. If current timestamps differ, only the
  latest timestamp contributes, with an explicit warning. No categorical codes
  are averaged. Condition codes are categorical: the first available aligned
  model supplies the symbol (GFS, then ICON), with its source retained in JSON.
  Future daily summaries average the available model values for each date.
  Current-day ranges are calculated **after blending hourly curves**,
  not by averaging model extrema. This is not a calibrated probability forecast or a claim of
  improved accuracy.

Forecasts cache for 30 minutes, geolocation for one day, geocoding for 30 days
under `$XDG_CACHE_HOME/nubila/responses-rust-v1`. `--cache-dir PATH`
overrides. Writes are atomic. HTTP requests have a 12-second global timeout.
Each distinct request has only one cache file: a successful fetch replaces it.
Files for other cities, models, time ranges, searches and location lookups remain
reusable; they are not previous versions of the same response. Storage is bounded
to 512 files / 128 MiB, evicting oldest entries first.
On failure, stale cached responses remain available and are explicitly marked.
`--offline` makes no network requests. Manual refresh (`u` in the TUI or
`--refresh` in the CLI) bypasses fresh cache; automatic refresh honors expiry.
Launching the TUI with `--refresh` forces its initial load only; subsequent
automatic and mode-triggered loads reuse fresh cache.
A failed refresh retains the last successful response as a marked stale fallback.
Inspect `!` rows for errors or degraded/stale data.
`R` retries missing/stale data while retaining fresh caches; both `R` and `u`
work in the list and city details. Historical requests use one worker and
weighted background budgets (550/minute, 4,750/hour, 9,500/day), with deferred cities
automatically retried. Accounting and provider cooldowns survive restart in
`responses-rust-v1/limits/archive.json`; failed responses are never cached as data.
HTTP 429 errors retain the provider reason and honor `Retry-After`, falling back
to the next UTC minute/hour/day reset, with a short boundary safety margin.
Quota estimates use Open-Meteo's combined variable/date/model weight, with a
minimum of one credit per location. Confirmed DNS/connection failures release
their reservation; structured API errors retain one credit, and HTTP 429 retains
its cooldown without consuming the requested-data weight. Ambiguous timeouts,
gateway failures and invalid/truncated successful responses keep their full
reservation. Daily refunded request/credit totals are recorded in the budget
file for auditing. Legacy entries are retained because their outcomes are unknown.
The calculation follows upstream `ForecastApiResult.calculateQueryWeight` and
`Request.withFreeApiRateLimiter`; safety reserves remain independent of billing.

Current weather uses the reserved capacity up to 590/minute, 4,950/hour and
9,950/day. HTTP 429 reservations are refunded; model/location counts and forecast
date ranges contribute to request weight. Explicit refresh bypasses
data freshness, not quota cooldowns. Budgets cover this cache's Open-Meteo requests;
other apps on the same public IP can still exhaust the provider's allowance.

## CLI for agents and scripts

```sh
nubila weather --no-location                   # full JSON, default format
nubila weather --mode comparison --pinned tokyo --city paris
nubila weather --source blend --format table
nubila weather --period baseline --month 4 --format csv
nubila detail tokyo --source gfs                # complete hourly series
nubila detail tokyo --period recent           # all twelve months
nubila weather --offline --filter lon --reverse
nubila cities --no-location
nubila sources
nubila --help
```

Repeat `--city ID` for multiple cities. The pinned is fetched even outside the
requested subset. JSON (`schema_version: 1`) is the lossless format, containing
units, current/hourly/monthly values, `ranges` (min/max/sample counts), local range
date/timezone, sources, contributor counts, cache provenance and errors.
`range_units` identifies range units separately, notably hourly rain. CSV adds
`FIELD_min`, `FIELD_max`, `FIELD_samples`, `range_date` and `range_timezone`.
Human-readable table output accepts
`--width N`. Errors/diagnostics use stderr, preserving machine-readable stdout.

Exit codes: **0** success (including explicitly marked stale or degraded results),
**1** one or more city failures, **2** invalid input or whole-request failure,
**130** SIGINT/SIGTERM interruption. The TUI's q/Ctrl-C keys exit normally.

Existing TOML city configs remain compatible with the earlier prototype. Cached
HTTP responses use a new directory and are re-fetched by the Rust implementation.

## API terms and attribution

No API keys or paid endpoints. Open-Meteo's hosted free API is for
**non-commercial** use, with a 10,000-call daily limit; large historical requests
can count as multiple calls. Data is CC BY 4.0.

- [Forecast API and models](https://open-meteo.com/en/docs)
- [Historical API / ERA5](https://open-meteo.com/en/docs/historical-weather-api)
- [Service limits and terms](https://open-meteo.com/en/pricing)
- [ipapi.co geolocation](https://ipapi.co/api/)

Weather attribution: Open-Meteo, NOAA GFS, DWD ICON, ERA5 / Copernicus.
Geocoding: GeoNames through Open-Meteo.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

- `src/model.rs`: typed domain and comparison semantics.
- `src/config.rs`: validation and atomic persistence.
- `src/service.rs`: HTTP/cache, providers, blending and fixtures.
- `src/view.rs`: shared formatting and responsive columns.
- `src/tui.rs`: Ratatui rendering, state, input and terminal lifecycle.
- `src/cli.rs`: Clap commands over the shared service.

Rust tests cover numerical semantics, missing data, cache behavior, config
round-trips, CLI JSON, Unicode input, focus, modal isolation and Ratatui rendering
at tiny/narrow/normal/wide sizes. Native PTY tests cover navigation, resizing,
zero idle output, persistence, normal exit and SIGTERM restoration. Checks run
locally; no CI is configured. Linux results do not imply macOS or Windows has passed.

## Weather palette

All colors use the terminal ANSI palette, with the default background preserved.
Symbols and values remain readable without color; `--monochrome` disables hues.

| Symbols | Conditions | Color |
| --- | --- | --- |
| ☀ / ☾ / ⛅︎ | Sun / moon / partly cloudy | Yellow / cyan / yellow |
| ☁ / ≋ | Cloud / fog | Muted |
| ┊ / ☂ / ☔︎ | Drizzle / rain / showers (including freezing) | Light cyan, moderate blue, heavy bright blue |
| ❄ / ❅ | Snow (including showers) / snow grains | Light blue, moderate cyan, heavy bright cyan |
| ⚡︎ | Storm / storm with hail | Magenta / bright magenta |

One glyph per condition; intensity uses color. Full labels retain freezing,
hail, and intensity distinctions, including in monochrome mode.

| Metric | Color bands (lower bound included) |
| --- | --- |
| Temperature / feels-like, °C | <0 blue; 0–<10 cyan; 10–<25 neutral; 25–<30 yellow; 30–<35 red; ≥35 magenta |
| Current/hourly rain total, mm | 0 neutral; >0–<2.5 cyan; 2.5–<7.5 blue; ≥7.5 bright blue |
| Current/hourly snowfall depth, cm | 0 neutral; >0–<1 cyan; 1–<5 blue; ≥5 bright blue |
| Daily snowfall depth, cm | 0 neutral; >0–<5 cyan; 5–<15 blue; ≥15 bright blue |
| Daily / historical mean daily rain, mm | 0 neutral; >0–<10 cyan; 10–<30 blue; ≥30 bright blue |
| Wind, km/h | <20 neutral; 20–<40 yellow; 40–<60 red; ≥60 magenta |
| Humidity, cloud cover, pressure | Neutral at all values |

Primary measurements use full intensity; secondary values (min/max beneath a
current value and monthly low/high) retain their hue but
are dimmed. The daily forecast's low/high pair is its primary temperature value.
Missing values are muted. Current rain is the provider's current-interval total;
hourly rain and rain ranges are hourly totals. These are display bands, not alerts.
Comparison colors use absolute weather, while the text remains a delta. The same
roles apply to details, hourly/daily/monthly tables and the temperature chart.

Sorting and time-range options are also available without the TUI:

```sh
nubila weather --sort temperature --order desc
nubila detail tokyo --past-days 7
nubila config --set pinned=
```

### Local clocks and additional sorting

City details show current local time, with a timezone abbreviation when it fits.
The main table shows local time as muted secondary text below each city name.
Clocks follow each city's IANA timezone and daylight saving rules; unknown zones
show `—`. Visible live clocks update once per minute. Demo clocks use fixture time.

Each metric has one **Ctrl+K** sort command and one shortcut (`1` temperature,
`2` feels-like, `3` rain, `4` snow, `5` wind, `6` humidity, `7` cloud, `9` pressure).
Repeat to cycle current ASC → current DESC → minimum ASC → maximum DESC → current ASC.
`0` sorts cities, `w` conditions and `t` local time; these toggle ASC/DESC.
`8` opens the Sun chooser for sunrise, sunset or daylight in either direction.
`o` reverses the current sort directly. CLI exposes every range endpoint explicitly:

```sh
nubila weather --sort temperature-min
nubila weather --sort temperature-max --reverse
nubila weather --sort feels-min
nubila weather --sort rain-max --reverse
nubila weather --sort local-time --format table --width 160
```

Min/max sorting uses displayed daily ranges (deltas in Comparison, usual daily
extrema in Historical). Normal/Comparison and pinned changes reuse current data;
period changes reuse cached calendar chunks. Use `u` for an
explicit refresh. The first historical view still needs its historical data.
The live filter opens a compact Find-style modal with one input. The city list
behind it updates while typing. Enter commits the preview; Esc restores the previous
filter. Ctrl-U clears only the preview until Enter commits it. Quitting during a
preview saves the last committed filter. The status bar remains visible, and `f`
is ordinary text inside any input.

The main table uses the shared cyan header style and adaptive row separators
(24 table-area lines). Column lines are disabled; only the top outer border is
enabled. The header rule stays visible, and shorter windows omit row rules.
Separators are not selectable. The pinned city sits directly above the table.

### Climate cache and demand loading

`c` opens a comparison picker: the pinned city (shown by name), or another period.
Period comparisons always show the later period minus the older one, for each
city independently. `n` restores absolute values. `p` still chooses the period;
`r` pins a city. The list's Annual/month choice survives these changes.
CLI equivalent: `--period baseline --compare-period future --city tokyo`.
Use `--pinned CITY_ID` or config key `pinned` for the pinned city.

Sunrise, sunset and daylight are calculated locally from
[NOAA's solar equations](https://gml.noaa.gov/grad/solcalc/solareqns.PDF), with the
city's timezone/DST. No additional API requests or weather-cache invalidation.
They are approximate astronomical times, not terrain-adjusted observations.
Climate values average the period's calendar dates; polar days/nights have no
sunrise/sunset and 24h/0h daylight. Comparisons show signed minutes (positive
sunrise/sunset means later on the local clock). JSON/CSV use numeric minutes;
absolute clock values count from local midnight. Details show solar values in
the top panel; wider list, monthly and daily tables add solar columns.

When comparing Now with a climate period, Now is today's weather and climate
rain totals are converted to typical daily amounts. Now against an older climate
period uses that city's current calendar month. Future against Now retains the
chosen Annual/month interval (all months in details). No hourly historical rain
is invented. A missing comparison dataset stays unavailable and retries normally.

`--period now|baseline|recent|future` is independent of `--mode normal|comparison`.
`--month 0` means Annual (default); `1..12` selects a month for the list/CLI.
City details always load all months and summarize the year in their first panel.
Comparison details subtract corresponding pinned-city hours/days/months.
The JSON report retains absolute monthly values as `absolute_monthly`.

Climate data lives below `responses-rust-v1/climate-v1`, keyed by coordinates,
model, variables, aggregation version, calendar year and month. Completed
historical summaries never expire. Projection summaries expire after 180 days.
Raw daily responses are reduced to summaries and discarded; no decades of hourly
rain are downloaded. Cached months are reused across periods. When Recent rolls
forward, only the new missing year is fetched. Explicit `u`/`--refresh` can replace
cached summaries; offline mode never fetches. Incomplete temperature months are
not saved as complete. Normal cache pruning does not evict immutable climate data.

The TUI loads visible list rows and its pinned/pinned city. Details load only
the selected city and, in comparison, the pinned. Monthly list requests fetch
only that calendar month in each year; Annual/details fetch missing months in
contiguous runs within one year. Requests are sequential and paced using a shared
persistent budget. A static `⋯` marks a quota wait without replacing normal
status; Info and a toast show the local retry time. Toasts stack and allow
50 ms per character, with a three-second minimum. Active views retry automatically. Superseded loads stop before
issuing further HTTP requests. A request already in flight may finish and be cached.
Each successful month is saved immediately, even if a later request is deferred.
Automatic retries and restarts resume missing intervals; they do not restart the
whole city. A cold multi-model period can need several quota windows. Waiting
keeps the dashboard visible with a status/toast; Info opens only on request.

Rain and snow are separate throughout forecasts, climate periods, comparisons,
charts and structured exports. Rain is liquid depth in mm (forecast rain plus
showers); snow is snowfall depth in cm. Their depths must not be added together.
Total precipitation remains available separately in Info and JSON, in mm of
water equivalent. Snow columns appear when table width permits; the detail
summary and Snow graph also expose snowfall. Climate values are average monthly
or annual totals; no hourly historical rain or snow is downloaded.

Existing climate cache months retain their weather data. Missing rain/snow fields
are fetched separately and saved per month, so quota retries resume without
redownloading successful additions. Until the split is complete for a period,
its rain/snow values are unavailable rather than misleading partial averages.

Hourly and daily detail tables place feels-like temperature beside temperature.
Hourly rows show the predicted value; daily rows show its low/high range.
The Feels graphs follow table focus: hourly values, or separate daily minimum
and maximum graphs. Shortcuts follow the visible tab order. They use the temperature
color thresholds and zero reference line. Missing provider
values remain unavailable; daily feels-like mean/min/max are also exported in JSON.
