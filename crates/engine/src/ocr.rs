//! Scan & OCR ▸ Recognize text: render pages, read the words in them and add an invisible text
//! layer ([`Edit::AddOcrText`]), making scanned pages searchable and selectable.
//!
//! Recognition is slow (about a second a page), so it is split from the edit: [`Session::ocr_job`]
//! captures what it needs, [`OcrJob::run`] works anywhere (the UI runs it on a worker thread) and
//! [`Session::apply_ocr`] applies the result as one undoable step.

use std::sync::{Arc, Mutex};

use pdfcraft_render::{PageInfo, PageRenderer, RenderConfig, RenderRequest, RequestKind};

pub use pdfcraft_ocr::{LANGUAGES, Models, Ocr, OcrError, PlacedWord};

use crate::{DocId, Edit, EditError, Session};

/// Recognize Text settings.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrSettings {
    /// Resolution pages are rendered at for recognition (Acrobat's "Downsample to" choices).
    pub dpi: f32,
    /// Language code from [`LANGUAGES`]; multilingual selections use Tesseract.
    pub language: String,
    /// Leave pages that already have text alone (Acrobat reports "page contains renderable
    /// text" and skips them). When `false`, such pages are read too (a picture on a page that
    /// also has text, #1028) and only the words that aren't already there are added.
    pub skip_text_pages: bool,
}

/// A recognised word whose box is at least this much covered by the page's own text is that
/// text read again, not a word in a picture.
const ALREADY_ON_PAGE: f32 = 0.5;

/// Whether native Unicode is reliable enough to suppress overlapping OCR.
///
/// This intentionally detects only clear encoding defects. It does not
/// guess whether otherwise valid Unicode matches the visible glyph.
fn reliable_native_text(text: &str) -> bool {
    let mut has_visible = false;
    for c in text.chars() {
        let u = c as u32;
        if c == '\u{fffd}'
            || c.is_control()
            || (0xe000..=0xf8ff).contains(&u)
            || (0xf0000..=0xffffd).contains(&u)
            || (0x100000..=0x10fffd).contains(&u)
        {
            return false;
        }
        if !c.is_whitespace() {
            has_visible = true;
        }
    }
    has_visible
}

/// How much of `word` the `text` boxes cover, from 0 to 1. Boxes are `[x0, y0, x1, y1]` in page
/// view space. Overlaps between the text boxes are counted twice (glyphs rarely overlap), so the
/// sum is capped at the whole word.
fn covered(word: [f32; 4], text: &[[f32; 4]]) -> f32 {
    let [x0, y0, x1, y1] = word;
    if !word.iter().all(|v| v.is_finite()) || x1 <= x0 || y1 <= y0 {
        return 0.0;
    }

    let total = (x1 - x0) * (y1 - y0);
    if !total.is_finite() || total <= 0.0 {
        return 0.0;
    }

    let rects: Vec<[f32; 4]> = text
        .iter()
        .filter(|r| r.iter().all(|v| v.is_finite()))
        .filter_map(|r| {
            let a = x0.max(r[0]);
            let b = y0.max(r[1]);
            let c = x1.min(r[2]);
            let d = y1.min(r[3]);
            (c > a && d > b).then_some([a, b, c, d])
        })
        .collect();

    if rects.is_empty() {
        return 0.0;
    }

    let mut xs: Vec<f32> = rects.iter().flat_map(|r| [r[0], r[2]]).collect();
    xs.sort_by(f32::total_cmp);
    xs.dedup();

    let mut area = 0.0f32;

    for pair in xs.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if right <= left {
            continue;
        }

        let mut intervals: Vec<(f32, f32)> = rects.iter().filter(|r| r[0] < right && r[2] > left).map(|r| (r[1], r[3])).collect();

        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut covered_y = 0.0f32;
        let mut active: Option<(f32, f32)> = None;

        for (top, bottom) in intervals {
            match active {
                Some((a, b)) if top <= b => {
                    active = Some((a, b.max(bottom)));
                }
                Some((a, b)) => {
                    covered_y += b - a;
                    active = Some((top, bottom));
                }
                None => active = Some((top, bottom)),
            }
        }

        if let Some((a, b)) = active {
            covered_y += b - a;
        }

        area += (right - left) * covered_y;
    }

    (area / total).clamp(0.0, 1.0)
}
impl Default for OcrSettings {
    fn default() -> Self {
        OcrSettings { dpi: 300.0, language: "en".into(), skip_text_pages: true }
    }
}

/// What recognition found on one page.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrPage {
    pub page: usize,
    pub words: Vec<PlacedWord>,
    /// Words recognised but left out because the page's own text already has them there.
    pub already_on_page: usize,
    /// Why the page was not read, if it was skipped.
    pub skipped: Option<String>,
}

impl OcrPage {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// The recogniser, loaded once (it takes a moment) and shared.
pub fn engine() -> Result<Arc<Ocr>, String> {
    static OCR: Mutex<Option<Arc<Ocr>>> = Mutex::new(None);
    let mut slot = OCR.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = slot.as_ref() {
        return Ok(o.clone());
    }
    let o = Arc::new(Ocr::find().map_err(|e| e.to_string())?);
    *slot = Some(o.clone());
    Ok(o)
}

/// Whether the recognition models are installed.
pub fn available() -> bool {
    Models::find().is_some()
}

/// Whether the selected OCR backend is available.
pub fn available_for(language: &str) -> bool {
    if language == "en" {
        return available();
    }
    pdfcraft_ocr::tesseract::available(language)
}
/// Everything recognition needs from a document, detached from the session.
pub struct OcrJob {
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    infos: Vec<PageInfo>,
    /// Per page: the render scale (pixels per point) that shows its images at their own
    /// resolution, if it has any.
    native: Vec<Option<f32>>,
    pub pages: Vec<usize>,
    pub settings: OcrSettings,
}

impl OcrJob {
    /// Read the pages. `progress(done, total)` is called before each page; returning `false`
    /// stops (the pages read so far are returned).
    pub fn run(self, ocr: &Ocr, progress: impl FnMut(usize, usize) -> bool) -> Vec<OcrPage> {
        self.run_with_optional_ocr(Some(ocr), progress)
    }

    /// Run multilingual Tesseract without requiring the English RTEN models.
    pub fn run_with_optional_ocr(self, ocr: Option<&Ocr>, mut progress: impl FnMut(usize, usize) -> bool) -> Vec<OcrPage> {
        let config = RenderConfig { password: self.password.as_deref().map(Arc::from), ..Default::default() };
        let mut r = PageRenderer::new(self.bytes.clone(), config);
        let mut out = Vec::new();
        let total = self.pages.len();
        for (done, &page) in self.pages.iter().enumerate() {
            if !progress(done, total) {
                break;
            }
            let Some(info) = self.infos.get(page) else { continue };
            let skip = |why: &str| OcrPage { page, words: Vec::new(), already_on_page: 0, skipped: Some(why.into()) };
            // The boxes of the page's own text (an earlier OCR layer's included), in view space.
            let existing: Vec<[f32; 4]> = r
                .render(RenderRequest { page, kind: RequestKind::Text, scale: 1.0, ..Default::default() })
                .text
                .map(|t| {
                    // A broken ToUnicode mapping invalidates the whole native word.
                    // Do not let its remaining valid glyphs suppress OCR recovery.
                    let mut reliable = vec![false; t.glyphs.len()];
                    let mut start = 0usize;

                    while start < t.glyphs.len() {
                        let mut end = start + 1;

                        while end < t.glyphs.len() && t.line_of.get(end) == t.line_of.get(start) && !t.space_before.get(end).copied().unwrap_or(true)
                        {
                            end += 1;
                        }

                        let valid = t.glyphs[start..end].iter().all(|g| g.text.chars().all(char::is_whitespace) || reliable_native_text(&g.text));

                        if valid {
                            reliable[start..end].fill(true);
                        }

                        start = end;
                    }

                    t.glyphs.iter().zip(reliable).filter_map(|(g, valid)| valid.then_some(g.rect)).collect()
                })
                .unwrap_or_default();
            if self.settings.skip_text_pages && !existing.is_empty() {
                out.push(skip("the page already contains text"));
                continue;
            }
            // A scan is read at its own resolution (enlarging it only blurs the letters), at most
            // the chosen one.
            let wanted = self.settings.dpi.clamp(72.0, 600.0) / 72.0;
            let scale = self.native.get(page).copied().flatten().map_or(wanted, |n| n.clamp(1.0, wanted));
            let shot = r.render(RenderRequest { page, scale, ..Default::default() });
            if let Some(e) = shot.error {
                out.push(skip(&e));
                continue;
            }
            let recognition = if self.settings.language == "en" {
                match ocr {
                    Some(ocr) => ocr.recognize(&shot.rgba, shot.width, shot.height),
                    None => Err(OcrError::NoModels),
                }
            } else {
                pdfcraft_ocr::tesseract::recognize(&shot.rgba, shot.width, shot.height, &self.settings.language)
            };
            let lines = match recognition {
                Ok(l) => l,
                Err(e) => {
                    out.push(skip(&e.to_string()));
                    continue;
                }
            };
            let scale = shot.width as f32 / info.width.max(1e-3);
            let to_user = |x: f32, y: f32| {
                let [u, v] = info.view_to_user(x / scale, y / scale);
                [u as f64, v as f64]
            };
            // Words the page's own text already has are not added a second time.
            let (fresh, already): (Vec<_>, Vec<_>) =
                lines.iter().flat_map(|l| &l.words).partition(|w| covered(w.rect.map(|v| v / scale), &existing) < ALREADY_ON_PAGE);
            let words = fresh.into_iter().map(|w| PlacedWord::place(w, to_user)).collect();
            out.push(OcrPage { page, words, already_on_page: already.len(), skipped: None });
        }
        progress(total, total);
        out
    }
}

impl Session {
    /// Capture what recognising `pages` (0-based; empty = all) of document `id` needs.
    pub fn ocr_job(&self, id: DocId, pages: &[usize], settings: OcrSettings) -> Option<OcrJob> {
        let doc = self.get(id)?;
        let all = doc.info.pages.len();
        let pages: Vec<usize> = if pages.is_empty() { (0..all).collect() } else { pages.iter().copied().filter(|p| *p < all).collect() };
        let native = (0..all)
            .map(|p| {
                if !pages.contains(&p) {
                    return None;
                }
                doc.page_images(p)
                    .iter()
                    .filter(|i| i.width > 1 && i.height > 1)
                    .map(|i| {
                        let m = i.matrix;
                        let (w, h) = (m[0].hypot(m[1]), m[2].hypot(m[3]));
                        (i.width as f64 / w.max(1e-6)).max(i.height as f64 / h.max(1e-6)) as f32
                    })
                    .reduce(f32::max)
            })
            .collect();
        Some(OcrJob { bytes: doc.bytes.clone(), password: doc.password.clone(), infos: doc.info.pages.clone(), native, pages, settings })
    }

    /// Add the words found by [`OcrJob::run`] as one undoable step. Returns the number of words.
    pub fn apply_ocr(&mut self, id: DocId, found: &[OcrPage]) -> Result<usize, EditError> {
        let edits: Vec<Edit> =
            found.iter().filter(|p| !p.words.is_empty()).map(|p| Edit::AddOcrText { page: p.page, words: p.words.clone() }).collect();
        let words = found.iter().map(|p| p.words.len()).sum();
        if !edits.is_empty() {
            self.apply(id, Edit::Batch { label: "Recognize text".into(), edits })?;
        }
        Ok(words)
    }

    /// Recognise text on `pages` and add it, in one call (the CLI and agents use this).
    pub fn recognize_text(&mut self, id: DocId, pages: &[usize], settings: OcrSettings) -> Result<Vec<OcrPage>, String> {
        let job = self.ocr_job(id, pages, settings).ok_or("no such document")?;
        if let Some(why) = self.get(id).and_then(|d| d.read_only_reason.clone()) {
            return Err(why);
        }
        let ocr = if job.settings.language == "en" { Some(engine()?) } else { None };
        let found = job.run_with_optional_ocr(ocr.as_deref(), |_, _| true);
        self.apply_ocr(id, &found).map_err(|e| e.to_string())?;
        Ok(found)
    }
}

/// What [`recognize_file`] did to one file.
#[derive(Clone, Debug, PartialEq)]
pub struct FileResult {
    /// The new file (incrementally saved), or the original when nothing was recognised.
    pub bytes: Arc<Vec<u8>>,
    pub pages: Vec<OcrPage>,
}

impl FileResult {
    pub fn words(&self) -> usize {
        self.pages.iter().map(|p| p.words.len()).sum()
    }
}

/// Recognize a file using the backend selected by its language.
pub fn recognize_file_auto(
    name: &str,
    bytes: Arc<Vec<u8>>,
    password: Option<&str>,
    settings: OcrSettings,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<FileResult, String> {
    let ocr = if settings.language == "en" { Some(engine()?) } else { None };
    recognize_file_optional(name, bytes, password, settings, ocr.as_deref(), progress)
}

fn recognize_file_optional(
    name: &str,
    bytes: Arc<Vec<u8>>,
    password: Option<&str>,
    settings: OcrSettings,
    ocr: Option<&Ocr>,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<FileResult, String> {
    let mut s = Session::new();
    let id = s.open(name, None, bytes.clone(), password).map_err(|e| e.to_string())?;
    if let Some(why) = s.get(id).and_then(|d| d.read_only_reason.clone()) {
        return Err(why);
    }
    let job = s.ocr_job(id, &[], settings).ok_or("the document could not be read")?;
    let pages = job.run_with_optional_ocr(ocr, progress);
    if s.apply_ocr(id, &pages).map_err(|e| e.to_string())? == 0 {
        return Ok(FileResult { bytes, pages });
    }
    Ok(FileResult { bytes: s.save_bytes(id).map_err(|e| e.to_string())?, pages })
}
/// Recognize text in multiple files: one PDF's bytes in, the searchable PDF out. `progress`
/// works as for [`OcrJob::run`].
pub fn recognize_file(
    name: &str,
    bytes: Arc<Vec<u8>>,
    password: Option<&str>,
    settings: OcrSettings,
    ocr: &Ocr,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<FileResult, String> {
    let mut s = Session::new();
    let id = s.open(name, None, bytes.clone(), password).map_err(|e| e.to_string())?;
    if let Some(why) = s.get(id).and_then(|d| d.read_only_reason.clone()) {
        return Err(why);
    }
    let job = s.ocr_job(id, &[], settings).ok_or("the document could not be read")?;
    let pages = job.run_with_optional_ocr(Some(ocr), progress);
    if s.apply_ocr(id, &pages).map_err(|e| e.to_string())? == 0 {
        return Ok(FileResult { bytes, pages });
    }
    Ok(FileResult { bytes: s.save_bytes(id).map_err(|e| e.to_string())?, pages })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1028: a recognised word over the page's own text is that text again; one in an empty
    /// part of the page (a picture) is new.
    #[test]
    fn unicode_quality_distinguishes_valid_and_broken_words() {
        // Korean and Vietnamese text must not be rejected just for being non-Latin.
        for word in ["GOOD", "안녕하세요", "Tiếng Việt", "ﬁ"] {
            assert!(reliable_native_text(word), "valid word: {word}");
        }

        // An invalid code point anywhere inside a word makes its native
        // Unicode unreliable, even when the remaining letters are valid.
        for word in ["�ROKEN", "BRO�EN", "BROKEN�", "한�글"] {
            assert!(!reliable_native_text(word), "broken word: {word}");
        }

        // The neighboring word is evaluated independently.
        assert!(reliable_native_text("GOOD"));
        assert!(!reliable_native_text("�ROKEN"));
        assert!(reliable_native_text("NEXT"));
    }
    #[test]
    fn native_unicode_quality_preserves_korean_and_ligatures() {
        assert!(reliable_native_text("한글 테스트"));
        assert!(reliable_native_text("한국어"));
        assert!(reliable_native_text("ffi"));
        assert!(reliable_native_text("가\u{0301}"));
        assert!(!reliable_native_text(""));
        assert!(!reliable_native_text("   "));
    }

    #[test]
    fn native_unicode_quality_rejects_unreliable_mappings() {
        assert!(!reliable_native_text("\u{fffd}"));
        assert!(!reliable_native_text("한\u{fffd}글"));
        assert!(!reliable_native_text("\u{e000}"));
        assert!(!reliable_native_text("\u{f0000}"));
        assert!(!reliable_native_text("\u{100000}"));
        assert!(!reliable_native_text("A\u{0001}B"));
    }
    #[test]
    fn words_over_existing_text_count_as_already_there() {
        // Glyph em boxes of "Total" at 12 pt, five letters side by side.
        let glyphs: Vec<[f32; 4]> = (0..5).map(|i| [100.0 + 7.0 * i as f32, 50.0, 107.0 + 7.0 * i as f32, 62.0]).collect();
        // OCR's tight box around the same ink.
        assert!(covered([100.5, 52.0, 134.0, 61.0], &glyphs) >= ALREADY_ON_PAGE);
        // A word in a picture further down, and one beside the text on the same line.
        assert_eq!(covered([100.0, 200.0, 140.0, 212.0], &glyphs), 0.0);
        assert_eq!(covered([140.0, 52.0, 170.0, 61.0], &glyphs), 0.0);
        // Touching the text's last letter is not enough.
        assert!(covered([131.0, 52.0, 170.0, 61.0], &glyphs) < ALREADY_ON_PAGE);
        // A page without text has nothing to cover a word.
        assert_eq!(covered([100.5, 52.0, 134.0, 61.0], &[]), 0.0);
    }

    #[test]
    fn image_regions_follow_page_rotation_and_crop() {
        // A nonzero CropBox origin catches accidental use of raw PDF
        // coordinates as top-left-origin view coordinates.
        for rotation in [0, 90, 180, 270] {
            let page = PageInfo {
                width: if rotation % 180 == 0 { 200.0 } else { 300.0 },
                height: if rotation % 180 == 0 { 300.0 } else { 200.0 },
                label: "1".into(),
                crop: [10.0, 20.0, 210.0, 320.0],
                rotation,
            };

            let corners = [[40.0, 70.0], [110.0, 70.0], [40.0, 150.0], [110.0, 150.0]];

            for [x, y] in corners {
                let [vx, vy] = page.user_to_view(x, y);
                let [ux, uy] = page.view_to_user(vx, vy);

                assert!((ux - x).abs() < 1e-3, "rotation={rotation}, x={x}, got={ux}");
                assert!((uy - y).abs() < 1e-3, "rotation={rotation}, y={y}, got={uy}");
                assert!(vx >= 0.0 && vx <= page.width);
                assert!(vy >= 0.0 && vy <= page.height);
            }
        }
    }
    #[test]
    fn overlapping_glyphs_do_not_hide_new_ocr_words() {
        // Both glyph boxes describe the same 40% of the OCR word.
        // Summing their areas would incorrectly reach 80% and discard it.
        let glyphs = [[0.0, 0.0, 4.0, 10.0], [0.0, 0.0, 4.0, 10.0]];
        let word = [0.0, 0.0, 10.0, 10.0];

        let ratio = covered(word, &glyphs);
        assert!((ratio - 0.4).abs() < 1e-6, "{ratio}");
        assert!(ratio < ALREADY_ON_PAGE);
    }

    #[test]
    fn adjacent_glyphs_cover_a_word_without_double_counting() {
        let glyphs = [[0.0, 0.0, 3.0, 10.0], [3.0, 0.0, 7.0, 10.0], [7.0, 0.0, 10.0, 10.0]];
        let word = [0.0, 0.0, 10.0, 10.0];

        assert!((covered(word, &glyphs) - 1.0).abs() < 1e-6);
    }
    #[test]
    fn coverage_of_overlapping_or_broken_boxes_stays_between_zero_and_one() {
        // A glyph drawn twice (fake bold) does not cover more than the whole word.
        let twice = [[0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 10.0, 10.0]];
        assert_eq!(covered([0.0, 0.0, 10.0, 10.0], &twice), 1.0);
        // Empty, inverted and non-finite boxes cover nothing and are covered by nothing.
        for word in [[5.0, 5.0, 5.0, 9.0], [9.0, 0.0, 1.0, 9.0], [f32::NAN, 0.0, 9.0, 9.0], [0.0, 0.0, f32::INFINITY, 9.0]] {
            assert_eq!(covered(word, &twice), 0.0, "{word:?}");
        }
        let broken = [[f32::NAN; 4], [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY]];
        let c = covered([0.0, 0.0, 10.0, 10.0], &broken);
        assert!((0.0..=1.0).contains(&c), "{c}");
    }
}
