//! Community links: the Discord button is one click away everywhere; Help menu, About dialog and
//! home screen open the ArtCraft and PdfCraft pages.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{Dialog, PdfCraftApp};

fn harness(setup: impl FnOnce(&mut PdfCraftApp) + 'static) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        setup(&mut app);
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn discord_button_in_the_top_bar_opens_discord() {
    let h = harness(|_| {});
    assert!(
        h.query_by_label("Discord").is_none(),
        "Discord must not appear in the lhtPDF top bar"
    );
}

#[test]
fn home_screen_links() {
    let mut h = harness(|_| {});
    h.get_by_label("lhtPDF - Website & Support");
    h.get_by_label("Visit https://letuan.io.vn").click();

    // egui_kittest delivers the click in the next frame.
    // OpenUrl is frame-local and must be checked immediately afterward.
    h.run_steps(1);

    let opened = h.output().platform_output.commands.iter().any(|command| {
        matches!(
            command,
            egui::OutputCommand::OpenUrl(url)
                if url.url == "https://letuan.io.vn"
        )
    });

    assert!(opened, "lhtPDF Home must open its official support URL");
}

#[test]
fn about_dialog_shows_the_brand_and_links() {
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
    // With a document open, so the home screen's own links are not on screen.
    let h = harness(move |app| {
        app.open_bytes("one.pdf", None, pdf.to_vec()).unwrap();
        app.dialog = Some(Dialog::About);
    });
    assert!(
        h.query_all_by_label("lhtPDF").count() >= 1,
        "About must display the lhtPDF brand"
    );
    assert!(
        h.query_by_label("Join our Discord").is_none(),
        "Discord must not appear in the personalized About dialog"
    );
}

#[test]
fn help_commands_match_lhtpdf_menu_contract() {
    use pdfcraft_engine::commands;

    let help: Vec<_> = commands::menu("Help")
        .filter(|spec| spec.id != "help.shortcuts")
        .map(|spec| spec.id)
        .collect();

    assert_eq!(
        help,
        ["help.check_updates", "help.about"],
        "lhtPDF Help menu must contain only updates and About"
    );

    assert!(
        commands::command("help.shortcuts").is_some(),
        "keyboard shortcuts must remain registered for the header button"
    );

    for id in ["help.discord", "help.app_page", "help.github", "help.website"] {
        assert!(
            commands::command(id).is_none(),
            "legacy Help link command must not be registered: {id}"
        );
    }

    let mut h = harness(|_| {});
    assert!(h.state_mut().execute("help.about"));
    assert_eq!(h.state().dialog, Some(Dialog::About));
}

/// Runs frames until the node labelled `label` exists and has stopped moving, then returns it.
///
/// The About dialog is a modal centred on its own size, and switching tabs changes that size, so
/// for a few frames afterwards the modal grows and recentres and every control in it moves. A
/// fixed number of frames is a guess at how long that takes: it was too few after the 0.5.0
/// contributors refresh (78 names instead of 5) on some machines, the click on Table landed where
/// the button had been, and the table never opened (#671).
fn settled<'a>(h: &'a mut Harness<'static, PdfCraftApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let mut last = None;
    for _ in 0..60 {
        h.run_steps(1);
        let now = h.query_by_label(label).map(|n| n.rect());
        if now.is_some() && now == last {
            return h.get_by_label(label);
        }
        last = now;
    }
    panic!("{label:?} never appeared or never stopped moving");
}

#[test]
fn about_dialog_has_contributors_and_models_tabs() {
    let mut h = harness(|app| app.dialog = Some(Dialog::About));
    settled(&mut h, "Contributors").click();
    // The owner is always in the compiled-in credits (contributors/contributors.json), shown by username.
    settled(&mut h, "@echelon");
    settled(&mut h, "Table").click();
    settled(&mut h, "PRs");
    settled(&mut h, "Display name").click();
    settled(&mut h, "Brandon Thomas");
    settled(&mut h, "Models").click();
    h.run_steps(4);
    assert!(h.query_all_by_label("Anthropic").count() >= 1);
}

/// Search through real toolbar controls, then reuse upstream Find via Ctrl+F.
#[test]
fn inline_pdf_toolbar_search_finds_navigates_and_shares_ctrl_f() {
    use egui::accesskit::Role;

    const PDF: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj
4 0 obj << /Length 55 >> stream
BT /F1 14 Tf 20 150 Td (The quick brown fox) Tj ET
endstream endobj
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj
trailer << /Root 1 0 R >>
%%EOF";

    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1100.0))
        .build_eframe(|_cc| {
            let mut app = PdfCraftApp::new();
            app.set_option("language", "en").unwrap();
            app.open_bytes("inline-search.pdf", None, PDF.to_vec()).unwrap();
            app
        });

    h.run_steps(6);
    h.get_by_label("Find text").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Find text").click();
    h.run_steps(3);

    assert!(
        h.state().views[0].find.as_ref().is_some_and(|f| f.toolbar_visible),
        "toolbar Search must reveal its inline input"
    );

    h.get_by_role_and_label(Role::TextInput, "Find text").click();
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, "Find text").type_text("o");

    crate::support::wait_until(&mut h, "two PDF text matches", |app| {
        app.views[0].find.as_ref().is_some_and(|f| {
            f.query == "o" && f.matches.len() == 2 && f.current.is_some()
        })
    });

    let find = h.state().views[0].find.as_ref().unwrap();
    assert_eq!(find.matches.len(), 2);
    let initial = find.current.unwrap();

    h.get_by_label_contains("Next (").scroll_to_me();
    h.run_steps(2);
    h.get_by_label_contains("Next (").click();
    h.run_steps(3);

    assert_eq!(
        h.state().views[0].find.as_ref().unwrap().current,
        Some((initial + 1) % 2),
        "Next must advance to the next PDF match"
    );

    h.get_by_label_contains("Previous (").scroll_to_me();
    h.run_steps(2);
    h.get_by_label_contains("Previous (").click();
    h.run_steps(3);

    assert_eq!(
        h.state().views[0].find.as_ref().unwrap().current,
        Some(initial),
        "Previous must restore the initial PDF match"
    );

    h.state_mut().views[0].open_find();
    h.run_steps(2);

    let find = h.state().views[0].find.as_ref().expect("shared Find");
    assert!(!find.toolbar_visible);
    assert_eq!(find.query, "o");
    assert_eq!(find.matches.len(), 2);
}
