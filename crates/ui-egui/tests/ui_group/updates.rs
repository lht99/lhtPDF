//! Help ▸ Check for updates (issue #28), with stand-in release sources (no network).

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::updates::{Release, UpdateSource, is_newer};

fn source(answer: Result<&str, &str>) -> UpdateSource {
    let answer = answer.map(str::to_string).map_err(str::to_string);
    Arc::new(move || {
        answer.clone().map(|v| Release {
            url: format!("https://github.com/lht99/lhtPDF/releases/tag/{v}"),
            msi_url: Some(format!("https://github.com/lht99/lhtPDF/releases/download/{v}/lhtPDF-{v}-windows-x64.msi")),
            version: v,
        })
    })
}

fn harness(answer: Result<&str, &str>) -> Harness<'static, PdfCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.update_source = Some(source(answer));
        app
    })
}

/// Run frames until the background check has reported (or give up).
fn settle(h: &mut Harness<'static, PdfCraftApp>) {
    for _ in 0..200 {
        h.run_steps(2);
        if h.query_by_label_contains("Checking for a newer version").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.2.0", "0.1.1"));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    assert!(!is_newer("v0.1.1", "0.1.1"));
    assert!(!is_newer("v0.1.0", "0.1.1"));
    assert!(!is_newer("v0.1.1-beta.2", "0.1.1"), "suffixes are ignored");
    assert!(!is_newer("nightly", "0.1.1"), "a tag that isn't a version is never newer");
    assert!(!is_newer("v1.2.3.4", "0.1.1"));
    assert!(!is_newer("v99999999999999999999.0.0", "0.1.1"), "out of range");
}

#[test]
fn a_newer_release_is_offered_for_download() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("lhtPDF 99.0.0 is available");
    h.get_by_label("Download MSI");
    assert!(h.query_by_label("Don't show today").is_none(), "Manual checks must not offer the startup-only snooze action");
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("is available").is_none(), "Later closes the dialog");
}

#[test]
fn an_up_to_date_or_failed_check_says_so() {
    let mut h = harness(Ok(concat!("v", env!("CARGO_PKG_VERSION"))));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("is up to date");
    assert!(h.query_by_label("Download").is_none());

    let mut h = harness(Err("couldn't reach GitHub"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("Couldn't check for updates: couldn't reach GitHub");
}

#[test]
fn startup_checks_once_and_offers_new_release() {
    // Startup checks are enabled by default and run only once per app session.
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        // Explicitly enable startup checks through persisted preferences.
        app.restore(r#"{"check_updates_at_start": true}"#);
        let counted = counted.clone();
        app.update_source = Some(Arc::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Release {
                version: "v99.0.0".into(),
                url: "https://github.com/lht99/lhtPDF/releases/tag/v99.0.0".into(),
                msi_url: Some("https://github.com/lht99/lhtPDF/releases/download/v99.0.0/lhtPDF-99.0.0-windows-x64.msi".into()),
            })
        }));
        app
    });
    settle(&mut h);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    h.get_by_label_contains("lhtPDF 99.0.0 is available");
    h.get_by_label("Download MSI");
    h.get_by_label("Don't show today");

    // Further frames must not start additional background checks.
    h.run_steps(10);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn disabled_startup_check_still_allows_manual_checks() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();

    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(r#"{"check_updates_at_start": false}"#);

        let counted = counted.clone();
        app.update_source = Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Release {
                version: "v99.0.0".into(),
                url: "https://github.com/lht99/lhtPDF/releases/tag/v99.0.0".into(),
                msi_url: Some("https://github.com/lht99/lhtPDF/releases/download/v99.0.0/lhtPDF-99.0.0-windows-x64.msi".into()),
            })
        }));

        app
    });

    h.run_steps(10);

    assert_eq!(calls.load(Ordering::SeqCst), 0, "Disabled startup checks must not contact the update source");
    assert!(h.query_by_label_contains("is available").is_none(), "No automatic update dialog should appear");

    h.state_mut().execute("help.check_updates");
    settle(&mut h);

    assert_eq!(calls.load(Ordering::SeqCst), 1, "Manual checks must work even when startup checks are disabled");

    h.get_by_label_contains("lhtPDF 99.0.0 is available");
    h.get_by_label("Download MSI");
    assert!(h.query_by_label("Don't show today").is_none(), "Manual checks must not show the snooze button");
}
#[test]
fn snoozed_update_is_suppressed_until_another_day() {
    // The date comes from the same clock used by the production updater.
    let app = PdfCraftApp::new();
    let (year, month, day) = app.session.today();
    let today = format!("{year:04}-{month:02}-{day:02}");

    let settings = format!(r#"{{"check_updates_at_start":true,"update_snooze_until":"{today}"}}"#);

    // A previously snoozed release must not trigger a startup popup today.
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(&settings);
        app.update_source = Some(source(Ok("v99.0.0")));
        app
    });

    settle(&mut h);
    assert!(h.query_by_label_contains("lhtPDF 99.0.0 is available").is_none(), "A release snoozed today must not open an automatic popup");

    // An expired snooze must allow the same release to be offered again.
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(r#"{"check_updates_at_start":true,"update_snooze_until":"2000-01-01"}"#);
        app.update_source = Some(source(Ok("v99.0.0")));
        app
    });

    settle(&mut h);
    h.get_by_label_contains("lhtPDF 99.0.0 is available");
    h.get_by_label("Don't show today").click();
    h.run_steps(3);

    assert!(h.query_by_label_contains("is available").is_none(), "Snooze must close the update dialog");

    // Persisted state must retain the snooze date, not the notified tag.
    let saved = h.state().persist();
    let state: serde_json::Value = serde_json::from_str(&saved).expect("valid persisted settings");

    assert_eq!(state["update_snooze_until"].as_str(), Some(today.as_str()), "Snooze must persist the current date");
    assert!(state["last_notified_update"].is_null(), "Snoozing must not permanently mark the release as notified");
}
#[test]
fn automatic_notice_without_msi_can_be_snoozed() {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(r#"{"check_updates_at_start":true}"#);

        app.update_source = Some(Arc::new(|| {
            Ok(Release { version: "v99.0.0".into(), url: "https://github.com/lht99/lhtPDF/releases/tag/v99.0.0".into(), msi_url: None })
        }));

        app
    });

    settle(&mut h);

    h.get_by_label_contains("lhtPDF 99.0.0 is available");
    h.get_by_label("View release");
    h.get_by_label("Don't show today");
    h.get_by_label("Later");

    assert!(h.query_by_label("Download MSI").is_none(), "An unavailable MSI must not produce a download button");

    h.get_by_label("Don't show today").click();
    h.run_steps(3);

    assert!(h.query_by_label_contains("is available").is_none(), "Snoozing must close the automatic notice");

    let saved = h.state().persist();
    let state: serde_json::Value = serde_json::from_str(&saved).expect("valid persisted settings");

    let (year, month, day) = h.state().session.today();
    let today = format!("{year:04}-{month:02}-{day:02}");

    assert_eq!(state["update_snooze_until"].as_str(), Some(today.as_str()), "Snooze must save the current date");

    assert!(state["last_notified_update"].is_null(), "Snoozing must allow the release to be offered on a later day");
}
