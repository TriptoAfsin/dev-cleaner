cask "dev-cleaner" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.0"
  sha256 arm:   "REPLACE_WITH_SHA256_OF_AARCH64_DMG",
         intel: "REPLACE_WITH_SHA256_OF_X64_DMG"

  url "https://github.com/TriptoAfsin/dev-cleaner/releases/download/v#{version}/Dev.Cleaner_#{version}_#{arch}.dmg"
  name "Dev Cleaner"
  desc "Reclaim disk and memory from developer caches, build folders and ports"
  homepage "https://github.com/TriptoAfsin/dev-cleaner"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates false
  depends_on macos: ">= :big_sur"

  app "Dev Cleaner.app"

  # Not notarised yet. Without this, macOS reports the app as damaged.
  postflight do
    system_command "/usr/bin/xattr", args: ["-cr", "#{appdir}/Dev Cleaner.app"]
  end

  zap trash: [
    "~/Library/Application Support/dev.tripto.dev-cleaner",
    "~/Library/Caches/dev.tripto.dev-cleaner",
    "~/Library/Preferences/dev.tripto.dev-cleaner.plist",
    "~/Library/Saved Application State/dev.tripto.dev-cleaner.savedState",
    "~/Library/WebKit/dev.tripto.dev-cleaner",
  ]

  caveats <<~EOS
    Dev Cleaner is not notarised by Apple yet. This cask clears the quarantine
    flag after install so the app opens normally.
  EOS
end
