//! Offline multilingual OCR using the Tesseract command-line engine.
//!
//! Tesseract writes UTF-8 TSV directly to disk. Avoid piping its Unicode
//! output through Windows PowerShell or a locale-dependent decoder.

use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Line, OcrError, Word};

static NEXT_JOB: AtomicU64 = AtomicU64::new(0);

struct TemporaryFiles {
    directory: PathBuf,
}

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("LHTPDF_TESSERACT") {
        return PathBuf::from(path);
    }

    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let bundled = parent.join("tesseract").join("tesseract.exe");
            if bundled.is_file() {
                return bundled;
            }
        }
    }

    #[cfg(windows)]
    {
        let installed = Path::new(r"C:\Program Files\Tesseract-OCR\tesseract.exe");
        if installed.is_file() {
            return installed.to_path_buf();
        }
    }

    PathBuf::from("tesseract")
}

fn parse_tsv(contents: &str) -> Result<Vec<Line>, OcrError> {
    let mut groups: Vec<(String, Line)> = Vec::new();

    for row in contents.lines().skip(1) {
        let columns: Vec<&str> = row.splitn(12, '\t').collect();
        if columns.len() != 12 || columns[0] != "5" {
            continue;
        }

        let text = columns[11].trim();
        if text.is_empty() {
            continue;
        }

        let number = |index: usize| columns[index].parse::<f32>().map_err(|e| OcrError::Recognize(format!("invalid Tesseract TSV coordinate: {e}")));

        let left = number(6)?;
        let top = number(7)?;
        let width = number(8)?;
        let height = number(9)?;

        if !left.is_finite() || !top.is_finite() || !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            continue;
        }

        let key = format!("{}:{}:{}:{}", columns[1], columns[2], columns[3], columns[4]);

        if groups.last().is_none_or(|(last, _)| last != &key) {
            groups.push((key, Line::default()));
        }

        if let Some((_, line)) = groups.last_mut() {
            line.words.push(Word { text: text.to_owned(), rect: [left, top, left + width, top + height] });
        }
    }

    Ok(groups.into_iter().map(|(_, line)| line).collect())
}

/// Recognize RGBA pixels with a specified installed Tesseract language.
///
/// `language` accepts `eng`, `vie`, `kor`, or combinations such as
/// `vie+eng`. The model must exist in Tesseract's tessdata directory.
/// Check that Tesseract and the requested language data are installed.

/// LHTPDF_OCR_NO_WINDOW: prevent Tesseract from opening a console window.
fn hide_console_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn tesseract_command() -> std::process::Command {
    let mut command = std::process::Command::new(executable());
    hide_console_window(&mut command);
    command
}
pub fn available(language: &str) -> bool {
    if language.is_empty() || !language.split('+').all(|part| matches!(part, "eng" | "vie" | "kor")) {
        return false;
    }

    static INSTALLED_LANGUAGES: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

    let installed = INSTALLED_LANGUAGES.get_or_init(|| {
        let output = tesseract_command().arg("--list-langs").output().ok()?;

        if !output.status.success() {
            return None;
        }

        Some(format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
    });

    let Some(installed) = installed else {
        return false;
    };

    language.split('+').all(|part| installed.lines().any(|line| line.trim() == part))
}
pub fn recognize(pixels: &[u8], width: u32, height: u32, language: &str) -> Result<Vec<Line>, OcrError> {
    if width == 0 || height == 0 || pixels.is_empty() {
        return Err(OcrError::EmptyImage);
    }

    let valid_language = !language.is_empty() && language.split('+').all(|part| matches!(part, "eng" | "vie" | "kor"));

    if !valid_language {
        return Err(OcrError::Recognize(format!("unsupported Tesseract language: {language}")));
    }

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| OcrError::Recognize("OCR image dimensions overflow".into()))?;

    if pixels.len() != expected {
        return Err(OcrError::Recognize("Tesseract OCR expects RGBA pixels".into()));
    }

    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!("lhtpdf-ocr-{}-{job}", std::process::id()));

    std::fs::create_dir(&directory).map_err(|e| OcrError::Recognize(e.to_string()))?;
    let _cleanup = TemporaryFiles { directory: directory.clone() };

    let input = directory.join("input.png");
    let output = directory.join("result");
    let tsv = directory.join("result.tsv");

    let image = image::RgbaImage::from_raw(width, height, pixels.to_vec()).ok_or_else(|| OcrError::Recognize("invalid RGBA image".into()))?;

    image.save(&input).map_err(|e| OcrError::Recognize(e.to_string()))?;

    let result = tesseract_command()
        .arg(&input)
        .arg(&output)
        .args(["-l", language, "--psm", "3", "-c", "tessedit_create_txt=0", "-c", "tessedit_create_tsv=1"])
        .output()
        .map_err(|e| OcrError::Recognize(format!("cannot start Tesseract: {e}")))?;

    if !result.status.success() {
        return Err(OcrError::Recognize(String::from_utf8_lossy(&result.stderr).into_owned()));
    }

    let contents = std::fs::read_to_string(&tsv).map_err(|e| OcrError::Recognize(format!("cannot read Tesseract TSV: {e}")))?;

    parse_tsv(&contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_tsv_preserves_vietnamese_and_korean() {
        let tsv = concat!(
            "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\t",
            "left\ttop\twidth\theight\tconf\ttext\n",
            "5\t1\t1\t1\t1\t1\t10\t20\t90\t30\t96\tTiếng\n",
            "5\t1\t1\t1\t1\t2\t110\t20\t100\t30\t97\tViệt\n",
            "5\t1\t1\t1\t2\t1\t10\t70\t150\t35\t95\t안녕하세요\n",
        );

        let lines = parse_tsv(tsv).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "Tiếng Việt");
        assert_eq!(lines[1].text(), "안녕하세요");
        assert_eq!(lines[0].words[0].rect, [10.0, 20.0, 100.0, 50.0]);
    }

    #[test]
    fn empty_tsv_has_no_words() {
        assert!(parse_tsv("level\tpage_num\n").unwrap().is_empty());
    }
}
