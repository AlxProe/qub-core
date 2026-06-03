# QUB Hotfix46 / v1.3.1 — Full Pools GUI

This hotfix is built on the v1.3.0 pooled-mining protocol branch.

## Scope

- Version bump to `v1.3.1`.
- Full GUI Pools window.
- Browse pools with search/open-only filter.
- Join pool by submitting a zero-fee PoW-gated share.
- Start pool mining from GUI.
- Create pool from GUI.
- Manage locally-owned pools from GUI.
- Decrease commission from GUI.
- Pay extra for pool capacity increase from GUI.
- Rename pool from GUI through the new manager-only `POOLRENAME1` marker.
- Explorer transaction JSON parses `pool_rename` markers.
- CLI adds `pool-rename <pool-id> <new-name> [fee]`.

## Consensus / activation

Mainnet remains:

```text
QNS active:              #1000
JIN active:              #5555
QNS miner split:         #8305
JIN conversion:          #8305
Pools activation:        #9999
```

Testnet pooled-mining activation in this branch is `#3285`.

`POOLRENAME1` is a new manager-only pool-management marker. Ship v1.3.1 as mandatory before mainnet reaches `#9999`.

## Build

```powershell
cd C:\Users\proes\Desktop\qub-node\qubd-v1.3.1
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
```

## Mainnet installer

```powershell
$ISCC = @(
  "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
  "C:\Program Files\Inno Setup 6\ISCC.exe",
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found" }
$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -BuildInstaller -SkipTests
```

Patch manifest as mandatory:

```powershell
$Manifest = (Resolve-Path ".\dist\updates\mainnet\windows-x64\manifest.json").Path
$m = Get-Content $Manifest -Raw | ConvertFrom-Json

function Set-JsonProp($obj, $name, $value) {
  if ($obj.PSObject.Properties.Name -contains $name) { $obj.$name = $value }
  else { $obj | Add-Member -NotePropertyName $name -NotePropertyValue $value }
}

Set-JsonProp $m "mandatory" $true
Set-JsonProp $m "chain_upgrade" $true
Set-JsonProp $m "activation_feature" "pools"
Set-JsonProp $m "activation_height" 9999
Set-JsonProp $m "consensus_family" "qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename"
Set-JsonProp $m "notes" "Mandatory QUB Core v1.3.1 update. Adds full Pools GUI and manager-only pool rename support before pooled mining activates on mainnet at block #9999."

$json = $m | ConvertTo-Json -Depth 30
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($Manifest, $json + "`r`n", $utf8NoBom)
```

## Smoke test

Before public distribution:

```text
1. Open GUI and confirm title/version shows v1.3.1.
2. Pools button opens the Pools window.
3. Browse lists existing pools.
4. Create pool works on regtest/testnet.
5. Join submits a share and shows pending status.
6. After mining one block, pool-info shows active miner/recent shares.
7. Start pool mining runs CPU pool miner.
8. Managed pool can be renamed.
9. Managed pool commission can decrease.
10. Commission increase remains rejected.
11. Capacity top-up increases capacity only upward.
12. Solo mining and pool mining are not started simultaneously.
```

## Packaging note

The source artifact intentionally excludes `assets/`, `target/`, `dist/`, `data/`, and `.git/`.
