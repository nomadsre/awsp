# Copy this file to github.com/nomadsre/homebrew-awsp as Formula/awsp-beta.rb.

class AwspBeta < Formula
  desc "Switch AWS SSO profiles across shell sessions"
  homepage "https://github.com/nomadsre/awsp"
  url "https://github.com/nomadsre/awsp/archive/refs/tags/v0.1.0-beta.6.tar.gz"
  sha256 "51a14ca9477db27c87c8e5aa24e525763d28ce0b8f9765e708e0b2d279e93761"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/nomadsre/awsp.git", branch: "main"

  depends_on "rust" => :build
  depends_on "awscli"
  depends_on "fzf"

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  def caveats
    <<~EOS
      Homebrew installed awsp but did not modify your shell startup files.

      Enable shell integration once:
        awsp setup zsh

      For bash:
        awsp setup bash

      Then restart the shell, or enable it immediately:
        source "$HOME/.config/awsp/shell/awsp.sh"
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/awsp --version")
    assert_match "awsp shell integration", shell_output("#{bin}/awsp init zsh")
  end
end
