class Nubila < Formula
  desc "Fast weather TUI and structured CLI"
  homepage "https://github.com/gerukin/nubila"
  version "0.1.0"
  license "MIT"

  on_linux do
    on_intel do
      url "https://github.com/gerukin/nubila/releases/download/v0.1.0/nubila-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a34bffe954263d3bf2e3245bfa47587ad5087a46a9e70cbf45ebc8a58571ced4"
    end
    on_arm do
      url "https://github.com/gerukin/nubila/releases/download/v0.1.0/nubila-0.1.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "93a2ee24bdc5e562575b1ff035cf3450d5f0d25cb70051b6b4651fefdb4c95ce"
    end
  end

  on_macos do
    on_intel do
      url "https://github.com/gerukin/nubila/releases/download/v0.1.0/nubila-0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "691e4d45da8655ff90e422e0737479c9f02dd868ff3b8e316be8a3c9f132f37d"
    end
    on_arm do
      url "https://github.com/gerukin/nubila/releases/download/v0.1.0/nubila-0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "8350ed3f09d99002997f10e0b9e2592539141de74a26e2d890487f42fa7edcc4"
    end
  end

  def install
    bin.install "nubila"
    doc.install "LICENSE", "LICENSE-tapp-ui", "THIRD_PARTY.html", "RELEASE.txt", "LICENSE-timezone-data", "TIMEZONE-DATA.txt"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/nubila --version")
  end
end
