use pdfcraft_ocr::tesseract;

#[test]
#[ignore = "requires local Tesseract and lhtPDF-OCR-516 fixtures"]
fn recognizes_english_vietnamese_and_korean() {
    let root = std::env::temp_dir().join("lhtPDF-OCR-516");

    let cases = [("eng", "English OCR document"), ("vie", "Tiếng Việt có dấu"), ("kor", "안녕하세요 대한민국")];

    for (language, expected) in cases {
        let path = root.join(format!("{language}.png"));

        assert!(path.is_file(), "Missing test image: {}", path.display());

        let image = image::open(&path).unwrap_or_else(|e| panic!("Cannot open {}: {e}", path.display())).to_rgba8();

        let lines = tesseract::recognize(image.as_raw(), image.width(), image.height(), language)
            .unwrap_or_else(|e| panic!("OCR failed for {language}: {e}"));

        let recognized = lines.iter().map(|line| line.text()).collect::<Vec<_>>().join(" ");

        println!("{language}: {recognized}");

        assert_eq!(recognized, expected, "Incorrect Unicode OCR output for {language}");

        assert!(
            lines.iter().flat_map(|line| &line.words).all(|word| { word.rect[2] > word.rect[0] && word.rect[3] > word.rect[1] }),
            "Invalid OCR coordinates for {language}"
        );
    }
}
