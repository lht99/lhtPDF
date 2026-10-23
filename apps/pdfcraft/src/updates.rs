//! Asks GitHub for the latest lhtPDF release (Help ▸ Check for updates, issue #28).

use std::time::Duration;

use pdfcraft_ui_egui::updates::{RELEASES_PAGE, Release};

const LATEST: &str = "https://api.github.com/repos/lht99/lhtPDF/releases/latest";

/// The latest release. The answer is untrusted: its size is capped, and only a page under
/// [`RELEASES_PAGE`] is ever offered for download (anything else falls back to that list).
pub fn latest_release() -> Result<Release, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("lhtPDF/", env!("CARGO_PKG_VERSION")))
        // Read error statuses ourselves: a 403 from GitHub usually means its anonymous
        // rate limit ran out, and the retry time rides along in a response header.
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    let status = response.status().as_u16();
    if status == 403 {
        let reset = response.headers().get("x-ratelimit-reset").and_then(|v| v.to_str().ok());
        return Err(rate_limited(reset, now_unix()));
    }
    if !(200..300).contains(&status) {
        return Err(format!("couldn't reach GitHub (http status: {status})"));
    }
    let body = response.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("unreadable answer ({e})"))?;
    parse(&body)
}

/// The certificate authorities the operating system trusts.
fn os_roots() -> Result<ureq::tls::RootCerts, String> {
    let found = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("no trusted certificates found on this system".into());
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certs))
}

/// The current Unix time in seconds (0 when the clock gives nothing usable).
fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What to tell the user when GitHub answers 403: its anonymous quota (60 checks an hour,
/// shared by everyone behind the same address) ran out. `reset` is the `x-ratelimit-reset`
/// header, the Unix time the quota refills; `now` is the current Unix time. Anything
/// surprising (no header, not a number, already past) falls back to asking for patience.
fn rate_limited(reset: Option<&str>, now: u64) -> String {
    const FALLBACK: &str = "GitHub is limiting update checks for now (shared hourly quota); try again later";
    let wait = reset.and_then(|r| r.trim().parse::<u64>().ok()).and_then(|reset| reset.checked_sub(now)).filter(|&wait| wait > 0);
    match wait {
        Some(wait) => {
            let minutes = wait.div_ceil(60).max(1);
            let plural = if minutes == 1 { "" } else { "s" };
            format!("GitHub is limiting update checks for now (shared hourly quota); try again in about {minutes} minute{plural}")
        }
        None => FALLBACK.into(),
    }
}

fn parse(body: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("unreadable answer ({e})"))?;
    let version = v["tag_name"].as_str().filter(|t| !t.is_empty() && t.len() <= 64).ok_or("no release found")?.to_string();

    let url = v["html_url"]
        .as_str()
        .filter(|u| u.strip_prefix(RELEASES_PAGE).is_some_and(|rest| {
            rest.starts_with("/tag/")
                && !rest.contains(['?', '#', '\\'])
                && !rest.contains("..")
                && !rest.contains("//")
        }))
        .unwrap_or(RELEASES_PAGE)
        .to_string();

    let name = v["name"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect();

    let notes = v["body"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(16_000)
        .collect();

    let expected_prefix = format!("{RELEASES_PAGE}/download/{version}/");
    let msi_url = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            let filename = asset["name"].as_str()?;
            let download = asset["browser_download_url"].as_str()?;

            if !filename.starts_with("lhtPDF-")
                || !filename.ends_with("-windows-x64.msi")
                || !filename.contains(&format!("lhtPDF-{version}-"))
                || filename.contains(['/', '\\', '?', '#'])
            {
                return None;
            }

            let expected_url = format!("{expected_prefix}{filename}");
            (download == expected_url).then_some(download.to_string())
        })
        .next();

    Ok(Release {
        version,
        url,
        name,
        notes,
        msi_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_and_only_our_release_pages_are_offered() {
        let r = parse(r#"{"tag_name":"26.10.25","name":"lhtPDF 26.10.25","body":"Release notes","html_url":"https://github.com/lht99/lhtPDF/releases/tag/26.10.25","assets":[{"name":"lhtPDF-26.10.25-windows-x64.msi","browser_download_url":"https://github.com/lht99/lhtPDF/releases/download/26.10.25/lhtPDF-26.10.25-windows-x64.msi"}]}"#).unwrap();
        assert_eq!(r, Release {
            version: "26.10.25".into(),
            url: "https://github.com/lht99/lhtPDF/releases/tag/26.10.25".into(),
            name: "lhtPDF 26.10.25".into(),
            notes: "Release notes".into(),
            msi_url: Some("https://github.com/lht99/lhtPDF/releases/download/26.10.25/lhtPDF-26.10.25-windows-x64.msi".into()),
        });
        for elsewhere in ["https://example.com/pdfcraft.exe", "https://github.com/lht99/lhtPDF/releases.evil/x", "javascript:alert(1)"] {
            let r = parse(&format!(r#"{{"tag_name":"v9.9.9","html_url":"{elsewhere}"}}"#)).unwrap();
            assert_eq!(r.url, RELEASES_PAGE, "{elsewhere}");
        }
        assert!(parse(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn msi_download_requires_matching_repository_tag_and_filename() {
        let version = "26.10.25";
        let filename = "lhtPDF-26.10.25-windows-x64.msi";
        let valid = format!(
            "https://github.com/lht99/lhtPDF/releases/download/{version}/{filename}"
        );

        let cases = [
            (valid.as_str(), true),
            ("https://example.com/lhtPDF-26.10.25-windows-x64.msi", false),
            ("https://github.com/storytold/pdfcraft/releases/download/26.10.25/lhtPDF-26.10.25-windows-x64.msi", false),
            ("https://github.com/lht99/lhtPDF/releases/download/26.10.24/lhtPDF-26.10.25-windows-x64.msi", false),
            ("https://github.com/lht99/lhtPDF/releases/download/26.10.25/lhtPDF-26.10.24-windows-x64.msi", false),
            ("javascript:alert(1)", false),
        ];

        for (download, accepted) in cases {
            let body = serde_json::json!({
                "tag_name": version,
                "html_url": format!("{RELEASES_PAGE}/tag/{version}"),
                "assets": [{
                    "name": filename,
                    "browser_download_url": download
                }]
            });

            let release = parse(&body.to_string()).unwrap();
            assert_eq!(
                release.msi_url.is_some(),
                accepted,
                "Unexpected MSI URL acceptance: {download}"
            );
        }

        let without_assets = serde_json::json!({
            "tag_name": version,
            "html_url": format!("{RELEASES_PAGE}/tag/{version}")
        });

        let release = parse(&without_assets.to_string()).unwrap();

        assert_eq!(release.msi_url, None);
        assert_eq!(
            release.url,
            format!("{RELEASES_PAGE}/tag/{version}")
        );
    }
    #[test]
    fn rate_limit_messages_name_the_wait() {
        let limited = "GitHub is limiting update checks for now (shared hourly quota); try again in about 20 minutes";
        assert_eq!(rate_limited(Some("1800001200"), 1800000000), limited);
        // Rounds up: 61 seconds out reads as 2 minutes.
        assert_eq!(
            rate_limited(Some("1800000061"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 2 minutes"
        );
        assert_eq!(
            rate_limited(Some("1800000060"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 1 minute"
        );
        // No header, not a number, or already past: ask for patience instead of a time.
        let later = "GitHub is limiting update checks for now (shared hourly quota); try again later";
        for bad in [None, Some(""), Some("soon"), Some("1799999999"), Some("1800000000")] {
            assert_eq!(rate_limited(bad, 1800000000), later);
        }
    }

    /// Live: asks GitHub over TLS with the OS's roots (`cargo test -p pdfcraft -- --ignored`).
    #[test]
    #[ignore = "needs network access"]
    fn github_answers_with_the_latest_release() {
        let r = latest_release().unwrap();
        assert!(pdfcraft_ui_egui::updates::is_newer(&r.version, "0.0.0"), "{r:?}");
        assert!(r.url.starts_with(RELEASES_PAGE), "{r:?}");
    }
}
