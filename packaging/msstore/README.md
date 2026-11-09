# Microsoft Store

Product `dev-cleaner` (Store ID `9N1VVF6949HW`), an MSIX app. The identity in
`AppxManifest.xml` comes from Partner Center > Product identity; don't change
it. `DisplayName` must be a name reserved for the product.

## Building

The release workflow builds the MSIX on every tag and keeps it as the
`msstore-msix` artifact of the run. Locally:

```powershell
npm run tauri build -- --no-bundle
powershell -ExecutionPolicy Bypass -File scripts/build-msix.ps1 -Version 0.5.0
```

The Store signs the package; upload it unsigned.

## Testing before submitting

From an elevated shell (trusting the test certificate needs admin):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-msix.ps1 -Version 0.5.0 -Sign
Add-AppxPackage dist-msix\Dev.Cleaner_0.5.0_x64.msix
```

Then check, in the installed app:

- About says "Microsoft Store".
- Cleaning a cache under `%LOCALAPPDATA%` (npm, pip, ...) really frees the space.
- An admin cleanup (System junk) shows one UAC prompt and works. Its script is
  written to `%TEMP%`; if the elevated PowerShell can't find it, that write
  was virtualized and needs another location.
- Killing a process and freeing a port.
- Settings > Definitions > Refresh.

Remove with `Get-AppxPackage TriptoAfsin.dev-cleaner | Remove-AppxPackage`.

## Submission

- **Pricing:** Free. **Category:** Developer tools.
- **Privacy policy URL:**
  `https://github.com/TriptoAfsin/dev-cleaner/blob/main/PRIVACY.md`
  (live once `PRIVACY.md` is pushed to main).
- **Restricted capability `runFullTrust`**, justification:
  > Dev Cleaner is a desktop app that finds and removes developer caches
  > (npm, pip, Cargo, Gradle, Docker and others), build folders, and stops
  > processes holding ports. It must read and delete files across the
  > user's profile and run each tool's own clean command, which needs full
  > trust.

### Notes for certification

> Dev Cleaner deletes only what the user selects, after showing each item's
> path and size. Nothing is deleted automatically. Optional admin cleanups
> (Windows temp, update caches) ask for UAC each time. The app downloads
> "definitions" from its public GitHub repository: JSON data listing cache
> locations, not code; it never downloads or runs executables. Update checks
> only read GitHub's latest release; Store installs are sent to the Store to
> update. No account, telemetry, or data collection.

### Listing

**Short description:** Reclaim disk and memory from developer caches, build
folders, processes and ports.

**Description:**

> Dev Cleaner finds the space developer tools quietly take: package-manager
> caches (npm, pnpm, Yarn, pip, Cargo, Gradle, Maven, NuGet and more), project
> build folders like node_modules and target, AI tool and model caches, old
> SDK versions, and system junk. Each item says what it is, how big it is and
> whether it is safe to remove, and it is cleaned with the tool's own command
> where there is one.
>
> It also shows memory-hungry processes and the processes holding ports, so
> you can stop them in one click.
>
> Free and open source (MIT): https://github.com/TriptoAfsin/dev-cleaner
