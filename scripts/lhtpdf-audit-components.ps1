param(
    [ValidateSet('Audit')]
    [string]$Mode = 'Audit'
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$utf8 = New-Object System.Text.UTF8Encoding($false)

$policyPath = Join-Path $root 'config/lhtpdf-custom.json'
$chromePath = Join-Path $root 'crates/ui-egui/src/chrome.rs'
$uiPath = Join-Path $root 'crates/ui-egui/tests/ui.rs'
$homePath = Join-Path $root 'crates/ui-egui/src/home.rs'


$config = Get-Content -LiteralPath $policyPath -Raw -Encoding UTF8 | ConvertFrom-Json
$contracts = $config.uiRegressionContracts
$upgrade = $contracts.upgradeComponentPolicy

if ($null -eq $upgrade -or $upgrade.version -ne 1) {
    throw 'Unsupported component upgrade policy.'
}

$results = New-Object System.Collections.ArrayList

function Add-Result {
    param(
        [string]$Name,
        [string]$Status,
        [string]$File,
        [string]$Detail,
        [string]$Repair
    )

    $null = $results.Add([pscustomobject]@{
        Component = $Name
        Status = $Status
        File = $File
        Detail = $Detail
        Repair = $Repair
    })
}

function Test-Component {
    param(
        [string]$Name,
        [string]$File,
        [string]$Repair,
        [scriptblock]$Check
    )

    try {
        & $Check
        Add-Result $Name 'INTEGRATED' $File 'Static contract satisfied.' '-'
    }
    catch {
        $message = $_.Exception.Message
        $status = if ($message -like 'INCOMPATIBLE:*') {
            'INCOMPATIBLE'
        } elseif ($message -like 'NOT_VERIFIED:*') {
            'NOT_VERIFIED'
        } else {
            'NEEDS_REPAIR'
        }

        Add-Result $Name $status $File $message $Repair
    }
}

function Require-Token {
    param(
        [string]$Source,
        [string]$Token,
        [string]$Description
    )

    if (!$Source.Contains($Token)) {
        throw "Missing contract: $Description"
    }
}

Write-Host "`n=== LHTPDF COMPONENT AUDIT V1 ==="

# LHTPDF_ABOUT_DISCORD_REGRESSION_V1
Test-Component 'about-branding-and-community-links' 'tests/ui_group/links.rs; src/dialogs.rs; src/widgets.rs' `
    'Keep the lhtPDF About branding and remove Discord from the About dialog.' {
    $linksTests = [IO.File]::ReadAllText(
        (Join-Path $root 'crates/ui-egui/tests/ui_group/links.rs'), $utf8)
    $dialogs = [IO.File]::ReadAllText(
        (Join-Path $root 'crates/ui-egui/src/dialogs.rs'), $utf8)
    $widgets = [IO.File]::ReadAllText(
        (Join-Path $root 'crates/ui-egui/src/widgets.rs'), $utf8)

    Require-Token $linksTests 'fn about_dialog_shows_the_brand_and_links()' 'About branding regression'
    Require-Token $linksTests 'h.query_all_by_label("lhtPDF").count() >= 1' 'About lhtPDF brand assertion'
    Require-Token $linksTests 'h.query_by_label("Join our Discord").is_none()' 'About Discord absence assertion'
    Require-Token $dialogs 'RichText::new("lhtPDF")' 'About product name'
    Require-Token $dialogs 'assets/branding/lhtpdf-icon.png' 'About custom logo'
    if ($dialogs.Contains('Join our Discord')) {
        throw 'About dialog still contains the removed Discord link.'
    }
    $aboutStart = $dialogs.IndexOf('Dialog::About => {')
    if ($aboutStart -lt 0) {
        throw 'About dialog implementation not found.'
    }
    $aboutEnd = $dialogs.IndexOf('Dialog::', $aboutStart + 1)
    if ($aboutEnd -lt 0) {
        $aboutEnd = $dialogs.Length
    }
    $aboutSource = $dialogs.Substring($aboutStart, $aboutEnd - $aboutStart)
    if (!$aboutSource.Contains('widgets::community_links(ui)')) {
        throw 'About dialog must preserve the remaining community links.'
    }
    Require-Token $widgets 'l.command != "help.discord"' 'Discord exclusion from community links'
    Require-Token $widgets 'for l in pdfcraft_engine::links::LINKS.iter().filter' 'Filtered community link iteration'
    if ($linksTests.Contains('h.get_by_label("Join our Discord").click()')) {
        throw 'About regression still requires the removed Discord button.'
    }
}

# LHTPDF_DISCORD_FREE_TOOLBAR_REGRESSION_V1
Test-Component 'discord-free-toolbar' 'tests/ui_group/links.rs; tests/ui.rs; src/chrome.rs' `
    'Keep Discord absent from the top bar while preserving tab controls.' {
    $linksTests = [IO.File]::ReadAllText(
        (Join-Path $root 'crates/ui-egui/tests/ui_group/links.rs'), $utf8)
    $uiTests = [IO.File]::ReadAllText($uiPath, $utf8)
    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)

    Require-Token $linksTests 'fn discord_button_in_the_top_bar_opens_discord()' 'Discord-free toolbar regression'
    Require-Token $linksTests 'h.query_by_label("Discord").is_none()' 'Discord absence in UI Group'
    Require-Token $uiTests 'h.query_by_label("Discord").is_none()' 'Discord absence in main UI tests'
    Require-Token $chrome 'let controls_width =' 'Toolbar controls width'
    if ($linksTests.Contains('h.get_by_label("Discord").click()')) {
        throw 'Toolbar regression still clicks the removed Discord button.'
    }
}
Test-Component 'home-branding' 'tests/ui.rs; src/home.rs' `
    'Restore lhtPDF branding and Home regression assertions.' {

    $ui = [IO.File]::ReadAllText($uiPath, $utf8)
    $homeSource = [IO.File]::ReadAllText($homePath, $utf8)

    foreach ($label in @(
        $contracts.branding.homeWelcome,
        $contracts.branding.recentFilesEmptyText
    )) {
        Require-Token $ui ('get_by_label_contains("' + $label + '")') $label
    }

    foreach ($test in $contracts.branding.regressionTests) {
        Require-Token $ui ("fn $test()") $test
    }

    Require-Token $homeSource 'lhtPDF' 'Home product branding'
}

Test-Component 'edit-mode-selector' 'tests/ui.rs; src/chrome.rs' `
    'Preserve horizontal Edit menu, Edit mode button and role-specific selectors.' {

    $ui = [IO.File]::ReadAllText($uiPath, $utf8)
    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)

    Require-Token $ui ("fn $($contracts.editMode.regressionTest)()") 'Edit regression'
    Require-Token $ui 'get_by_role_and_label(egui::accesskit::Role::Button, "All tools")' 'All tools selector'
    Require-Token $ui 'query_all_by_label("Edit")' 'Edit selector disambiguation'
    Require-Token $chrome 'ui.menu_button(tl!("Edit")' 'Horizontal Edit menu'
    Require-Token $chrome 'widgets::mode_tab(ui, tl!(label)' 'Edit mode tab'
}

Test-Component 'tab-strip-width' 'src/chrome.rs; tests/ui.rs' `
    'Review right-side control widths and rerun tab overlap regression.' {

    $ui = [IO.File]::ReadAllText($uiPath, $utf8)
    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)

    Require-Token $ui ("fn $($contracts.tabBar.regressionTest)()") 'Tab overlap test'
    Require-Token $ui 'h.query_by_label("Discord").is_none()' 'Discord absence assertion'
    Require-Token $ui 'open.right() <= shortcuts.left()' 'Open/right-side controls overlap assertion'
    Require-Token $ui 'open.right() <= h.get_by_label("Keyboard shortcuts").rect().left()' 'Open visibility with scrolling tabs'
    Require-Token $chrome 'let controls_width =' 'Reserved controls width'
    Require-Token $chrome '42.0 // Language switch (EN/VI).' 'Language width reservation'
    Require-Token $chrome '2.0 * ui.spacing().item_spacing.x' 'Discord-free control spacing reservation'
}

Test-Component 'tab-scroll-selector' 'tests/ui.rs' `
    'Use Role::Tab for document selectors and rerun narrow-tab wheel regression.' {

    $ui = [IO.File]::ReadAllText($uiPath, $utf8)

    Require-Token $ui ("fn $($contracts.narrowTabRegressionTest)()") 'Narrow tab regression'

    foreach ($name in @('narrow-00.pdf', 'narrow-11.pdf')) {
        $selector = 'get_by_role_and_label(egui::accesskit::Role::Tab, "' + $name + '")'
        Require-Token $ui $selector "Role::Tab selector: $name"

        if ($ui.Contains('get_by_label("' + $name + '")')) {
            throw "Ambiguous document selector: $name"
        }
    }
}

Test-Component 'horizontal-menu' 'src/chrome.rs' `
    'Review upstream menu wrapper; preserve menu actions and Search.' {

    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)

    Require-Token $chrome 'ui.horizontal_centered(|ui| {' 'Horizontal layout'
    Require-Token $chrome 'ui.menu_button(tl!("File")' 'File menu'
    Require-Token $chrome 'ui.menu_button(tl!("Edit")' 'Edit menu'
    Require-Token $chrome 'ui.menu_button(tl!("Pages")' 'Pages menu'
    Require-Token $chrome 'ui.menu_button(tl!("View")' 'View menu'
    Require-Token $chrome 'ui.menu_button(tl!("Help")' 'Help menu'
    Require-Token $chrome 'widgets::search_box(ui, tl!("Find tools and commands"), 260.0)' 'Upstream Search'
}

Test-Component 'pdf-action-toolbar' 'src/chrome.rs' `
    'Inspect toolbar implementation and preserve upstream PDF actions.' {

    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)

    Require-Token $chrome 'pub fn right_rail(' 'Viewer toolbar owner'
    Require-Token $chrome 'egui::Panel::top("pdf_viewer_toolbar")' 'Horizontal viewer toolbar'
    Require-Token $chrome 'pdf_action_toolbar(app, ui);' 'PDF toolbar invocation'
    Require-Token $chrome 'fn pdf_action_toolbar(' 'PDF action toolbar definition'
    Require-Token $chrome 'ui.add_enabled_ui(has_doc, |ui|' 'Disabled state without document'
}

Test-Component 'inline-search' 'tests/ui_group/links.rs; src/chrome.rs' `
    'Preserve inline Search and shared Find state; rerun Search regression.' {

    $chrome = [IO.File]::ReadAllText($chromePath, $utf8)
    $linksPath = Join-Path $root 'crates/ui-egui/tests/ui_group/links.rs'
    $links = [IO.File]::ReadAllText($linksPath, $utf8)

    Require-Token $links 'fn inline_pdf_toolbar_search_finds_navigates_and_shares_ctrl_f()' 'Inline Search regression'
    Require-Token $links 'get_by_role_and_label(Role::TextInput, "Find text")' 'Search accessibility selector'
    Require-Token $chrome 'Find tools and commands' 'Upstream command Search'
}

Test-Component 'about-version-source' 'src/dialogs.rs; engine/src/build_info.rs' `
    'Require About to display the workspace-driven central version.' {

    $dialogsPath = Join-Path $root 'crates/ui-egui/src/dialogs.rs'
    $buildInfoPath = Join-Path $root 'crates/engine/src/build_info.rs'
    $dialogs = [IO.File]::ReadAllText($dialogsPath, $utf8)
    $buildInfo = [IO.File]::ReadAllText($buildInfoPath, $utf8)
    $about = $contracts.aboutVersion

    if ($null -eq $about -or
        $about.source -cne 'pdfcraft_engine::build_info::version()' -or
        $about.workspaceVersionSource -cne 'env!("CARGO_PKG_VERSION")' -or
        $about.requireDynamicVersion -ne $true) {
        throw 'Central About version policy is missing or invalid.'
    }

    Require-Token $dialogs 'let about_version = pdfcraft_engine::build_info::version();' 'Dynamic About version variable'
    Require-Token $dialogs '&about_version' 'About displays central version variable'
    Require-Token $buildInfo 'env!("CARGO_PKG_VERSION")' 'Workspace Cargo version source'
    Require-Token $buildInfo 'describe(VERSION, LABEL)' 'Central build version formatter'
}
Test-Component 'single-release-version-source' 'Cargo.toml; packaging/windows/package.ps1; packaging/env.sh; config/lhtpdf-custom.json' `
    'Require every package script to read the sole workspace version source.' {

    $cargoPath = Join-Path $root 'Cargo.toml'
    $windowsPath = Join-Path $root 'packaging/windows/package.ps1'
    $shellPath = Join-Path $root 'packaging/env.sh'
    $cargoSource = [IO.File]::ReadAllText($cargoPath, $utf8)
    $windowsSource = [IO.File]::ReadAllText($windowsPath, $utf8)
    $shellSource = [IO.File]::ReadAllText($shellPath, $utf8)
    $versionPolicy = $contracts.releaseVersionSource

    if ($null -eq $versionPolicy -or
        $versionPolicy.path -cne 'Cargo.toml' -or
        $versionPolicy.section -cne 'workspace.package' -or
        $versionPolicy.key -cne 'version' -or
        $versionPolicy.requireInternalPathDependenciesInheritWorkspaceVersion -ne $true) {
        throw 'Central single-release-version policy is missing or invalid.'
    }

    Require-Token $cargoSource '[workspace.package]' 'Workspace package version section'
    Require-Token $windowsSource '$Version = $null' 'Windows package reads workspace version'
    Require-Token $windowsSource 'PDFCRAFT_VERSION overrides are disabled' 'Windows override rejection'
    Require-Token $shellSource 'PDFCRAFT_VERSION overrides are disabled' 'Shell override rejection'
    Require-Token $shellSource 'VERSION="$(workspace_version)"' 'Shell reads workspace version'

    if ($windowsSource.Contains("'26.10.21'") -or
        $windowsSource.Contains('$env:PDFCRAFT_VERSION') -eq $false -or
        $shellSource.Contains('${PDFCRAFT_VERSION:-$(workspace_version)}')) {
        throw 'A legacy version override or fixed release threshold remains.'
    }
}
Write-Host "`n=== COMPONENT RESULTS ==="

foreach ($result in $results) {
    Write-Host ("[{0}] {1}" -f $result.Status, $result.Component)
    Write-Host ("  File: {0}" -f $result.File)
    Write-Host ("  Detail: {0}" -f $result.Detail)

    if ($result.Status -ne 'INTEGRATED') {
        Write-Host ("  Repair: {0}" -f $result.Repair)
    }
}

$failed = @($results | Where-Object { $_.Status -ne 'INTEGRATED' })

Write-Host "`n=== COMPONENT AUDIT SUMMARY ==="
Write-Host "Components: $($results.Count)"
Write-Host "Integrated: $(@($results | Where-Object { $_.Status -eq 'INTEGRATED' }).Count)"
Write-Host "Requires review: $($failed.Count)"

if ($failed.Count -gt 0) {
    Write-Host 'RESULT: FAIL - One or more component contracts require review.'
    exit 1
}

Write-Host 'RESULT: PASS - All component static contracts satisfied.'
exit 0