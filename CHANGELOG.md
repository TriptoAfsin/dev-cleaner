# Changelog

All notable changes to Dev Cleaner are documented here. The project follows
[Semantic Versioning](https://semver.org/): MAJOR for breaking changes, MINOR
for new features, PATCH for fixes. Release notes on GitHub are taken from the
section matching the tag.

## [Unreleased]

## [0.1.0] - 2026-11-08

First release.

### Added
- **Overview**: live CPU, memory and system-disk gauges, reclaimable caches, Top CPU with End buttons, and recommendations.
- **Disk cleanup**: 50+ package-manager, tool, editor and AI/ML caches on Windows and macOS (npm, pnpm on every drive, Yarn, Bun, Cargo, pip, uv, Go, Gradle, Maven, NuGet, Docker, Hugging Face datasets, torch.compile/Triton, GPU shader caches and more). Each explains what it is, the risk, and the command that runs; the tool's own clean command is used where there is one.
- **Projects**: find and delete node_modules, Python venvs, Rust target folders and framework caches under any folder.
- **AI models**: list and remove Ollama, LM Studio, Hugging Face and loose .gguf / .safetensors / .ckpt model files.
- **Memory**: breakdown including compressed memory, and Free memory (trim idle working sets on Windows, purge on macOS).
- **Processes**: grouped by app with a Safe / Save first / System verdict; protected processes cannot be ended.
- **Ports**: which process holds each dev port, search, and free a port by ending its process.
- **System**: OS, CPU, memory compression, page file, drives, battery (laptops only), WSL, runtimes, editors and AI coding CLIs, with tooltips and a Markdown report.
- **Configs**: back up editor and AI-harness configs to a zip with agent restore instructions, restore with automatic backup and undo, and copy settings, skills, commands, rules and MCP servers between tools.
- Save or copy any page as a PNG, and a shareable card after each clean.
- Custom title bar with drag, snap, double-click maximize; window position, size, maximized state and monitor are remembered, with a fallback when that monitor is gone.
- Light, dark and system themes with an accent colour picker.
