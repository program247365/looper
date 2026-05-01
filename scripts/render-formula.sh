#!/usr/bin/env bash
# Render the Homebrew formula for looper with prebuilt-binary URLs and SHA256s.
# Usage: render-formula.sh <version> <arm64-sha256> <x86_64-sha256>

set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <arm64-sha256> <x86_64-sha256>" >&2
  exit 2
fi

VERSION="$1"
ARM_SHA="$2"
X86_SHA="$3"

cat <<EOF
class Looper < Formula
  desc "CLI tool that plays a song on loop with a ratatui TUI and FFT visualizer"
  homepage "https://github.com/program247365/looper"
  version "${VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/program247365/looper/releases/download/v${VERSION}/looper-aarch64-apple-darwin.tar.gz"
      sha256 "${ARM_SHA}"
    end
    on_intel do
      url "https://github.com/program247365/looper/releases/download/v${VERSION}/looper-x86_64-apple-darwin.tar.gz"
      sha256 "${X86_SHA}"
    end
  end

  depends_on "ffmpeg"
  depends_on "yt-dlp"

  head do
    url "https://github.com/program247365/looper.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
    else
      bin.install "looper"
    end
  end

  test do
    assert_match "looper", shell_output("#{bin}/looper --help 2>&1")
  end
end
EOF
