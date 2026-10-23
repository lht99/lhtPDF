param(
    [ValidateSet('Audit', 'Apply')]
    [string]$Mode = 'Audit'
)

$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$configPath = Join-Path $root 'config/lhtpdf-custom.json'
$chromePath = Join-Path $root 'crates/ui-egui/src/chrome.rs'
$libPath = Join-Path $root 'crates/ui-egui/src/lib.rs'
$utf8 = New-Object System.Text.UTF8Encoding($false)

Write-Host "`n=== LHTPDF CENTRAL CUSTOMIZATION: $Mode ==="

# The checked-in configuration is the policy source of truth.
$config = Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 |
    ConvertFrom-Json

if ($config.schemaVersion -ne 1 -or
    $config.project.name -cne 'lhtPDF' -or
    $config.interface.menuLayout -cne 'horizontal' -or
    $config.interface.preserveCustomMenus -ne $true -or
    $config.interface.preserveUpstreamFeatures -ne $true) {
    throw 'Unsupported central customization policy.'
}

$original = [IO.File]::ReadAllText($chromePath, $utf8)
$source = $original.Replace("`r`n", "`n")
$lib = [IO.File]::ReadAllText($libPath, $utf8)

$pattern = '(?ms)^fn main_menu\(app: &mut PdfCraftApp, ui: &mut egui::Ui\) \{.*?^\}'
$matches = [regex]::Matches($source, $pattern)

if ($matches.Count -ne 1) {
    throw "Expected exactly one main_menu; found $($matches.Count)."
}

$match = $matches[0]
$menu = $match.Value

if (!$source.Contains('pub fn main_menu_bar(') -or
    !$source.Contains('pub fn mode_bar(') -or
    !$lib.Contains('chrome::main_menu_bar(self, ui);')) {
    throw 'Release-style menu rendering points are missing.'
}

# Search is an upstream feature, not part of the legacy menu adapter.
$search = 'widgets::search_box(ui, tl!("Find tools and commands"), 260.0)'

if (!$source.Contains($search)) {
    throw 'Upstream Search implementation changed; review required.'
}

foreach ($name in @('File', 'Edit', 'Pages', 'View', 'Help')) {
    $token = 'ui.menu_button(tl!("' + $name + '")'

    if (!$menu.Contains($token)) {
        throw "Expected menu item missing: $name"
    }
}

foreach ($token in @(
    'crate::commands::registry_menu(',
    'RightPanel::Search',
    'RightPanel::Compare'
)) {
    if (!$menu.Contains($token)) {
        throw "Upstream menu feature changed: $token"
    }
}

# v0.13.0 renders the menu items inside main_menu(), while the
# horizontal layout is owned by main_menu_bar().
$menuBarPattern = '(?ms)^pub fn main_menu_bar\(app: &mut PdfCraftApp, ui: &mut egui::Ui\) \{.*?^\}'
$menuBarMatches = [regex]::Matches($source, $menuBarPattern)

$horizontal = (
    $menuBarMatches.Count -eq 1 -and
    $menuBarMatches[0].Value.Contains('ui.horizontal_centered(|ui| {') -and
    $menuBarMatches[0].Value.Contains('main_menu(app, ui);') -and
    !$menu.Contains('egui::Popup::menu(&resp)') -and
    !$menu.Contains('widgets::ghost_button(ui, "panel-left", tl!("Menu"))')
)

if ($horizontal) {
    Write-Host 'PASS: Horizontal menu already integrated.'
    Write-Host 'PASS: Upstream Search and menu actions preserved.'
    Write-Host 'ACTION: NONE'
}
else {
    # The adapter supports the verified upstream popup structure.
    # Unknown upstream layouts must be reviewed, never overwritten.
    $oldStartLines = @(
        'fn main_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {'
        '    let t = Tokens::get(ui.ctx());'
        '    let resp = widgets::ghost_button(ui, "panel-left", tl!("Menu"));'
        '    egui::Popup::menu(&resp).show(|ui| {'
        '        scroll_menu(ui, |ui| {'
        '            ui.set_min_width(230.0);'
    )

    $oldStart = ($oldStartLines -join "`n") + "`n"
    $oldEnd = "        })`n    });`n}"

    if (!$menu.StartsWith($oldStart, [StringComparison]::Ordinal) -or
        !$menu.EndsWith($oldEnd, [StringComparison]::Ordinal)) {
        throw 'Unknown upstream menu wrapper; manual review required.'
    }

    $body = $menu.Substring(
        $oldStart.Length,
        $menu.Length - $oldStart.Length - $oldEnd.Length
    )

    if ($body.Length -lt 1000) {
        throw 'Unexpectedly short upstream menu body.'
    }

    Write-Host 'PASS: Known vertical popup layout recognized.'

    if ($Mode -eq 'Audit') {
        Write-Host 'PENDING: Horizontal restoration is required.'
        Write-Host 'ACTION: Run -Mode Apply after reviewing the upstream update.'
    }
    else {
        $dirty = @(git -C $root status --porcelain --untracked-files=no)

        if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) {
            throw 'Tracked changes exist. Refusing automatic modification.'
        }

        if ((git -C $root branch --show-current).Trim() -eq 'main') {
            throw 'Refusing to modify protected main branch.'
        }

        # Remove only the two outer popup containers.
        # Keep menu actions, submenu contents and scroll helpers.
        $body = [regex]::Replace($body, '(?m)^ {12}', '        ')

        $newStartLines = @(
            'fn main_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {'
            '    let t = Tokens::get(ui.ctx());'
            '    ui.horizontal_centered(|ui| {'
            '        ui.spacing_mut().item_spacing.x = 8.0;'
        )

        $newStart = ($newStartLines -join "`n") + "`n"
        $newMenu = $newStart +
            $body.TrimEnd("`n") + "`n    });`n}"

        $updated = $source.Substring(0, $match.Index) +
            $newMenu +
            $source.Substring($match.Index + $match.Length)

        if (!$updated.Contains($search) -or
            $newMenu.Contains('egui::Popup::menu(&resp)')) {
            throw 'Upstream preservation check failed.'
        }

        if ($source.Remove($match.Index, $match.Length) -cne
            $updated.Remove($match.Index, $newMenu.Length)) {
            throw 'Unexpected changes outside main_menu.'
        }

        $lineEnding = if ($original.Contains("`r`n")) {
            "`r`n"
        } else {
            "`n"
        }

        $output = $updated.Replace("`n", $lineEnding)
        [IO.File]::WriteAllText($chromePath, $output, $utf8)

        Write-Host 'APPLIED: Horizontal menu adapter.'
        Write-Host 'NEXT: Run cargo check, tests and review the diff.'
        Write-Host 'NOTE: No automatic Git commit.'
    }
}

# LHTPDF_CENTRAL_HOME_AND_VIEWER_AUDIT_V1
# These requirements are part of the versioned lhtPDF policy.
# The viewer layout is an audit-only requirement until its implementation
# passes full UI regression. Never silently overwrite unknown upstream UI.

if ($config.home.supportUrl -cne 'https://letuan.io.vn' -or
    $config.home.openUrlMechanism -cne 'egui-platform-output' -or
    $config.home.kittestDeliveryFrames -ne 1 -or
    $config.interface.pdfViewerToolbarLayout -cne 'horizontal' -or
    $config.interface.pdfViewerToolbarHeight -ne 56 -or
    $config.interface.pdfViewerToolbarHorizontalScroll -ne $true -or
    $config.interface.preserveUpstreamPdfNavigation -ne $true -or
    $config.requiredValidation.homeSupportLink -ne $true -or
    $config.requiredValidation.pdfViewerToolbar -ne $true) {
    throw 'Unsupported or incomplete lhtPDF Home/viewer policy.'
}

$homeSourcePath = Join-Path $root 'crates/ui-egui/src/home.rs'
$homeTestPath = Join-Path $root 'crates/ui-egui/tests/it/links.rs'

$homeSource = [IO.File]::ReadAllText($homeSourcePath, $utf8)
$homeTest = [IO.File]::ReadAllText($homeTestPath, $utf8)

if (!$homeSource.Contains($config.home.supportUrl) -or
    !$homeSource.Contains($config.home.supportLabel) -or
    !$homeSource.Contains('ui.hyperlink_to(')) {
    throw 'Home support hyperlink differs from central policy.'
}

if (!$homeTest.Contains('fn home_screen_links()') -or
    !$homeTest.Contains('h.run_steps(1);') -or
    !$homeTest.Contains('egui::OutputCommand::OpenUrl(') -or
    !$homeTest.Contains($config.home.supportUrl)) {
    throw 'Home hyperlink regression test differs from central policy.'
}

Write-Host 'PASS: Home support URL and frame-local OpenUrl test present.'

$railPattern = '(?ms)^pub fn right_rail\(app: &mut PdfCraftApp, ui: &mut egui::Ui\) \{.*?^\}'
$railMatches = [regex]::Matches($source, $railPattern)

if ($railMatches.Count -ne 1) {
    throw 'PDF viewer toolbar changed structurally; manual review required.'
}

$railSource = $railMatches[0].Value

foreach ($upstreamFeature in @(
    'DocView::parse_zoom_percent(&typed)',
    'view.view_history(forward)',
    'view.page_input_id()',
    'view.page_input_reselect'
)) {
    if (!$railSource.Contains($upstreamFeature)) {
        throw "Upstream PDF viewer feature missing: $upstreamFeature"
    }
}

if ($railSource.Contains('egui::Panel::top("pdf_viewer_toolbar")') -and
    $railSource.Contains('egui::ScrollArea::horizontal()') -and
    !$railSource.Contains('egui::Panel::right("rail")')) {
    Write-Host 'PASS: Horizontal PDF viewer toolbar implemented.'
}
elseif ($railSource.Contains('egui::Panel::right("rail")')) {
    throw 'PDF viewer toolbar remains vertical; restore the horizontal lhtPDF toolbar and rerun UI tests before accepting this integration.'
}
else {
    throw 'Unknown PDF viewer toolbar layout; manual review required.'
}

# LHTPDF_PDF_ACTION_TOOLBAR_CONTRACT_V2
# Require each enabled toolbar button to retain its matching upstream action.
$actionPattern = '(?ms)^pub fn pdf_action_toolbar\(app: &mut PdfCraftApp, ui: &mut egui::Ui\) \{.*?^\}'
$actionMatches = [regex]::Matches($source, $actionPattern)

if ($actionMatches.Count -ne 1) {
    throw 'PDF action toolbar missing or structurally changed; manual review required.'
}

$actionSource = $actionMatches[0].Value

if (!$actionSource.Contains('ui.add_enabled_ui(has_doc, |ui|')) {
    throw 'PDF action toolbar must remain disabled without a document.'
}

$actionContracts = @(
    @{
        Name = 'Properties'
        Command = 'file.properties'
        Icon = 'info'
        Action = 'app.dialog = Some(Dialog::Properties(PropsTab::Description));'
    },
    @{
        Name = 'Save'
        Command = 'file.save'
        Icon = 'save'
        Action = 'app.run_command("file.save");'
    },
    @{
        Name = 'Print'
        Command = 'print.dialog'
        Icon = 'printer'
        Action = 'app.run_command("print.dialog");'
    }
)

foreach ($contract in $actionContracts) {
    $command = [regex]::Escape('"' + $contract.Command + '"')
    $icon = [regex]::Escape('"' + $contract.Icon + '"')
    $action = [regex]::Escape($contract.Action)

    $buttonPattern = '(?s)command_tip\([^;]*?' + $command +
        '\s*\)\s*;\s*if\s+icons::button\(\s*ui\s*,\s*' +
        $icon +
        '\s*,\s*42\.0\s*,\s*false\s*,\s*&tip\s*\)\.clicked\(\)\s*\{\s*' +
        $action + '\s*\}'

    $count = [regex]::Matches($actionSource, $buttonPattern).Count

    if ($count -ne 1) {
        throw "PDF action routing invalid: $($contract.Name)"
    }

    Write-Host "PASS: $($contract.Name) button routes to its expected action."
}

if (!$railSource.Contains('pdf_action_toolbar(app, ui);')) {
    throw 'PDF viewer does not invoke the PDF action toolbar.'
}

Write-Host 'PASS: PDF action toolbar routing contracts V2.'
# LHTPDF_CENTRAL_UI_TEST_POLICY_V2
# Static contracts for the 84-test UI suite, including inline Search.
# These checks do not replace running cargo test.

$p = $config.uiRegression

if ($null -eq $p -or
    $p.expectedTestCount -ne 84 -or
    $p.home.deliveryFrames -ne 1 -or
    $p.home.outputCommand -cne 'egui::OutputCommand::OpenUrl' -or
    $p.editMenu.accessibilityLabel -cne 'Edit' -or
    $p.editMenu.selection -cne 'topmost' -or
    $p.fileRadio.accessibilityLabel -cne 'File' -or
    $p.fileRadio.role -cne 'RadioButton' -or
    $p.viewMenu.accessibilityLabel -cne 'View' -or
    $p.viewMenu.selection -cne 'topmost' -or
    $p.viewMenu.themeSubmenuLabel -cne ('Display theme ' + [char]0x23F5) -or
    $p.inlineSearch.test -cne 'links::inline_pdf_toolbar_search_finds_navigates_and_shares_ctrl_f' -or
    $p.inlineSearch.accessibilityLabel -cne 'Find text' -or
    $p.inlineSearch.query -cne 'o' -or
    $p.inlineSearch.expectedMatches -ne 2) {
    throw 'Central UI regression policy is invalid.'
}

$uiTestsDir = Join-Path $root 'crates/ui-egui/tests/it'

$checks = @(
    @{
        File = 'links.rs'
        Tokens = @(
            'fn inline_pdf_toolbar_search_finds_navigates_and_shares_ctrl_f()'
            'get_by_role_and_label(Role::TextInput, "Find text")'
            '.type_text("o")'
            'f.matches.len() == 2'
            'get_by_label_contains("Next (")'
            'get_by_label_contains("Previous (")'
            'views[0].open_find()'
        )
    }
    @{
        File = 'links.rs'
        Tokens = @(
            'fn home_screen_links()'
            'h.run_steps(1);'
            'egui::OutputCommand::OpenUrl('
        )
    }
    @{
        File = 'text_select.rs'
        Tokens = @(
            'fn selected_text_copy_edit_and_context_menus()'
            'h.query_all_by_label("Edit")'
            '.rect().top().total_cmp(&b.rect().top())'
        )
    }
    @{
        File = 'marks.rs'
        Tokens = @(
            'fn backgrounds_and_watermarks_from_a_file()'
            'egui::accesskit::Role::RadioButton'
            'Some("File")'
        )
    }
    @{
        File = 'theme.rs'
        Tokens = @(
            'fn toolbar_and_view_menu_share_three_state_preferences()'
            'h.query_all_by_label("View")'
            'ThemePreference::System'
        )
    }
)

foreach ($check in $checks) {
    $testPath = Join-Path $uiTestsDir $check.File

    if (!(Test-Path -LiteralPath $testPath -PathType Leaf)) {
        throw "UI regression file missing: $($check.File)"
    }

    $testSource = [IO.File]::ReadAllText($testPath, $utf8)

    foreach ($token in $check.Tokens) {
        if (!$testSource.Contains($token)) {
            throw "UI regression contract mismatch: $($check.File)"
        }
    }

    Write-Host "PASS: $($check.File)"
}

$themePath = Join-Path $uiTestsDir 'theme.rs'
$themeSource = [IO.File]::ReadAllText($themePath, $utf8)

$submenuSelector = 'h.get_by_label("' +
    $p.viewMenu.themeSubmenuLabel + '").hover();'

if (!$themeSource.Contains($submenuSelector)) {
    throw 'Theme submenu selector differs from central configuration.'
}

Write-Host 'PASS: Theme submenu Unicode selector.'
$searchChrome = [IO.File]::ReadAllText(
    (Join-Path $root 'crates/ui-egui/src/chrome.rs'), $utf8
)
$searchCanvas = [IO.File]::ReadAllText(
    (Join-Path $root 'crates/ui-egui/src/canvas.rs'), $utf8
)

foreach ($token in @(
    'pdf-toolbar-find'
    'node.set_label(tl!("Find text"))'
    'view.rerun_find();'
    'view.find_step(forward);'
)) {
    if (!$searchChrome.Contains($token)) {
        throw "Inline toolbar Search contract missing: $token"
    }
}

foreach ($token in @(
    'pub toolbar_visible: bool'
    'f.toolbar_visible = false;'
    'if find.in_panel || find.toolbar_visible'
)) {
    if (!$searchCanvas.Contains($token)) {
        throw "Shared upstream Find contract missing: $token"
    }
}

Write-Host 'PASS: Inline Search toolbar and shared Find state.'
Write-Host 'PASS: Central UI regression contracts.'
# LHTPDF_CENTRAL_UI_REGRESSION_CONTRACT_V3
# Audit-only static contracts. Runtime UI tests remain authoritative.
$q = $config.uiRegressionContracts

if ($null -eq $q -or
    $q.version -ne 1 -or
    $q.releaseVersionSource.path -cne 'Cargo.toml' -or
    $q.releaseVersionSource.section -cne 'workspace.package' -or
    $q.releaseVersionSource.key -cne 'version' -or
    $q.releaseVersionSource.requireInternalPathDependenciesInheritWorkspaceVersion -ne $true -or    $q.branding.productName -cne 'lhtPDF' -or
    $q.aboutVersion.source -cne 'pdfcraft_engine::build_info::version()' -or
    $q.aboutVersion.workspaceVersionSource -cne 'env!("CARGO_PKG_VERSION")' -or
    $q.aboutVersion.requireDynamicVersion -ne $true -or
    $q.editMode.modeButtonRole -cne 'Button' -or
    $q.editMode.preserveHorizontalEditMenu -ne $true -or
    $q.editMode.preserveEditModeButton -ne $true -or
    $q.editMode.requireUnambiguousTestSelector -ne $true -or
    $q.tabBar.preserveOpenButton -ne $true -or
    $q.tabBar.preserveDiscordButton -ne $false -or
    $q.tabBar.preventControlOverlap -ne $true -or
    $q.validation.doNotPromoteVerifiedStateUntilFullPass -ne $true) {
    throw 'Central UI regression V3 policy is invalid.'
}

# Verification status is versioned data, not a permanent V3 constant.
# V4 checks the exact result below; V3 validates only the contract structure.
if ([string]::IsNullOrWhiteSpace($q.validation.fullUiRegressionStatus) -or
    [string]::IsNullOrWhiteSpace($q.tabBar.status)) {
    throw 'Missing UI regression verification state.'
}

$uiMainPath = Join-Path $root 'crates/ui-egui/tests/it/ui.rs'
$uiMain = [IO.File]::ReadAllText($uiMainPath, $utf8)

foreach ($testName in $q.branding.regressionTests) {
    if (!$uiMain.Contains("fn $testName()")) {
        throw "Home regression test missing: $testName"
    }
}

foreach ($label in @(
    $q.branding.homeWelcome,
    $q.branding.recentFilesEmptyText
)) {
    if (!$uiMain.Contains('get_by_label_contains("' + $label + '")')) {
        throw "Home branding regression assertion missing: $label"
    }
}

if (!$uiMain.Contains("fn $($q.editMode.regressionTest)()") -or
    !$uiMain.Contains('get_by_role_and_label(egui::accesskit::Role::Button, "All tools")') -or
    !$uiMain.Contains('query_all_by_label("Edit")') -or
    !$uiMain.Contains('expected exactly one Edit mode button')) {
    throw 'Edit mode regression selector contract differs from Central.'
}

if (!$uiMain.Contains("fn $($q.tabBar.regressionTest)()") -or
    !$uiMain.Contains('h.query_by_label("Discord").is_none()') -or
    !$uiMain.Contains('open.right() <= shortcuts.left()') -or
    !$uiMain.Contains('open.right() <= h.get_by_label("Keyboard shortcuts").rect().left()')) {
    throw 'Discord-free tab strip regression contract differs from Central.'
}

if (!$source.Contains('ui.menu_button(tl!("Edit")') -or
    !$source.Contains('widgets::mode_tab(ui, tl!(label)')) {
    throw 'Horizontal Edit menu or Edit mode button is missing.'
}

Write-Host 'PASS: Central UI regression V3 static contracts.'
# LHTPDF_COMPONENT_UPGRADE_POLICY_V1
$upgrade = $q.upgradeComponentPolicy

if ($null -eq $upgrade -or
    $upgrade.version -ne 1 -or
    $upgrade.preserveVerifiedRegressionFixes -ne $true -or
    $upgrade.preserveCustomUiAndUpstreamFeatures -ne $true -or
    $upgrade.auditEachComponentIndependently -ne $true -or
    $upgrade.reportEachComponentStatus -ne $true -or
    $upgrade.continueAuditingAfterComponentFailure -ne $true -or
    $upgrade.neverSilentlyOverwriteUnknownUpstream -ne $true -or
    $upgrade.neverAutomaticallyPromoteVerifiedHead -ne $true) {
    throw 'Component upgrade policy is invalid.'
}

$requiredComponents = @(
    'home-branding',
    'edit-mode-selector',
    'tab-strip-width',
    'tab-scroll-selector',
    'horizontal-menu',
    'pdf-action-toolbar',
    'inline-search'
)

foreach ($component in $requiredComponents) {
    if ($upgrade.protectedComponents -cnotcontains $component) {
        throw "Missing protected upgrade component: $component"
    }
}

foreach ($status in @(
    'INTEGRATED',
    'NEEDS_REPAIR',
    'INCOMPATIBLE',
    'NOT_VERIFIED'
)) {
    if ($upgrade.statuses -cnotcontains $status) {
        throw "Missing upgrade component status: $status"
    }
}

Write-Host 'PASS: Component upgrade policy recorded and validated.'
# Component-by-component execution/reporting is a separate pending implementation.
# LHTPDF_CENTRAL_UI_REGRESSION_CONTRACT_V4
# Preserve verified selectors and record the exact test scope.
# V0.13.0: Previous regression evidence is archived, not current.
if ($q.full84TestSuiteStatus -cne 'NOT_VERIFIED' -or
    $q.validation.fullUiRegressionStatus -cne 'NOT_VERIFIED' -or
    $q.tabBar.status -cne 'NOT_VERIFIED' -or
    $q.narrowTabSelectorRole -cne 'Tab' -or
    $q.narrowTabRegressionTest -cne 'narrow_tab_strips_accept_vertical_wheel_scrolling' -or
    $config.legacyVerificationV0120.fullUiTestVerification.totalPassed -ne 139 -or
    $config.legacyVerificationV0120.fullUiTestVerification.status -cne 'PASS') {
    throw 'Central v0.13.0 verification state differs from policy.'
}

if (!$uiMain.Contains("fn $($q.narrowTabRegressionTest)()")) {
    throw 'Narrow tab regression test is missing.'
}

foreach ($name in @('narrow-00.pdf', 'narrow-11.pdf')) {
    $selector = 'get_by_role_and_label(egui::accesskit::Role::Tab, "' + $name + '")'
    if (!$uiMain.Contains($selector)) {
        throw "Narrow tab accessibility selector missing: $name"
    }
}

if ($uiMain.Contains('get_by_label("narrow-00.pdf")') -or
    $uiMain.Contains('get_by_label("narrow-11.pdf")')) {
    throw 'Ambiguous narrow tab accessibility selector detected.'
}

Write-Host 'PASS: v0.13.0 regression contracts structurally checked.'
Write-Host 'NOT_VERIFIED: v0.13.0 runtime UI regression.'
# LHTPDF_REGRESSION_REPAIR_CONTRACTS_V1
$repairPolicy = $q.regressionRepairContracts

if ($null -eq $repairPolicy -or
    $repairPolicy.version -ne 1 -or
    $repairPolicy.automaticSourceRewrite -ne $false -or
    $repairPolicy.requireUpstreamCompatibilityReview -ne $true -or
    $repairPolicy.requireRelevantRegressionPass -ne $true -or
    $repairPolicy.preserveExistingVerifiedTests -ne $true -or
    $repairPolicy.allowUnknownUpstreamOverwrite -ne $false) {
    throw 'Central regression repair policy is invalid.'
}

$repairIds = @($repairPolicy.components | ForEach-Object { $_.id })

if ($repairIds.Count -ne $requiredComponents.Count -or
    @(Compare-Object ($repairIds | Sort-Object) ($requiredComponents | Sort-Object)).Count -ne 0) {
    throw 'Central regression repair components differ from protected component list.'
}

foreach ($repairContract in $repairPolicy.components) {
    if ([string]::IsNullOrWhiteSpace($repairContract.requirement) -or
        [string]::IsNullOrWhiteSpace($repairContract.repairStrategy) -or
        @($repairContract.sourceFiles).Count -eq 0 -or
        @($repairContract.validation).Count -eq 0) {
        throw "Incomplete regression repair contract: $($repairContract.id)"
    }
}

Write-Host 'PASS: Seven versioned regression repair contracts validated.'
# LHTPDF_FILE_FORMATTING_POLICY_V1
$formatPolicy = $config.fileFormattingPolicy

if ($null -eq $formatPolicy -or
    $formatPolicy.version -ne 1 -or
    $formatPolicy.encoding -cne 'UTF-8' -or
    $formatPolicy.byteOrderMark -ne $false -or
    $formatPolicy.lineEnding -cne 'LF' -or
    $formatPolicy.preserveExistingLineEndings -ne $true -or
    $formatPolicy.rejectMixedLineEndings -ne $true -or
    $formatPolicy.rejectTrailingWhitespace -ne $true -or
    $formatPolicy.requireGitDiffCheck -ne $true -or
    $formatPolicy.automaticRewriteDuringAudit -ne $false) {
    throw 'Central file formatting policy is invalid.'
}

foreach ($relativePath in $formatPolicy.managedFiles) {
    $filePath = Join-Path $root $relativePath

    if (!(Test-Path -LiteralPath $filePath)) {
        throw "Managed formatting file missing: $relativePath"
    }

    $bytes = [IO.File]::ReadAllBytes($filePath)

    if ($bytes.Length -ge 3 -and
        $bytes[0] -eq 239 -and
        $bytes[1] -eq 187 -and
        $bytes[2] -eq 191) {
        throw "UTF-8 BOM detected: $relativePath"
    }

    $content = [IO.File]::ReadAllText($filePath, $utf8)

    if ($content.Contains("`r")) {
        throw "Non-LF line ending detected: $relativePath"
    }

    if ([regex]::IsMatch($content, '(?m)[\t ]+$')) {
        throw "Trailing whitespace detected: $relativePath"
    }

    Write-Host "PASS: LF / UTF-8 no BOM / no trailing whitespace: $relativePath"
}

Write-Host 'PASS: Central file formatting policy V1.'
# LHTPDF_FULL_UI_TEST_VERIFICATION_V1
$fullUi = $q.fullUiTestVerification

if ($null -eq $fullUi -or
    $fullUi.version -ne 1 -or
    $fullUi.package -cne 'pdfcraft-ui-egui' -or
    $fullUi.status -cne 'NOT_VERIFIED' -or
    $fullUi.ui.status -cne 'NOT_VERIFIED' -or
    $fullUi.uiGroup.status -cne 'NOT_VERIFIED' -or
    $null -ne $fullUi.totalPassed -or
    $null -ne $fullUi.totalFailed -or
    $fullUi.automaticallyPromoteVerifiedHead -ne $false) {
    throw 'v0.13.0 full UI verification state is invalid.'
}

Write-Host 'NOT_VERIFIED: v0.13.0 full UI test suite.'
# LHTPDF_CENTRAL_CONSISTENCY_GUARD_V1
# Fail closed on policy, repair contract, audit coverage or verification drift.
# This guard does not rewrite source files or promote HEAD.
$consistencyProtected = @(
    $q.upgradeComponentPolicy.protectedComponents
)

$consistencyRepairs = @(
    $q.regressionRepairContracts.components
)

$consistencyRepairIds = @(
    $consistencyRepairs | ForEach-Object { $_.id }
)

if ($consistencyProtected.Count -ne 7 -or
    $consistencyRepairIds.Count -ne 7 -or
    @($consistencyProtected | Select-Object -Unique).Count -ne 7 -or
    @($consistencyRepairIds | Select-Object -Unique).Count -ne 7 -or
    @(Compare-Object `
        ($consistencyProtected | Sort-Object) `
        ($consistencyRepairIds | Sort-Object)).Count -ne 0) {
    throw 'Central consistency failure: protected and repair components differ.'
}

$consistencyAuditPath = Join-Path $PSScriptRoot 'lhtpdf-audit-components.ps1'

if (!(Test-Path -LiteralPath $consistencyAuditPath)) {
    throw 'Central consistency failure: Component Audit script missing.'
}

$consistencyAuditSource = [IO.File]::ReadAllText(
    $consistencyAuditPath,
    (New-Object System.Text.UTF8Encoding($false))
)

foreach ($consistencyId in $consistencyProtected) {
    $consistencyToken = "Test-Component '$consistencyId'"

    if (([regex]::Matches(
        $consistencyAuditSource,
        [regex]::Escape($consistencyToken)
    )).Count -ne 1) {
        throw "Central consistency failure: Audit coverage for $consistencyId."
    }
}

$consistencyFull = $q.fullUiTestVerification
$legacyFull = $config.legacyVerificationV0120.fullUiTestVerification

if ($null -eq $consistencyFull -or
    $null -eq $legacyFull -or
    $q.full84TestSuiteStatus -cne 'NOT_VERIFIED' -or
    $consistencyFull.status -cne 'NOT_VERIFIED' -or
    $consistencyFull.ui.status -cne 'NOT_VERIFIED' -or
    $consistencyFull.uiGroup.status -cne 'NOT_VERIFIED' -or
    $null -ne $consistencyFull.totalPassed -or
    $null -ne $consistencyFull.totalFailed -or
    $legacyFull.totalPassed -ne 139 -or
    $legacyFull.status -cne 'PASS' -or
    $q.validation.doNotPromoteVerifiedStateUntilFullPass -ne $true -or
    $config.personalizationV0130.autoPromoteHead -ne $false) {
    throw 'Central consistency failure: verification states differ.'
}

Write-Host 'PASS: Central consistency guard V0.13.0.'
Write-Host 'PASS: Seven protected components and Audit coverage.'
Write-Host 'NOT_VERIFIED: v0.13.0; legacy 139 PASS retained separately.'

# LHTPDF_COMPONENT_AUDIT_INTEGRATION_V1
# Independent component reporting runs after existing Central static checks.
# Apply mode retains its original behavior and safety guards.
if ($Mode -eq 'Audit') {
    $componentAuditPath = Join-Path $PSScriptRoot 'lhtpdf-audit-components.ps1'

    if (!(Test-Path -LiteralPath $componentAuditPath)) {
        throw 'Central Component Audit script is missing.'
    }

    Write-Host "`n=== CENTRAL COMPONENT AUDIT ==="

    & powershell.exe -NoProfile -ExecutionPolicy Bypass `
        -File $componentAuditPath -Mode Audit

    $componentExit = $LASTEXITCODE

    if ($componentExit -ne 0) {
        throw "Central Component Audit failed (exit $componentExit)."
    }

    Write-Host 'PASS: All required component static contracts.'
}
Write-Host "`n=== CENTRAL MENU ADAPTER COMPLETE ==="
