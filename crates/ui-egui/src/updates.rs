//! Help ▸ Check for updates (issue #28): ask for the latest release and offer its download page.
//!
//! The desktop app supplies how to ask ([`PdfCraftApp::update_source`]), so this crate has no
//! network code; without a source (the web build, tests) the command opens the releases page.
//! PdfCraft never downloads or installs anything itself: the user downloads the new version.
//! It asks only when the user does: there is no check at start (the owner's decision).

use std::sync::Arc;

use egui::{Align, Layout};

use crate::{PdfCraftApp, theme, widgets};

/// Where every PdfCraft release is listed.
pub const RELEASES_PAGE: &str = "https://github.com/lht99/lhtPDF/releases";

/// The latest published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version tag, such as `v0.2.0`.
    pub version: String,
    /// Its page on [`RELEASES_PAGE`], where the downloads are.
    pub url: String,
    /// Direct download URL for the Windows MSI installer, if available.
    pub msi_url: Option<String>,
}

/// Asks for the latest release (blocking; it runs on its own thread).
pub type UpdateSource = Arc<dyn Fn() -> Result<Release, String> + Send + Sync>;

/// Whether release `latest` (a tag such as `v0.2.0`) is newer than version `current` (`0.1.1`).
/// Pre-release and build suffixes are ignored; a version that doesn't parse is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = |required: bool| match parts.next() {
        Some(p) => p.parse::<u64>().ok(),
        None if required => None,
        None => Some(0),
    };
    let version = (next(true)?, next(false)?, next(false)?);
    parts.next().is_none().then_some(version)
}

/// Where a check is.
#[derive(Default)]
pub(crate) enum Check {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<Release, String>>),
    Done(Result<Release, String>),
}

#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) check: Check,
    /// The Updates dialog is showing.
    pub(crate) open: bool,
    /// Whether this app instance has attempted its startup update check.
    pub(crate) startup_checked: bool,
    /// Whether the running check was initiated automatically at startup.
    pub(crate) startup_pending: bool,
    /// True only while showing a notice opened by the startup check.
    pub(crate) auto_notice: bool,
}

impl PdfCraftApp {
    /// Check for a new release once per application session, without opening a dialog.
    pub(crate) fn check_updates_on_startup(&mut self) {
        if self.updates.startup_checked {
            return;
        }
        self.updates.startup_checked = true;

        if !self.check_updates_at_start {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(source) = self.update_source.clone() else {
                return;
            };

            if matches!(self.updates.check, Check::Running(_)) {
                return;
            }

            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();

            self.updates.startup_pending = true;
            self.updates.check = Check::Running(rx);

            std::thread::spawn(move || {
                let _ = tx.send(source());
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
        }
    }
    /// Help ▸ Check for updates: ask for the latest release and show the outcome.
    pub fn check_for_updates(&mut self) {
        let Some(source) = self.update_source.clone() else {
            self.open_url(RELEASES_PAGE);
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A user-initiated check must never count as an automatic notification.
            // Reuse an in-flight startup request without marking its release as seen.
            self.updates.startup_pending = false;
            self.updates.auto_notice = false;
            self.updates.open = true;
            if matches!(self.updates.check, Check::Running(_)) {
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                // The receiver may be gone (the app quit): nothing to report to then.
                let _ = tx.send(source());
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.check = Check::Running(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = source;
            self.open_url(RELEASES_PAGE);
        }
    }

    /// Pick up a finished check (each frame).
    pub(crate) fn poll_updates(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Check::Running(rx) = &self.updates.check {
            let result = match rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the update check stopped unexpectedly".into()),
            };
            if self.updates.startup_pending {
                self.updates.startup_pending = false;

                if let Ok(release) = &result {
                    let already_notified = self.last_notified_update.as_deref() == Some(release.version.as_str());
                    let (year, month, day) = self.session.today();
                    let today = format!("{year:04}-{month:02}-{day:02}");
                    let snoozed_today = self.update_snooze_until.as_deref() == Some(today.as_str());

                    if self.check_updates_at_start && is_newer(&release.version, env!("CARGO_PKG_VERSION")) && !already_notified && !snoozed_today {
                        self.last_notified_update = Some(release.version.clone());
                        self.updates.auto_notice = true;
                        self.updates.open = true;
                    }
                }
            }

            self.updates.check = Check::Done(result);
        }
    }
}

/// The Updates dialog.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.updates.open {
        return;
    }
    let t = theme::Tokens::get(ctx);
    let current = env!("CARGO_PKG_VERSION");
    let mut close = false;
    let mut download: Option<String> = None;
    let modal = crate::widgets::show_modal(ctx, "updates", |ui| {
        ui.set_width(420.0);
        ui.horizontal(|ui| {
            ui.add(crate::icons::image("cloud", 22.0, t.accent));
            ui.label(egui::RichText::new(tl!("Check for updates")).font(theme::semibold(16.0)));
        });
        ui.add_space(8.0);
        match &app.updates.check {
            Check::Idle => {
                ui.label(tl!("No check has run yet."));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Check::Running(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking for a newer version…"));
                });
            }
            Check::Done(Ok(r)) if is_newer(&r.version, current) => {
                let version = r.version.trim_start_matches(['v', 'V']);
                ui.label(egui::RichText::new(crate::i18n::fmt(tl!("lhtPDF {v} is available."), &[("v", version)])).strong());
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!(if r.msi_url.is_some() {
                            "You have version {c}. Download the Windows MSI installer below."
                        } else {
                            "You have version {c}. Download the new version from its release page."
                        }),
                        &[("c", current)],
                    ))
                    .color(t.text_muted),
                );
                if let Some(msi) = &r.msi_url {
                    download = Some(msi.clone());
                } else {
                    ui.label(egui::RichText::new(tl!("The Windows MSI installer is not available for this release yet.")).color(t.text_muted));
                    ui.hyperlink_to(tl!("View release"), &r.url);
                }
            }
            Check::Done(Ok(_)) => {
                ui.label(crate::i18n::fmt(tl!("lhtPDF {c} is up to date."), &[("c", current)]));
            }
            Check::Done(Err(e)) => {
                ui.label(crate::i18n::fmt(tl!("Couldn't check for updates: {e}"), &[("e", &e.to_string())]));
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!("You have version {c}. All releases are listed at {page}."),
                        &[("c", current), ("page", RELEASES_PAGE)],
                    ))
                    .color(t.text_muted),
                );
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(tl!("Asks GitHub for the latest release. Nothing is downloaded or installed automatically."))
                .color(t.text_muted)
                .small(),
        );
        ui.add_space(12.0);
        ui.checkbox(&mut app.check_updates_at_start, tl!("Notify me about updates at startup"));
        ui.add_space(8.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let newer_release = matches!(
                &app.updates.check,
                Check::Done(Ok(release)) if is_newer(&release.version, current)
            );

            if newer_release {
                let get = download.is_some() && widgets::pill_button(ui, tl!("Download MSI"), true).clicked();

                let snooze = app.updates.auto_notice && widgets::pill_button(ui, tl!("Don't show today"), false).clicked();

                let later = widgets::pill_button(ui, tl!("Later"), false).clicked();

                close = get || later || snooze;

                if snooze {
                    let (year, month, day) = app.session.today();
                    app.update_snooze_until = Some(format!("{year:04}-{month:02}-{day:02}"));

                    if let Check::Done(Ok(release)) = &app.updates.check {
                        if app.last_notified_update.as_deref() == Some(release.version.as_str()) {
                            app.last_notified_update = None;
                        }
                    }
                }

                if !get {
                    download = None;
                }
            } else if widgets::pill_button(ui, tl!("Close"), true).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
        download = None;
    }
    if close {
        app.updates.open = false;
        app.updates.auto_notice = false;
        if let Some(url) = download {
            app.open_url(&url);
        }
    }
}
