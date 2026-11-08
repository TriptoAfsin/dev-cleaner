# Changelog

All notable changes to Dev Cleaner are documented here. The project follows
[Semantic Versioning](https://semver.org/): MAJOR for breaking changes, MINOR
for new features, PATCH for fixes. Release notes on GitHub are taken from the
section matching the tag.

## [Unreleased]

### Added
- Flatpak bundle for Linux. Inside the sandbox, tools run on the host through `flatpak-spawn --host`, and processes and ports are read from the host (`ps`, `ss`), so cleaning, ending processes and freeing ports keep working.
- After a clean, the dialog shows what was freed and what was left behind: files open in other programs are counted, a few are listed, and the programs holding them are named (Windows Restart Manager, /proc on Linux, lsof on macOS), with Clean again and a link to Processes. Files kept on purpose for being too new are counted separately.
- Clean progress: an overall bar (bytes deleted of the selected size), what is happening right now (running a tool, deleting files, checking what is left, finding which programs hold files), a bar and timer per item, and an indeterminate bar for tool commands that do not report progress. The result shows the total time, the time per item and the slowest one.
- Permissions per OS. Windows: Restart as administrator through UAC (from the status bar, System or Settings); the window reopens where it was, and the elevated copy waits for the old one to close. macOS: Full Disk Access status with a button to the right System Settings pane; admin cleanups ask for a password when they run, and the app never runs as root. Linux: admin cleanups go through pkexec; the status shows if it's missing.
- More languages in Disk cleanup: Gradle wrapper distributions and daemon logs (daemons are stopped first), Maven wrapper, Kotlin/Native, Android build cache, Groovy Grape, sbt, Clojure, NuGet HTTP and plugin caches, .NET workload cleanup, RubyGems, Bundler, rbenv/rvm downloads, SwiftPM, Cabal, Stack, GHCup, Hex, rebar3, Julia, renv, cpanm, opam, Nimble, Crystal, dub, Gleam, Elm, LuaRocks, pdm, Hatch, pipx, pyenv, Corepack, Terraform, Pulumi and Bazel.
- More languages in Projects: Maven, sbt and Leiningen target, Gradle build, .gradle and .kotlin, .NET bin/obj, Composer vendor, Bundler vendor/bundle, Swift .build, Dart/Flutter .dart_tool and build, Mix _build/deps, Stack and Cabal output, Zig cache/out, CMake build trees, tox/nox, and Unity, Godot and Unreal caches. Each matches only beside its project file.
- Available through Homebrew (`t21dev/tap/dev-cleaner`), Scoop (`t21dev/dev-cleaner`) and WinGet (`TriptoAfsin.DevCleaner`).

## [0.1.0] - 2026-11-09

First release, for Windows, macOS and Linux.

### Added
- **Overview**: live CPU, memory and system-disk gauges, reclaimable caches, Top CPU with End buttons, and recommendations.
- **Disk cleanup**: 50+ package-manager, tool, editor and AI/ML caches (npm, pnpm on every drive, Yarn, Bun, Deno, Cargo, pip, uv, Poetry, Conda, Go, Gradle, Maven, NuGet, Composer, Dart, Coursier, Zig, vcpkg, Conan, ccache, Docker, Cypress, Puppeteer, Playwright, Electron, JetBrains and VS Code/Cursor caches, Hugging Face datasets, torch.compile/Triton/vLLM, GPU shader caches and more). Each tool is asked where its cache really is, every row explains what it is, what it deletes and what runs, and the tool's own clean command is used where there is one.
- **System junk** on all three OSes: temp folders, Windows Update and Delivery Optimization caches, error reports, memory dumps, servicing logs, DISM component cleanup, thumbnail and Internet caches, browser and chat-app caches, unfinished downloads, Thumbs.db and Steam shader caches on every chosen drive, old installers, the Recycle Bin / Trash per drive; macOS app caches, logs, Xcode and simulator leftovers, Mail attachments and Time Machine snapshots; Linux thumbnails, journal, package caches, old Snap revisions and unused Flatpak runtimes. Admin items run in one elevated batch after a single prompt.
- **Projects**: find and delete node_modules, Python venvs, Rust target folders and framework caches in several folders or whole drives, named by their repository.
- **AI models**: Ollama, LM Studio, Hugging Face, ComfyUI and loose .gguf / .safetensors / .ckpt files on every chosen drive; nothing is pre-selected.
- **Memory**: breakdown including compressed memory, and Free memory with an exact per-OS explanation.
- **Processes**: grouped by app with a Safe / Save first / System verdict; protected processes can't be ended.
- **Ports**: which process holds each dev port, search, and free a port by ending its process.
- **System**: OS, CPU, memory compression, page file, drives, battery (laptops only), WSL, runtimes, editors and AI coding CLIs, with tooltips, a Markdown report and a separate Recommendations tab.
- **Configs**: back up editor and AI-harness configs to a zip with agent restore instructions, restore with automatic backup and undo, and copy settings, extensions, skills, commands, rules and MCP servers between tools.
- **Drives** setting to choose which drives cleanups include.
- Save or copy any page as a PNG, and a shareable card after each clean.
- Custom title bar with drag, snap and double-click maximize; position, size, maximized state and monitor are remembered, with a fallback when that monitor is gone.
- Light, dark and system themes with an accent colour picker; settings saved to a file.
- Windows installers and a portable zip, macOS Apple silicon and Intel disk images, Linux AppImage, .deb and .rpm.
