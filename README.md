# Nubila

A fast weather dashboard and structured CLI. Find out whether it's raining
where you are, where you'd rather be, and whether moving would actually help.

- **All your cities. One terminal.** Local times, pinned favorites and sortable
  tables. Keep tabs on the world without collecting browser tabs about it.
- **Forecasts with receipts.** Hourly and daily tables, colored graphs,
  feels-like temperatures, rain, snow and wind. "Nice out" lacked precision.
- **Daylight included.** Sunrise, sunset and day length. Plan your brief
  appearance outside with the same rigor as a production deployment.
- **Compare your options.** Cities or periods: now, 1950–1969, the recent five
  years and 2040–2049 projections. Nostalgia can have a data column too.
- **Polite to the APIs.** Free sources, optional blending, persistent caches
  and quota-aware retries. Refreshing harder does not improve the weather.
- **Agents get data, humans get colors.** JSON, CSV and text output; query a city
  without adding it to your favorites. Your script doesn't need a relationship.
- **Your terminal still looks like your terminal.** Theme-aware colors, keyboard
  and mouse navigation. The forecast may be gloomy; the interface needn't be.

## Install

With mise (including Omarchy/Arch):

```sh
mise use -g github:gerukin/nubila@0.1.0
# Later updates: mise upgrade github:gerukin/nubila
```

mise may withhold `@latest` for 24 hours after a release. Use `@0.1.0` now,
then switch to `@latest` after that window.

Linux/macOS installer (repeat to update):

```sh
curl -fsSL https://github.com/gerukin/nubila/releases/latest/download/install.sh | sh
```

macOS Homebrew:

```sh
brew tap gerukin/nubila https://github.com/gerukin/nubila
brew install --cask gerukin/nubila/nubila
```

Linux Homebrew uses `brew install gerukin/nubila/nubila`. Windows users can extract
the matching release ZIP and add its directory to PATH. Six release targets are
built: Linux, macOS and Windows, each on x86-64 and ARM64. Linux binaries require
glibc 2.28+; macOS requires 11+. See [installation](docs/installation.md).

## Quick start

```sh
nubila                          # TUI in a terminal; JSON when piped
nubila tui --demo               # weather without consulting the sky
nubila search Kyoto             # find city coordinates
nubila weather --no-location    # structured weather output
nubila --help                   # a more reliable forecast of what the flags do
```

In the TUI, `?` opens help, `p` chooses a period, `c` chooses a comparison and `f`
filters cities. Open a city with Enter; arrow keys scroll and switch graph tabs.

See the [reference](docs/reference.md) for configuration, queries, caching and all
shortcuts; [changelog](CHANGELOG.md) for releases and [release guide](docs/releasing.md)
for maintainers. Source, issues and releases: https://github.com/gerukin/nubila.

## Source and licenses

Nubila is written in Rust with Ratatui. Its public app source depends on the private
sibling `tapp-ui` framework and its patched Crossterm; a standalone public checkout
cannot currently build without access to those dependencies. Release binaries
require no framework checkout or Rust installation.

Nubila is MIT licensed. Archives include dependency/framework notices and the ODbL
license and attribution for bundled timezone boundaries. Weather data comes from
Open-Meteo and its upstream providers; the free hosted service is for non-commercial
use and has quotas. Historical/projection data are estimates, not observed forecasts.

Only Linux x86-64 has been runtime-tested for this release preparation. Other target
builds are not evidence of runtime support. macOS binaries are not Developer ID
signed/notarized; Windows binaries are unsigned. Installation does not modify your
terminal, desktop shortcuts or preferences.

No umbrella included. That would complicate cross-compilation.
