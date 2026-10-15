use pdfcraft_engine::ocr::PlacedWord;
use pdfcraft_engine::{Edit, Session, export};

fn word(text: &str, x: f64, y: f64) -> PlacedWord {
    PlacedWord { text: text.into(), origin: [x, y], across: [110.0, 0.0], up: [0.0, 16.0] }
}

#[test]
fn ocr_unicode_roundtrips_vietnamese_korean_and_english() {
    let rgba = vec![255u8; 400 * 160 * 4];
    let png = export::encode_png(400, 160, &rgba).unwrap();

    let mut session = Session::new();
    let pdf = session.create_from_images(&[("unicode-scan.png".into(), png)]).unwrap();

    let id = session.open("unicode-scan.pdf", None, pdf, None).unwrap();

    let expected = ["Tiếng Việt có dấu", "안녕하세요", "English OCR"];

    let words = vec![word(expected[0], 20.0, 120.0), word(expected[1], 20.0, 90.0), word(expected[2], 20.0, 60.0)];

    session.apply(id, Edit::AddOcrText { page: 0, words }).unwrap();

    let lines = session.get(id).unwrap().text_lines(0);
    let extracted = lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join(" ");

    for text in expected {
        assert!(extracted.contains(text), "OCR Unicode text was not preserved: {text:?}; extracted: {extracted:?}");
    }

    let blocks = session.get(id).unwrap().text_blocks(0);
    assert!(blocks.iter().all(|block| block.invisible), "OCR text must remain invisible over the scan");
}
