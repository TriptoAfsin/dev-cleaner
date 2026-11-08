# WinGet

Package identifier: `TriptoAfsin.DevCleaner`.

## First submission (once, by hand)

WinGet only auto-updates packages that already exist in
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). After the
first release is published:

```powershell
winget install wingetcreate
wingetcreate new https://github.com/TriptoAfsin/dev-cleaner/releases/download/v0.1.0/Dev.Cleaner_0.1.0_x64_en-US.msi
```

Use `TriptoAfsin.DevCleaner` as the identifier, `Dev Cleaner` as the name, and
submit. Microsoft's validation runs on the PR; once merged, `winget install
TriptoAfsin.DevCleaner` works.

## Every release after that

`.github/workflows/winget.yml` opens the update PR automatically when a
release is published. It needs:

1. A fork of `microsoft/winget-pkgs` under your account.
2. A classic personal access token with `public_repo` scope, saved as the
   `WINGET_TOKEN` repository secret.
