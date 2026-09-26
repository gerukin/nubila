# Nubila and tapp-ui

Nubila is a Rust/Ratatui application using the canonical sibling `../tapp-ui`
checkout. It depends on core components with terminal-palette support; document
renderers and the editor are not linked. The workspace Cargo patch uses the shared
vendored Crossterm implementation.

The framework owns terminal lifecycle, commands, input, transactional filtering,
help/info dialogs, themes, notifications, table geometry and styling, graph
rendering, tabs, background-work coordination, and storage primitives. Nubila owns
weather semantics, providers, configuration, CLI output and clipboard transport.

The main table uses cyan headers, adaptive row rules, a top border, no column
rules, and the shared faint row selection. Local time is secondary text under city
names. Known cities appear immediately; weather arrives per city. The shared
full-height loading indicator animates at 8 FPS only while data is loading.

Normal/comparison views reuse forecast data. Network work stays off the event
thread, with at most eight weather requests in flight, coalesced reloads and
obsolete-result rejection. See [fetching review](fetch-review.md) for measurements
and remaining limits. Charts and visible-row preparation reuse cached geometry.

Configuration, durable view preferences, and response caches are separate. Config
updates are locked and atomic; view-state writes protect concurrent edits and
preserve invalid/future files. Filtering commits on Enter and rolls back on Esc.
Temporary focus and dialogs are not persisted.

Run `cargo test --locked`, `cargo clippy --locked --all-targets -- -D warnings`,
and `cargo fmt --package nubila --check` for validation. `cargo run --example review`
provides narrow/wide TestBackend captures; `cargo run --release --example measure`
and `cargo run --release --example measure_cache` provide local measurements.
Linux PTY tests cover terminal cleanup, resize, signals and idle behavior. These
checks do not establish physical font, multiplexer or other-platform equivalence.
