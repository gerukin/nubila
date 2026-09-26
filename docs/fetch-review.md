# Fetching review — 2026-09-21

For a small saved-city list, the current design has appropriate bounds and cache
reuse. This pass made three concrete improvements:

- Fetch GFS and ICON concurrently for a blend, rather than serially. Limit blend
  work to four concurrent cities, maintaining eight active weather requests.
- Transfer parsed cached JSON into the caller instead of deep-cloning it. Serialize
  newly fetched responses by reference, outside the cache-write lock.
- Consume the TUI launch `--refresh` flag once; subsequent automatic reloads no
  longer inherit forced fetching. Explicit `u` refresh still bypasses fresh cache.

Verified existing behavior: one shared ureq connection pool; 12-second request
limits; 32 MiB response bodies; eight weather requests maximum; latest pending
reload coalescing and generation checks; normal/comparison/reference projections
reuse loaded forecasts; 30-minute forecast freshness, expiry-driven auto refresh,
30-minute failure backoff, and no offline/demo auto-fetches. Disk responses are
replaced per exact request, with atomic writes and bounded total cache storage.

Local release measurement (`cargo run --release --example measure_cache`):
a synthetic 138,093-byte cached extended forecast, 200 reads per case, measured
747 us/read with the former additional JSON clone simulated, versus 520 us/read
with ownership transfer (~30% less time). This includes disk-cache reads and JSON
parsing on this Linux host; it is not a live API or end-to-end network benchmark.

Follow-up: the TUI now receives each completed city immediately through its bounded
message channel, retains pending placeholders, and projects comparison values as
the reference arrives. Final completion merges metadata and ends loading. Normal/
comparison presentation changes can reuse an in-flight batch. CLI output remains
one complete report. A city completion only invalidates the detail chart for that
city; pending detail views initialize their hour window when data first arrives.

Remaining limitations: uncached geolocation precedes the weather batch. Active HTTP requests are not
cancelled when superseded (queued reloads coalesce). Independent app processes or
duplicate configured coordinates can still issue duplicate simultaneous misses.
Cache eviction scans the bounded directory on each successful write. Concurrent
geolocation and per-request deduplication would be the next useful changes if cold
startup latency or much larger city lists become a priority; no provider batching
or claimed real-network speedup was introduced in this pass.
