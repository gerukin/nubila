cask "nubila" do
  version "0.1.0"
  arch arm: "aarch64", intel: "x86_64"
  sha256 arm: "8350ed3f09d99002997f10e0b9e2592539141de74a26e2d890487f42fa7edcc4", intel: "691e4d45da8655ff90e422e0737479c9f02dd868ff3b8e316be8a3c9f132f37d"
  url "https://github.com/gerukin/nubila/releases/download/v#{version}/nubila-#{version}-#{arch}-apple-darwin.tar.gz"
  name "Nubila"
  desc "Fast weather TUI and structured CLI"
  homepage "https://github.com/gerukin/nubila"
  depends_on macos: :big_sur
  binary "nubila"
end
