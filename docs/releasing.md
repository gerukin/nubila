# Local releases

The Rust helper builds/packages all six targets only when explicitly invoked.
Normal Cargo builds remain native-only. No CI or build hooks are used.

Set PATH to Rust, cargo-zigbuild, Zig, cargo-xwin, Clang/LLD and cargo-about;
set SDKROOT to the existing legally obtained macOS SDK. Preserve CARGO_HOME and
RUSTUP_HOME. Use CARGO_BUILD_JOBS=2 and the existing xwin cache when available.

1. Finalize version/changelog; run formatting, clippy, app and release-helper tests.
2. Commit app and private framework sources; both must be clean.
3. Run `cargo run --locked --manifest-path tools/release/Cargo.toml -- prepare`.
4. Review `dist/VERSION`: six archives, licenses, target/source manifests,
   SHA256SUMS, installer, release notes and generated formula/cask.
5. Validate archive hashes and isolated installation. Record actual runtime tests.
6. After approval, create `gerukin/nubila`, push the single initial source commit,
   upload a draft release targeting that commit, then publish when authorized.
7. Copy generated recipes to Formula/nubila.rb and Casks/nubila.rb and push a
   packaging metadata commit. Test actual public install URLs; do not rebuild or
   replace uploaded archives for metadata-only fixes.

`prepare` has no remote side effects. Keep SDKs, toolchains, logs and archives ignored.
Cross-built macOS/Windows/ARM binaries must be labeled untested unless exercised on
those systems. Preserve macOS ad-hoc signatures and Windows static CRT.

The macOS path uses the existing Clang plus Rust's bundled ld64.lld and the
macOS SDK, with an explicit platform version and ad-hoc signing. Inspect
`LC_BUILD_VERSION` after building: Zig 0.15.2 produced a macOS 13 minimum despite
MACOSX_DEPLOYMENT_TARGET=11.0. Windows ARM64 retains xwin's existing SDK and linker setup, but adapts its C
include flags from /imsvc to -isystem for ring's required GNU-style Clang driver.
