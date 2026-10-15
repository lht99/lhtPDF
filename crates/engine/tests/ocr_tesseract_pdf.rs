use std::sync::Arc;

use pdfcraft_engine::Session;
use pdfcraft_engine::ocr::OcrSettings;

#[test]
#[ignore = "requires local Tesseract and lhtPDF-OCR-516 fixtures"]
fn tesseract_multilingual_pdf_roundtrip() {
    let root = std::env::temp_dir().join("lhtPDF-OCR-516");

    let cases = [("eng", "English OCR document"), ("vie", "Tiếng Việt có dấu"), ("kor", "안녕하세요 대한민국")];

    for (language, expected) in cases {
        let image_path = root.join(format!("{language}.png"));
        let png = std::fs::read(&image_path).unwrap_or_else(|e| panic!("Cannot read {}: {e}", image_path.display()));

        let mut session = Session::new();

        let pdf = session.create_from_images(&[(format!("{language}.png"), png)]).expect("Cannot create scanned PDF");

        let id = session.open(format!("{language}.pdf"), None, pdf, None).expect("Cannot open scanned PDF");

        let settings = OcrSettings { language: language.into(), dpi: 300.0, skip_text_pages: true };

        let pages = session.recognize_text(id, &[], settings).unwrap_or_else(|e| panic!("OCR failed for {language}: {e}"));

        let recognized = pages.iter().map(|page| page.text()).collect::<Vec<_>>().join(" ");

        println!("{language} OCR: {recognized}");

        assert_eq!(recognized, expected, "Tesseract recognition mismatch for {language}");

        assert!(pages.iter().any(|page| !page.words.is_empty()), "No OCR words for {language}");

        let saved = session.save_bytes(id).expect("Cannot save searchable PDF");

        assert!(saved.len() > 100, "Saved PDF unexpectedly small for {language}");

        let mut reopened = Session::new();

        let reopened_id = reopened.open(format!("{language}-searchable.pdf"), None, Arc::clone(&saved), None).expect("Cannot reopen searchable PDF");

        let doc = reopened.get(reopened_id).unwrap();

        let extracted = doc.text_lines(0).iter().map(|line| line.text.clone()).collect::<Vec<_>>().join(" ");

        println!("{language} PDF: {extracted}");

        assert!(extracted.contains(expected), "Unicode text missing after PDF save/reopen for {language}: {extracted:?}");

        let blocks = doc.text_blocks(0);

        assert!(!blocks.is_empty(), "No searchable text blocks for {language}");

        assert!(blocks.iter().all(|block| block.invisible), "OCR text must remain invisible for {language}");
    }
}
