use pdfcraft_fonts::embedding::{covers, validate_document_input};

const VIETNAMESE: &[u8] = include_bytes!("../../../assets/ocr-fonts/BeVietnamPro-Regular.ttf");

const KOREAN: &[u8] = include_bytes!("../../../assets/ocr-fonts/GowunDodum-Regular.ttf");

#[test]
fn vietnamese_ocr_font_is_valid_and_covers_text() {
    validate_document_input(VIETNAMESE).unwrap();

    assert!(covers(VIETNAMESE, "Tiếng Việt có dấu"));
    assert!(covers(VIETNAMESE, "English OCR"));
}

#[test]
fn korean_ocr_font_is_valid_and_covers_text() {
    validate_document_input(KOREAN).unwrap();

    assert!(covers(KOREAN, "안녕하세요"));
    assert!(covers(KOREAN, "English OCR"));
}
