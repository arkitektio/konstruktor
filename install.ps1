<#
Konstruktor installer for Windows (PowerShell 5.1 or 7+).

    irm https://raw.githubusercontent.com/arkitektio/konstruktor/main/install.ps1 | iex

Downloads the binary for this machine, verifies it against the release's published checksums,
and installs it. Then, when there is a console to talk to, it asks two things: whether to put
konstruktor on your PATH, and whether to create a hub now, in ~\MyHubs\<identifier>. With
nobody to ask it goes on your PATH and nothing is created.

Options, when the script is run rather than piped (or through the environment when piped):
    -NoRun              install only; ask nothing                    (KONSTRUKTOR_NO_RUN=1)
    -HubDir <path>      put the hub here instead of ~\MyHubs\<id>    (KONSTRUKTOR_HUB_DIR)
    -Template <id>      the kind of hub to create; default: default  (KONSTRUKTOR_TEMPLATE)
    -Version <tag>      a specific release, e.g. konstruktor-v0.6.0  (KONSTRUKTOR_VERSION)
    -Dir <path>         where to install                             (KONSTRUKTOR_INSTALL_DIR)
                        default: %LOCALAPPDATA%\Programs\konstruktor

    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/arkitektio/konstruktor/main/install.ps1))) -NoRun
#>
param(
    [switch]$NoRun,
    [string]$HubDir = $env:KONSTRUKTOR_HUB_DIR,
    [string]$Template = $env:KONSTRUKTOR_TEMPLATE,
    [string]$Version = $env:KONSTRUKTOR_VERSION,
    [string]$Dir = $env:KONSTRUKTOR_INSTALL_DIR,
    # The release asset to fetch. Chosen from this machine; overridable only to test the
    # script somewhere it would not otherwise run.
    [string]$Target = $env:KONSTRUKTOR_TARGET
)

$ErrorActionPreference = 'Stop'
# Invoke-WebRequest's progress bar makes downloads many times slower on Windows PowerShell.
$ProgressPreference = 'SilentlyContinue'

$Repo = 'arkitektio/konstruktor'
$BinName = 'konstruktor'
$HubParent = if ($env:KONSTRUKTOR_HUB_PARENT) { $env:KONSTRUKTOR_HUB_PARENT } else { Join-Path $HOME 'MyHubs' }
if ($env:KONSTRUKTOR_NO_RUN -eq '1') { $NoRun = $true }
if (-not $Dir) {
    $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { Join-Path $HOME '.local' }
    $Dir = Join-Path (Join-Path $base 'Programs') 'konstruktor'
}

function Say([string]$Message) { [Console]::Error.WriteLine("  $Message") }
function Die([string]$Message) {
    [Console]::Error.WriteLine("`n  error: $Message`n")
    # `exit` would close the window of whoever piped this into `iex`; throwing ends only the script.
    throw "konstruktor install failed: $Message"
}

# Windows PowerShell 5.1 still defaults to TLS 1.0/1.1, which GitHub refuses.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

# --- what are we running on ------------------------------------------------------------
if (-not $Target) {
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
        'AMD64' { $Target = 'x86_64-pc-windows-msvc' }
        # No native ARM64 build yet: Windows on ARM runs the x64 binary under emulation.
        'ARM64' { $Target = 'x86_64-pc-windows-msvc'; Say '! No ARM64 build yet; installing the x64 one, which Windows runs emulated.' }
        default { Die "unsupported architecture: $arch. See https://github.com/$Repo/releases" }
    }
}
$IsWindowsTarget = $Target -like '*-windows-*'
$Asset = if ($IsWindowsTarget) { "$BinName-$Target.exe" } else { "$BinName-$Target" }
$ExeName = if ($IsWindowsTarget) { "$BinName.exe" } else { $BinName }

$Base = if ($Version) {
    "https://github.com/$Repo/releases/download/$Version"
} else {
    "https://github.com/$Repo/releases/latest/download"
}

Write-Host ''
Say "konstruktor - $Target"

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("konstruktor-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Tmp -Force | Out-Null

try {
    Say 'Downloading...'
    $Downloaded = Join-Path $Tmp $Asset
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset" -OutFile $Downloaded
    } catch {
        Die "no build for $Target in that release.`n    See https://github.com/$Repo/releases"
    }

    # --- verify -------------------------------------------------------------------------
    # A binary that puts itself on someone's PATH has to be worth trusting; a missing
    # checksum file is a reason to stop, not to shrug.
    $Sums = Join-Path $Tmp 'SHA256SUMS'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS" -OutFile $Sums
    } catch {
        Die 'that release publishes no SHA256SUMS - refusing to install unverified.'
    }
    # "<hash>  <name>", or "<hash> *<name>" as sha256sum writes binary-mode entries.
    $expected = $null
    foreach ($line in Get-Content -LiteralPath $Sums) {
        $parts = $line.Trim() -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $Asset) { $expected = $parts[0].ToLowerInvariant() }
    }
    if (-not $expected) { Die "$Asset is not listed in that release's SHA256SUMS" }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Downloaded).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        Die "checksum mismatch - refusing to install.`n    expected $expected`n    got      $actual"
    }
    Say 'Checksum verified.'

    # --- install ------------------------------------------------------------------------
    New-Item -ItemType Directory -Path $Dir -Force | Out-Null
    $Bin = Join-Path $Dir $ExeName
    try {
        Move-Item -LiteralPath $Downloaded -Destination $Bin -Force
    } catch {
        Die "could not write $Bin - is konstruktor still running? Close it and try again."
    }
    if (-not $IsWindowsTarget) { & chmod +x $Bin }
    Say "Installed to $Bin"
} finally {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# --- two questions, both optional ------------------------------------------------------
#
# No console to ask in - a CI job, a remote non-interactive session - or -NoRun: there is nobody
# to ask, so nothing is asked and no hub is created.
$interactive = -not $NoRun -and [Environment]::UserInteractive -and -not [Console]::IsInputRedirected

# Yes unless told otherwise: a bare Enter takes the default.
function Confirm-Yes([string]$Question) {
    $answer = Read-Host "  $Question [Y/n]"
    return (-not $answer) -or ($answer -match '^[Yy]')
}

# On the user's PATH for good (new terminals), and in this session right away. Asked first,
# before a hub is offered; with nobody to ask it is simply done, as it always was.
$sep = [IO.Path]::PathSeparator
$onPath = ($env:PATH -split [regex]::Escape($sep)) -contains $Dir
if (-not $onPath) {
    $addToPath = $true
    if ($interactive) {
        Write-Host ''
        Say "! $Dir is not on your PATH."
        $addToPath = Confirm-Yes 'Add it?'
    }
    if (-not $addToPath) {
        Say "Put it on your PATH later with: $Bin self install"
    } elseif ($IsWindowsTarget -or $env:OS -eq 'Windows_NT') {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not (($userPath -split ';') -contains $Dir)) {
            $newPath = if ($userPath) { "$userPath;$Dir" } else { $Dir }
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Say "Added $Dir to your PATH (new terminals pick it up)."
        }
    } else {
        Say "! $Dir is not on your PATH."
    }
    $env:PATH = "$env:PATH$sep$Dir"
}

# --- and then actually make a hub ------------------------------------------------------
#
# `hub create` builds the hub in the current directory unless it is given one, and the current
# directory here is wherever the one-liner was pasted. So the installer names the folder: ~\MyHubs,
# one folder per hub, named after the hub itself, or whatever -HubDir says. The identifier is
# asked for out here, before there is a folder to name, and passed on so it is not asked twice.

# Compose's own rules, so the folder name and the identifier can stay the same string.
function ConvertTo-Slug([string]$Text) {
    $slug = $Text.ToLowerInvariant() -replace '[^a-z0-9._-]+', '-'
    $slug = $slug -replace '^[^a-z0-9]+', ''
    return ($slug -replace '-+$', '')
}

# The first free folder for this identifier: ~\MyHubs\lab-hub, then -2, -3, ... Free means "not
# there", or there and empty - `hub create` refuses a folder that already holds a hub.
function Get-FreeHubDir([string]$Id) {
    for ($i = 1; $i -le 20; $i++) {
        $name = if ($i -eq 1) { $Id } else { "$Id-$i" }
        $candidate = Join-Path $HubParent $name
        if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
        if ((Test-Path -LiteralPath $candidate -PathType Container) -and -not (Get-ChildItem -LiteralPath $candidate -Force | Select-Object -First 1)) {
            return $candidate
        }
    }
    return $null
}

# Templates name the kind of hub: `default`, `personal`, ... - `konstruktor hub templates` lists
# them. Releases up to konstruktor-v0.12.1 have neither the command nor the flag, and -Version can
# ask for one of those, so it is probed for rather than assumed. Without it `default` is still
# what an older `hub create` makes; anything else cannot be honoured.
if (-not $Template) { $Template = 'default' }
$hasTemplates = $false
try {
    & $Bin hub templates *> $null
    $hasTemplates = ($LASTEXITCODE -eq 0)
} catch { }
if (-not $hasTemplates -and $Template -ne 'default') {
    Die "this release of konstruktor has no templates, so it cannot create a '$Template' hub.`n    Install a newer one, or leave -Template out."
}

$templateHint = if ($Template -eq 'default') { '' } else { " --template $Template" }
$hint = "Run: mkdir $(Join-Path $HubParent 'my-hub'); cd $(Join-Path $HubParent 'my-hub'); $BinName hub create$templateHint"

if ($NoRun) {
    Write-Host ''
    Say $hint
    Write-Host ''
    return
}

# Installing and stopping is right where there is no console; prompting into nothing is not.
if (-not $interactive) {
    Write-Host ''
    Say 'No console attached, so nothing was created.'
    Say $hint
    Write-Host ''
    return
}

Write-Host ''
if (-not (Confirm-Yes 'Create a hub now?')) {
    Write-Host ''
    Say $hint
    Write-Host ''
    return
}

Write-Host ''
$hubId = ''
while (-not $hubId) {
    $raw = Read-Host '  Hub identifier'
    if ($null -eq $raw) { Die 'no identifier given.' }
    $hubId = ConvertTo-Slug $raw
    if (-not $hubId) { Say '! letters, digits, dot, underscore and dash - try again.' }
}

# An explicit -HubDir is used verbatim; otherwise the first free ~\MyHubs\<id>.
if (-not $HubDir) {
    $HubDir = Get-FreeHubDir $hubId
    if (-not $HubDir) { Die "no free folder for $hubId under $HubParent - pass -HubDir to say where it should go." }
}

Write-Host ''
Say "Creating $hubId in $HubDir"
# `hub create` makes the folder itself, so there is nothing to mkdir or cd into here.
if ($hasTemplates) {
    & $Bin hub create --template $Template --identifier $hubId $HubDir
} else {
    & $Bin hub create --identifier $hubId $HubDir
}
