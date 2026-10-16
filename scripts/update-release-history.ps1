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

$utf8 = New-Object System.Text.UTF8Encoding($false, $true)
$original = $utf8.GetString(
    [System.IO.File]::ReadAllBytes(
        (Resolve-Path -LiteralPath $ReadmePath).Path
    )
)

$start = "<!-- LHTPDF_RELEASE_HISTORY_START -->"
$end = "<!-- LHTPDF_RELEASE_HISTORY_END -->"

if (
    ([regex]::Matches($original, [regex]::Escape($start))).Count -ne 1 -or
    ([regex]::Matches($original, [regex]::Escape($end))).Count -ne 1
) {
    throw "README release history markers must occur exactly once."
}

$startIndex = $original.IndexOf($start)
$endIndex = $original.IndexOf($end)

if ($endIndex -le $startIndex) {
    throw "Invalid release history marker order."
}

$headers = @{
    "User-Agent" = "lhtPDF-Release-History"
    "Accept" = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
}

if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) {
    $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
}

$uri = "https://api.github.com/repos/$Repository/releases?per_page=30"
$response = Invoke-RestMethod -Uri $uri -Headers $headers -Method Get

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

if (@($releases).Count -eq 0) {
    throw "No published GitHub releases found."
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
        # Exclude Markdown table rows and release asset filenames.
        if (
            $line.TrimStart() -match '^\|' -or
            $value -match '\.(msi|zip|exe|txt)(\s|$)' -or
            $value -match '^(?i:Highlights?|Downloads?|Assets?|Files?|Installation|Requirements|Changelog|Release\s+notes)\s*:?\s*$'
        ) {
            continue
        }

        # Skip Markdown table rows and structural labels.
        if (
            $value -match '^\s*\|.*\|\s*$' -or
            $value -match '^\s*:?-{3,}:?(\s*:?-{3,}:?)*\s*$' -or
            $value -match '^(?i:Tệp\s+Mô\s+tả|File\s+Description)$' -or
            $value -match '^(?i:Highlights?|Key\s+changes?|Changes?|What.s\s+new|Downloads?|Assets?|Installation|Requirements)\s*:\s*$' -or
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

$newline = if ($original.Contains("`r`n")) { "`r`n" } else { "`n" }
$lines = New-Object 'System.Collections.Generic.List[string]'

foreach ($release in $releases) {
    $tag = [string]$release.tag_name
    $url = [string]$release.html_url

    if ($url -notmatch '^https://github\.com/') {
        throw "Unexpected release URL: $url"
    }

    $date = ([datetime]$release.published_at).ToString("yyyy-MM-dd")
    $kind = if ($release.prerelease) { "Pre-release" } else { "Stable" }

    $lines.Add("### [$tag]($url) — $date ($kind)")
    $lines.Add("")

    $description = @(Get-ShortDescription -Body ([string]$release.body))

    if ($description.Count -eq 0) {
        $lines.Add("Xem mô tả đầy đủ tại GitHub Release.")
    }
    else {
        foreach ($item in $description) {
            $lines.Add("- $item")
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

            $lines.Add(
                "- [Tải $label]($($asset.browser_download_url))"
            )
        }
    }

    $lines.Add("")
}

$replacement = $newline +
    ($lines.ToArray() -join $newline).TrimEnd() +
    $newline

$contentStart = $startIndex + $start.Length

$updated = $original.Substring(0, $contentStart) +
    $replacement +
    $original.Substring($endIndex)

if ($updated -ceq $original) {
    Write-Host "README release history already up to date."
    exit 0
}

[System.IO.File]::WriteAllText(
    (Resolve-Path -LiteralPath $ReadmePath).Path,
    $updated,
    $utf8
)

Write-Host "Updated README release history."
Write-Host "Published releases included: $(@($releases).Count)"
