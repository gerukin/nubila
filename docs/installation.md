# Installation

Release 0.1.0 is available from GitHub Releases.
See the README for mise, Homebrew, curl and Windows routes.

mise may withhold `@latest` for 24 hours after publication. Use `@0.1.0` initially,
then switch to `@latest`; do not disable the release-age protection.

The Unix installer verifies archive SHA-256, installs to `~/.local/bin`, and keeps
notices in `~/.local/share/doc/nubila`. Set `NUBILA_INSTALL_DIR` to override the
binary directory. Repeat the command to update. Remove the executable to uninstall;
config/state/cache remain until you remove them deliberately. No sudo or shell-profile
changes are made. Windows ZIPs include all notices; close Nubila before replacing it.

macOS uses a binary cask, retaining notices in its Caskroom. Linux Homebrew uses a
binary formula. Both recipes live in the same repository. Windows uses static CRT;
no extra Visual C++ runtime installation is required. Downloads are unsigned (macOS
has an ad-hoc linker signature); OS security policies may block them. Do not bypass
quarantine or application-control protections to install.

Omarchy floating-window registration and `gerukin.nubila` shortcuts are optional,
separate desktop configuration, not performed by the CLI installer.

For source builds, obtain the private `tapp-ui` sibling at the commit in RELEASE.txt,
then `cargo build --release --locked`. Rust target/toolchain requirements are
separate from binary installation. Public source alone is insufficient today.
