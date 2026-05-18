# Homebrew formula for gpp. Tap: `brew install gpp-vcs/tap/gpp`.
# Update `version`/`url`/`sha256` per release (CI fills these in).
class Gpp < Formula
  desc "AI-native version control system"
  homepage "https://github.com/gpp-vcs/gpp"
  url "https://github.com/gpp-vcs/gpp/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "MIT"
  head "https://github.com/gpp-vcs/gpp.git", branch: "main"

  depends_on "rust" => :build
  depends_on "cmake" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "crates/gpp-cli"
    system "cargo", "install", "--locked", "--root", prefix, "--path", "crates/gpp-relay"
  end

  test do
    assert_match "gpp", shell_output("#{bin}/gpp --version")
  end
end
