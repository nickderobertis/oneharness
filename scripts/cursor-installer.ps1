# Return Cursor's Windows installer pinned to an exact cursor-agent version.
#
# Like scripts/install-cursor-agent.sh: Cursor's installer
# (https://cursor.com/install?win32=true) takes no version, it is regenerated
# per release with its version written into its download URL. This reads that
# embedded version, replaces every occurrence with the pin, checks the result
# downloads exactly https://downloads.cursor.com/lab/<pin>/, and returns the
# script text for the caller to run (e2e-cursor.yml runs it visibly).
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
$pinned
