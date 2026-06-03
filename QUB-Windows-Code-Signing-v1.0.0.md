# QUB Core v1.0.0 — Windows signing notes

Windows publisher identity is not a Cargo string. It comes from Authenticode signing with a real code-signing certificate.

## Build unsigned release bundle

```powershell
cargo clean
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan
```

## Build signed release bundle

By certificate subject:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
  -Config regtest-lan `
  -Sign `
  -CertSubject "Alexander Proestakis"
```

By certificate thumbprint:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
  -Config regtest-lan `
  -Sign `
  -CertThumbprint "PASTE_CERT_THUMBPRINT_HERE"
```

The script signs `QUB-Core.exe` and `tools\qubd.exe`, verifies them, then writes `SHA256SUMS.txt`.

## Installer

Install Inno Setup and run:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan -BuildInstaller
```

To sign first and then build the installer:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
  -Config regtest-lan `
  -Sign `
  -CertSubject "Alexander Proestakis" `
  -BuildInstaller
```

If you need the installer executable itself signed too, sign the generated `dist\installer\*.exe` with SignTool after Inno Setup finishes, then publish its SHA256.

## Defender / SmartScreen

A signed publisher helps reputation, but every new binary hash can still start with low reputation. If Microsoft Defender reports an actual malware name, submit the exact file as a false-positive candidate before distributing it publicly.
