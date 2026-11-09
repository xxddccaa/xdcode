param(
    [switch]$Force,
    [switch]$NoPathUpdate,
    [string]$Release = 'latest',
    [string]$Proxy = $env:XDCODE_INSTALL_PROXY
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-SafeMember([string]$Name) {
    $relative = $Name.TrimEnd('/')
    if ($relative -notmatch '^[A-Za-z0-9._/-]+$' -or
        "/$relative/" -match '/(\.|\.\.)/|//') {
        throw "Unsafe package member: $Name"
    }
    foreach ($component in $relative.Split('/')) {
        if ($component -match '[. ]$|^(?i:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$') {
            throw "Reserved Windows package path: $Name"
        }
    }
}

function Assert-NativeExecutable([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) {
            throw "Expected Windows PE executable: $Path"
        }
        $stream.Position = 0x3c
        $offset = $reader.ReadUInt32()
        if ($offset -lt 64 -or $offset -gt $stream.Length - 26) {
            throw "Truncated PE header: $Path"
        }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) {
            throw "Expected native Windows x86_64 executable: $Path"
        }
        $stream.Position = $offset + 24
        if ($reader.ReadUInt16() -ne 0x20b) {
            throw "Expected PE32+ executable: $Path"
        }
    } finally {
        $reader.Dispose()
    }
}

function Expand-CheckedArchive([string]$Archive, [string]$Directory) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $package = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        foreach ($member in $package.Entries) {
            Assert-SafeMember $member.FullName
            if (-not $names.Add($member.FullName.TrimEnd('/'))) {
                throw "Duplicate package member: $($member.FullName)"
            }
            $kind = ($member.ExternalAttributes -shr 16) -band 0xf000
            if ($kind -notin @(0, 0x8000, 0x4000) -or
                ($member.FullName.EndsWith('/') -and $member.Length -ne 0)) {
                throw "Package contains a link or special file: $($member.FullName)"
            }
            $total += $member.Length
            if ($member.Length -gt 600MB -or $total -gt 1GB) {
                throw 'Package exceeds extraction size limits'
            }
        }
        foreach ($member in $package.Entries) {
            $destination = Join-Path $Directory $member.FullName
            if ($member.FullName.EndsWith('/')) {
                [void][IO.Directory]::CreateDirectory($destination)
            } else {
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
                [IO.Compression.ZipFileExtensions]::ExtractToFile($member, $destination, $false)
            }
        }
    } finally {
        $package.Dispose()
    }
}

function Assert-Package([string]$Directory, [string]$Identity) {
    if ($Identity -cnotmatch '^((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-(g[a-f0-9]{12})$') {
        throw 'Invalid release identity'
    }
    $version, $buildId = $Matches[1], $Matches[2]
    $checksums = Join-Path $Directory 'SHA256SUMS'
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($line in [IO.File]::ReadAllLines($checksums)) {
        if ($line -cnotmatch '^([a-f0-9]{64})  ([A-Za-z0-9._/-]+)$') {
            throw 'Invalid package checksum entry'
        }
        $digest, $name = $Matches[1], $Matches[2]
        Assert-SafeMember $name
        if ($name -ieq 'SHA256SUMS' -or -not $names.Add($name)) {
            throw 'Duplicate or self-referential package checksum'
        }
        $path = Join-Path $Directory $name
        if (-not [IO.File]::Exists($path) -or
            ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) {
            throw "Missing, unsafe or corrupt package file: $name"
        }
    }
    $actual = @(Get-ChildItem -LiteralPath $Directory -Recurse -Force)
    foreach ($item in $actual) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Installed package contains a reparse point'
        }
        if (-not $item.PSIsContainer -and $item.FullName -ne $checksums) {
            $relative = $item.FullName.Substring($Directory.Length + 1).Replace([IO.Path]::DirectorySeparatorChar, [char]'/')
            if (-not $names.Contains($relative)) {
                throw "Unlisted package file: $relative"
            }
        }
    }
    $files = @($actual | Where-Object { -not $_.PSIsContainer -and $_.FullName -ne $checksums })
    if ($names.Count -eq 0 -or $files.Count -ne $names.Count) {
        throw 'Package file list does not match its checksums'
    }
    $info = Get-Content -LiteralPath (Join-Path $Directory 'build-info.json') -Raw | ConvertFrom-Json
    $manifest = Get-Content -LiteralPath (Join-Path $Directory 'xdcode-package.json') -Raw | ConvertFrom-Json
    if ($info.schema_version -ne 1 -or $info.launcher_schema -ne 1 -or $info.version -cne $version -or
        $info.build_id -cne $buildId -or $info.source_dirty -isnot [bool] -or $info.source_dirty -or
        $info.target -cne 'x86_64-pc-windows-msvc' -or $info.source_commit -cnotmatch '^[a-f0-9]{40}$' -or
        "g$($info.source_commit.Substring(0, 12))" -cne $buildId) {
        throw 'Package metadata does not match the selected release'
    }
    if ($manifest.layoutVersion -ne 1 -or $manifest.version -cne $version -or
        $manifest.buildId -cne $buildId -or $manifest.target -cne 'x86_64-pc-windows-msvc' -or
        $manifest.variant -cne 'xdcode' -or $manifest.entrypoint -cne 'bin/xdcode.exe' -or
        $manifest.resourcesDir -cne 'xdcode-resources' -or $manifest.pathDir -cne 'xdcode-path') {
        throw 'Invalid package layout metadata'
    }
    foreach ($binary in @('bin/xdcode.exe', 'bin/xdcode-code-mode-host.exe', 'xdcode-path/rg.exe',
                          'xdcode-resources/xdcode-command-runner.exe', 'xdcode-resources/xdcode-windows-sandbox-setup.exe',
                          'xdcode-resources/xdcode-install-launcher.exe')) {
        if (-not $names.Contains($binary)) {
            throw "Missing executable: $binary"
        }
        Assert-NativeExecutable (Join-Path $Directory $binary)
    }
}

function Assert-Idle {
    if (-not $Force) {
        $running = @(Get-Process -Name xdcode, xdcode-code-mode-host, xdcode-app-server -ErrorAction SilentlyContinue)
        if ($running.Count -gt 0) {
            throw 'Active xdcode processes; finish tasks or pass -Force. No processes are stopped by this installer.'
        }
    }
}

function Assert-ManagedEntry([string]$BinDirectory, [string]$BuildRoot) {
    foreach ($other in @('xdcode.cmd', 'xdcode.com', 'xdcode.bat', 'xdcode.ps1')) {
        if (Test-Path -LiteralPath (Join-Path $BinDirectory $other)) {
            throw "Refusing to shadow an existing entry: $other"
        }
    }
    $entry = Join-Path $BinDirectory 'xdcode.exe'
    $receipt = Join-Path $BinDirectory 'launcher.json'
    $selection = Join-Path $BinDirectory 'current.txt'
    foreach ($path in @($entry, $receipt, $selection)) {
        if ((Test-Path -LiteralPath $path) -and
            ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing an entry reparse point'
        }
    }
    if (-not (Test-Path -LiteralPath $entry) -and
        ((Test-Path -LiteralPath $receipt) -or (Test-Path -LiteralPath $selection))) {
        throw 'Refusing orphaned launcher metadata without a managed entry'
    }
    if (Test-Path -LiteralPath $entry) {
        $metadata = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
        if ($metadata.schema_version -ne 1 -or $metadata.sha256 -cnotmatch '^[a-f0-9]{64}$') {
            throw 'Unrecognized native launcher receipt'
        }
        $original = Join-Path $BuildRoot $metadata.source_identity
        Assert-Package $original $metadata.source_identity
        $originalLauncher = Join-Path $original 'xdcode-resources/xdcode-install-launcher.exe'
        if ((Get-FileHash -LiteralPath $entry -Algorithm SHA256).Hash.ToLowerInvariant() -cne $metadata.sha256 -or
            (Get-FileHash -LiteralPath $originalLauncher -Algorithm SHA256).Hash.ToLowerInvariant() -cne $metadata.sha256) {
            throw 'Existing launcher differs from its verified original package'
        }
        if (Test-Path -LiteralPath $selection) {
            $identity = [IO.File]::ReadAllText($selection).Trim()
            Assert-Package (Join-Path $BuildRoot $identity) $identity
        }
    }
}

function Set-CurrentSelection([string]$BinDirectory, [string]$Identity) {
    $selection = Join-Path $BinDirectory 'current.txt'
    $entryTemporary = Join-Path $BinDirectory ".current-$([Guid]::NewGuid().ToString('N')).txt"
    [IO.File]::WriteAllText($entryTemporary, $Identity, [Text.UTF8Encoding]::new($false))
    try {
        if ([IO.File]::Exists($selection)) {
            $backup = Join-Path $BinDirectory ".current-backup-$([Guid]::NewGuid().ToString('N')).txt"
            try {
                [IO.File]::Replace($entryTemporary, $selection, $backup)
            } finally {
                if ([IO.File]::Exists($backup)) { [IO.File]::Delete($backup) }
            }
        } else {
            [IO.File]::Move($entryTemporary, $selection)
        }
    } finally {
        if ([IO.File]::Exists($entryTemporary)) { [IO.File]::Delete($entryTemporary) }
    }
}

function Find-LatestWindowsRelease([hashtable]$Request, [string]$Repository) {
    $selected = $null
    for ($page = 1; $page -le 20; $page++) {
        $batch = @(Invoke-RestMethod @Request -Uri "https://api.github.com/repos/$Repository/releases?per_page=100&page=$page")
        foreach ($candidate in $batch) {
            if ($null -eq $candidate -or $candidate.draft -or $candidate.prerelease -or
                $candidate.tag_name -cnotmatch '^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-g[a-f0-9]{12}$') {
                continue
            }
            $archiveName = "xdcode-$($candidate.tag_name.Substring(1))-windows-x86_64.zip"
            $assetNames = @($candidate.assets | ForEach-Object { $_.name })
            if ($assetNames -cnotcontains $archiveName -or $assetNames -cnotcontains "$archiveName.sha256") {
                continue
            }
            $published = [DateTimeOffset]::Parse($candidate.published_at, [Globalization.CultureInfo]::InvariantCulture)
            if ($null -eq $selected -or $published -gt $selected.published) {
                $selected = @{ tag = $candidate.tag_name; published = $published }
            }
        }
        if ($batch.Count -lt 100) {
            if ($null -eq $selected) { throw 'No published stable release contains a Windows archive and checksum' }
            return $selected.tag
        }
    }
    throw 'Release history exceeds automatic lookup limits; pin a release with -Release'
}

function Install-Xdcode {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'Use install.sh on Linux or macOS'
    }
    if ($PSVersionTable.PSVersion -lt [Version]'5.1') { throw 'PowerShell 5.1 or newer is required' }
    if ('System.Runtime.InteropServices.RuntimeInformation' -as [type]) {
        $nativeX64 = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -eq 'X64'
    } else {
        $processors = @(Get-CimInstance -ClassName Win32_Processor)
        $nativeX64 = $processors.Count -gt 0 -and @($processors | Where-Object { $_.Architecture -ne 9 }).Count -eq 0
    }
    if (-not $nativeX64) {
        throw 'This release supports Windows x86_64, not Windows ARM64 or 32-bit Windows'
    }
    if ($Release -eq 'latest' -and $env:XDCODE_RELEASE) { $Release = $env:XDCODE_RELEASE }
    $request = @{ UseBasicParsing = $true; ErrorAction = 'Stop' }
    if ($Proxy) { $request.Proxy = $Proxy }
    $repository = 'xxddccaa/xdcode'
    $base = "https://github.com/$repository/releases"
    if ($Release -eq 'latest') {
        $Release = Find-LatestWindowsRelease $request $repository
    }
    if ($Release -cnotmatch '^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-g[a-f0-9]{12}$') {
        throw "Unsupported release tag: $Release"
    }
    $identity = $Release.Substring(1)
    $archiveName = "xdcode-$identity-windows-x86_64.zip"
    $root = Join-Path $env:LOCALAPPDATA 'xdcode'
    $buildRoot = Join-Path $root 'builds'
    $binDirectory = Join-Path $root 'bin'
    foreach ($directory in @($root, $buildRoot, $binDirectory)) {
        [void][IO.Directory]::CreateDirectory($directory)
        if ((Get-Item -LiteralPath $directory -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Installation directories must not be reparse points'
        }
    }
    $temporary = Join-Path ([IO.Path]::GetTempPath()) "xdcode-install-$([Guid]::NewGuid().ToString('N'))"
    [void][IO.Directory]::CreateDirectory($temporary)
    $staging = $null
    $lock = $null
    $freshLauncher = $false
    try {
        Write-Host "Installing $Release for Windows x86_64"
        if ($Force) { Write-Warning '-Force keeps running sessions on their old builds; restart them after tasks finish.' }
        $archive = Join-Path $temporary $archiveName
        Invoke-WebRequest @request -Uri "$base/download/$Release/$archiveName" -OutFile $archive
        $sidecar = "$archive.sha256"
        Invoke-WebRequest @request -Uri "$base/download/$Release/$archiveName.sha256" -OutFile $sidecar
        $contents = [IO.File]::ReadAllText($sidecar)
        if ($contents -cnotmatch '\A([a-f0-9]{64})  ([^\r\n]+)(?:\r?\n)?\z' -or $Matches[2] -cne $archiveName) {
            throw 'Checksum sidecar does not name the selected archive'
        }
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Matches[1]) {
            throw 'Archive checksum mismatch'
        }
        $lockPath = Join-Path $root '.install.lock'
        if ((Test-Path -LiteralPath $lockPath) -and
            ((Get-Item -LiteralPath $lockPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Unsafe installation lock reparse point'
        }
        $lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate,
                               [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        Assert-Idle
        Assert-ManagedEntry $binDirectory $buildRoot
        $staging = Join-Path $buildRoot ".staging-$([Guid]::NewGuid().ToString('N'))"
        [void][IO.Directory]::CreateDirectory($staging)
        Expand-CheckedArchive $archive $staging
        Assert-Package $staging $identity
        $version = $identity.Substring(0, $identity.LastIndexOf('-g'))
        $runtimeHome = $env:XDCODE_HOME
        try {
            $env:XDCODE_HOME = Join-Path $temporary 'probe-home'
            $actual = & (Join-Path $staging 'bin/xdcode.exe') --version
            if ($LASTEXITCODE -ne 0 -or ($actual -join "`n").Trim() -cne "xdcode $version") {
                throw 'Native CLI version differs from package metadata'
            }
            & (Join-Path $staging 'bin/xdcode-code-mode-host.exe') --help | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Code-mode helper probe failed' }
            & (Join-Path $staging 'xdcode-path/rg.exe') --version | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'ripgrep probe failed' }
        } finally {
            $env:XDCODE_HOME = $runtimeHome
        }
        $destination = Join-Path $buildRoot $identity
        if (Test-Path -LiteralPath $destination) {
            if ((Get-Item -LiteralPath $destination -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Existing build is a reparse point'
            }
            if ([IO.File]::ReadAllText((Join-Path $destination 'SHA256SUMS')) -cne
                [IO.File]::ReadAllText((Join-Path $staging 'SHA256SUMS'))) {
                throw 'Existing build differs and is never overwritten'
            }
            Assert-Package $destination $identity
        } else {
            [IO.Directory]::Move($staging, $destination)
            $staging = $null
        }
        Assert-Idle
        Assert-ManagedEntry $binDirectory $buildRoot
        $entry = Join-Path $binDirectory 'xdcode.exe'
        if (-not [IO.File]::Exists($entry)) {
            $originalLauncher = Join-Path $destination 'xdcode-resources/xdcode-install-launcher.exe'
            [IO.File]::Copy($originalLauncher, $entry, $false)
            $freshLauncher = $true
            $receipt = @{schema_version=1; source_identity=$identity; sha256=(Get-FileHash -LiteralPath $entry -Algorithm SHA256).Hash.ToLowerInvariant()}
            [IO.File]::WriteAllText((Join-Path $binDirectory 'launcher.json'), ($receipt | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
        }
        Set-CurrentSelection $binDirectory $identity
        $freshLauncher = $false
        if (-not $NoPathUpdate) {
            $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
            if ($binDirectory -notin @($userPath -split ';')) {
                $updatedPath = if ($userPath) { "$binDirectory;$userPath" } else { $binDirectory }
                [Environment]::SetEnvironmentVariable('Path', $updatedPath, 'User')
            }
            if ($binDirectory -notin @($env:Path -split ';')) { $env:Path = "$binDirectory;$env:Path" }
        }
        Write-Host "Installed $identity. Entrypoint: $entry"
        Write-Host 'Old builds, configuration and sessions retained. Open a new terminal if its PATH is stale.'
    } finally {
        if ($freshLauncher) {
            [IO.File]::Delete((Join-Path $binDirectory 'xdcode.exe'))
            [IO.File]::Delete((Join-Path $binDirectory 'launcher.json'))
        }
        if ($staging -and [IO.Directory]::Exists($staging)) { [IO.Directory]::Delete($staging, $true) }
        if ($lock) { $lock.Dispose() }
        if ([IO.Directory]::Exists($temporary)) { [IO.Directory]::Delete($temporary, $true) }
    }
}

if ($MyInvocation.InvocationName -ne '.') { Install-Xdcode }
