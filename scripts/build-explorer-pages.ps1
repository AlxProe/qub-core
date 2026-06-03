param(
  [string]$ApiBase = 'https://api.qubit-coin.io/api/v1',
  [string]$ProjectName = 'qub-explorer',
  [switch]$Deploy
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Public = Join-Path $Root 'explorer\public'
$Dist = Join-Path $Root 'dist\x-qubit-coin-io'
Remove-Item -Recurse -Force $Dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Dist | Out-Null
Copy-Item (Join-Path $Public '*') $Dist -Recurse -Force
@"
window.QUB_EXPLORER_CONFIG = {
  apiBase: "$ApiBase",
  refreshMs: 5000,
  pageSize: 25
};
"@ | Set-Content -Encoding UTF8 (Join-Path $Dist 'config.js')
Write-Host "Explorer static build ready: $Dist"
Write-Host "API base: $ApiBase"
if ($Deploy) {
  npx wrangler pages deploy $Dist --project-name $ProjectName
}
