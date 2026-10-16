
param(
    [string]$Repository = "lht99/lhtPDF",
    [string]$ReadmePath = "README.md",
    [int]$MaxReleases = 3
)

$ErrorActionPreference = "Stop"

if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw "Invalid GitHub repository."
}

if ($MaxReleases -lt 1 -or $MaxReleases -gt 20) {
    throw "MaxReleases must be between 1 and 20."
}

$path = (Resolve-Path -LiteralPath $ReadmePath).Path
$utf8 = New-Object System.Text.UTF8Encoding($false, $true)
$original = $utf8.GetString(
    [System.IO.File]::ReadAllBytes($path)
)

$historyStart = "<!-- LHTPDF_RELEASE_HISTORY_START -->"
$historyEnd = "<!-- LHTPDF_RELEASE_HISTORY_END -->"

$downloadStart = "<!-- LHTPDF_DOWNLOAD_START -->"
$downloadEnd = "<!-- LHTPDF_DOWNLOAD_END -->"

function Replace-MarkedSection {
    param(
        [string]$Content,
        [string]$StartMarker,
        [string]$EndMarker,
        [string]$Replacement,
        [string]$Newline
    )

    if (
        ([regex]::Matches($Content, [regex]::Escape($StartMarker))).Count -ne 1 -or
        ([regex]::Matches($Content, [regex]::Escape($EndMarker))).Count -ne 1
    ) {
        throw "README markers must occur exactly once: $StartMarker"
    }

    $startIndex = $Content.IndexOf($StartMarker)
    $endIndex = $Content.IndexOf($EndMarker)

    if ($endIndex -le $startIndex) {
        throw "Invalid README marker order: $StartMarker"
    }

    $contentStart = $startIndex + $StartMarker.Length

    return $Content.Substring(0, $contentStart) +
        $Newline +
        $Replacement.TrimEnd() +
        $Newline +
        $Content.Substring($endIndex)
}

$headers = @{
    "User-Agent" = "lhtPDF-Release-History"
    "Accept" = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
}

if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) {
    $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
}

$baseApi = "https://api.github.com/repos/$Repository"

# Latest Stable: GitHub's official latest release endpoint.
$latest = Invoke-RestMethod `
    -Uri "$baseApi/releases/latest" `
    -Headers $headers `
    -Method Get

if (
    $latest.draft -or
    $latest.prerelease -or
    [string]::IsNullOrWhiteSpace($latest.tag_name)
) {
    throw "Cannot identify the latest Stable release."
}

# Identify actual assets, rather than constructing filenames.
$latestAssets = @($latest.assets)

$msiAssets = @(
    $latestAssets | Where-Object {
        $_.name -match '(?i)\.msi$' -and
        $_.browser_download_url -match '^https://github\.com/'
    }
)

$zipAssets = @(
    $latestAssets | Where-Object {
        $_.name -match '(?i)portable\.zip$' -and
        $_.browser_download_url -match '^https://github\.com/'
    }
)

if ($msiAssets.Count -ne 1 -or $zipAssets.Count -ne 1) {
    throw "Latest Stable must contain exactly one MSI and one Portable ZIP."
}

$msiUrl = [string]$msiAssets[0].browser_download_url
$zipUrl = [string]$zipAssets[0].browser_download_url

# Download table: direct links to latest Stable assets.
$downloadLines = @(
    "| Tệp | Mô tả |"
    "| --- | --- |"
    "| [Tải MSI Installer]($msiUrl) | Cài đặt lhtPDF trên Windows |"
    "| [Tải Portable ZIP]($zipUrl) | Giải nén và chạy ứng dụng |"
)

# History includes Stable and Pre-release.
$response = Invoke-RestMethod `
    -Uri "$baseApi/releases?per_page=30" `
    -Headers $headers `
    -Method Get

$releases = @(
    @($response) |
        Where-Object {
            -not $_.draft -and
            -not [string]::IsNullOrWhiteSpace($_.tag_name) -and
            -not [string]::IsNullOrWhiteSpace($_.html_url)
        } |
        Sort-Object {
            [datetime]$_.published_at
        } -Descending |
        Select-Object -First $MaxReleases
)

if ($releases.Count -eq 0) {
    throw "No published releases found."
}

function Get-ShortDescription {
    param([string]$Body)

    $items = New-Object 'System.Collections.Generic.List[string]'

    foreach ($line in ($Body -split '\r?\n')) {
        $value = $line.Trim()

        if (
            [string]::IsNullOrWhiteSpace($value) -or
            $value -match '^#{1,6}\s' -or
            $value -match '^---+$' -or
            $value -match '^```'
        ) {
            continue
        }

        $value = $value -replace '^\s*[-*+]\s+', ''
        $value = $value -replace '^\s*\d+[.)]\s+', ''
        $value = $value -replace '\[([^\]]+)\]\([^)]+\)', '$1'
        $value = $value -replace '[`*|]', ''
        $value = $value.Trim()

        if (
            $line.TrimStart() -match '^\|' -or
            $value -match '\.(msi|zip|exe|txt)(\s|$)' -or
            $value -match '^https?://\S+$'
        ) {
            continue
        }

        if ($value.Length -gt 160) {
            $value = $value.Substring(0, 157).TrimEnd() + "..."
        }

        if ($value.Length -gt 0) {
            $items.Add($value)
        }

        if ($items.Count -ge 3) {
            break
        }
    }

    return @($items.ToArray())
}

$historyLines = New-Object 'System.Collections.Generic.List[string]'

foreach ($release in $releases) {
    $tag = [string]$release.tag_name
    $url = [string]$release.html_url

    if ($url -notmatch '^https://github\.com/') {
        throw "Unexpected release URL: $url"
    }

    $date = ([datetime]$release.published_at).ToString("yyyy-MM-dd")
    $kind = if ($release.prerelease) { "Pre-release" } else { "Stable" }

    $historyLines.Add("### [$tag]($url) — $date ($kind)")
    $historyLines.Add("")

    $description = @(Get-ShortDescription -Body ([string]$release.body))

    if ($description.Count -eq 0) {
        $historyLines.Add("Xem mô tả đầy đủ tại GitHub Release.")
    }
    else {
        foreach ($item in $description) {
            $historyLines.Add("- $item")
        }
    }

    $assets = @($release.assets)

    foreach ($extension in @(".msi", ".zip")) {
        $asset = $assets |
            Where-Object {
                $_.name -like "*$extension" -and
                $_.browser_download_url -match '^https://github\.com/'
            } |
            Select-Object -First 1

        if ($null -ne $asset) {
            $label = if ($extension -eq ".msi") {
                "MSI"
            } else {
                "Portable ZIP"
            }

            $historyLines.Add(
                "- [Tải $label]($($asset.browser_download_url))"
            )
        }
    }

    $historyLines.Add("")
}

$newline = if ($original.Contains("`r`n")) { "`r`n" } else { "`n" }

$updated = Replace-MarkedSection `
    -Content $original `
    -StartMarker $historyStart `
    -EndMarker $historyEnd `
    -Replacement ($historyLines.ToArray() -join $newline) `
    -Newline $newline

$updated = Replace-MarkedSection `
    -Content $updated `
    -StartMarker $downloadStart `
    -EndMarker $downloadEnd `
    -Replacement ($downloadLines -join $newline) `
    -Newline $newline

if ($updated -ceq $original) {
    Write-Host "README already up to date."
    exit 0
}

[System.IO.File]::WriteAllText($path, $updated, $utf8)

Write-Host "README updated successfully."
Write-Host "Latest Stable: $($latest.tag_name)"
Write-Host "MSI: $msiUrl"
Write-Host "Portable ZIP: $zipUrl"
Write-Host "History entries: $($releases.Count)"
