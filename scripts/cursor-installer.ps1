# Return Cursor's Windows installer pinned to an exact cursor-agent build, for
# the caller to run (e2e-cursor.yml runs it visibly). The pinning is
# scripts/install-cursor-agent.sh's, in PowerShell.
#
# Usage: $script = & ./scripts/cursor-installer.ps1 -Version 2026.11.22-e9042b2
#   -InstallerUrl overrides the source (tests use a local fixture path).
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$InstallerUrl = 'https://cursor.com/install?win32=true'
)
$ErrorActionPreference = 'Stop'

function Fail([string]$Message, [string]$Next) {
  if (-not $Next) { $Next = 'rerun with the cursor-agent version .harness-cli-versions pins' }
  throw "cursor-installer: $Message; $Next"
}

# The version reaches a URL and install paths, so it is held to Cursor's
# build-id shape and nothing else.
if ($Version -cnotmatch '^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9a-f]{7,40}$') {
  Fail "'$Version' is not a Cursor build id (YYYY.MM.DD-<hex>)"
}

try {
  if (Test-Path -LiteralPath $InstallerUrl) { $installer = Get-Content -Raw -LiteralPath $InstallerUrl }
  else { $installer = Invoke-RestMethod -Uri $InstallerUrl }
} catch {
  Fail "could not fetch Cursor's installer from ${InstallerUrl}: $_" 'check the network and that the URL still serves the installer'
}

$found = [regex]::Match([string]$installer, 'https://downloads\.cursor\.com/lab/([0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9a-f]{7,40})/')
if (-not $found.Success) {
  Fail "Cursor's installer at $InstallerUrl no longer downloads from https://downloads.cursor.com/lab/<version>/" 'read the current installer and update this script to its new shape'
}
$embedded = $found.Groups[1].Value
$pinned = ([string]$installer).Replace($embedded, $Version)
if ($embedded -cne $Version -and $pinned.Contains($embedded)) {
  Fail "the installer still names $embedded after pinning $Version"
}
if (-not $pinned.Contains("https://downloads.cursor.com/lab/$Version/")) {
  Fail "the pinned installer does not download https://downloads.cursor.com/lab/$Version/"
}
# Every download the pinned installer could make must be the pin's.
foreach ($download in [regex]::Matches($pinned, 'https://downloads\.cursor\.com/lab/([^/\s''"]*)/')) {
  if ($download.Groups[1].Value -cne $Version) {
    Fail "Cursor's installer at $InstallerUrl also downloads build $($download.Groups[1].Value), which pinning $Version leaves in place" 'read the current installer; pin it only once it downloads a single build'
  }
}
$pinned
