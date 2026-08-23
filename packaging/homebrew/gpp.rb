# Homebrew formula for gpp. Tap: `brew install mahabubul470/tap/gpp`.
# Update `version`/`url`/`sha256` per release (CI fills these in).
class Gpp < Formula
  desc "AI-native version control system"
  homepage "https://github.com/mahabubul470/gpp"
  url "https://github.com/mahabubul470/gpp/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "fd6b6ebd04ec8d920c98d88f142d29e99c1c06ea09f0cb650c0cad873ec064a0"
  license "MIT"
  head "https://github.com/mahabubul470/gpp.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "crates/gpp-cli"
    system "cargo", "install", "--locked", "--root", prefix, "--path", "crates/gpp-relay"
  end

  test do
    assert_match "gpp", shell_output("#{bin}/gpp --version")
  end
end
