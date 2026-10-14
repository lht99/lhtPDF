# lhtPDF

**lhtPDF** is a Windows PDF application based on the open-source
[PdfCraft](https://github.com/storytold/pdfcraft) project.

**Website & Support:** https://letuan.io.vn

## Features

- Open and view PDF documents.
- Organize, combine and split PDF files.
- Edit and annotate PDF documents.
- Work with PDF forms.
- Recognize text using OCR models.
- Process and export PDF documents.
- English and Vietnamese interface languages.

Some features may depend on the application build and bundled components.

## Download

Download the latest builds from the **GitHub Releases** section
of the lhtPDF repository.

### Windows x64

| File | Description |
| --- | --- |
| `lhtPDF-10.2.2-windows-x64.msi` | Windows installer |
| `lhtPDF-10.2.2-windows-x64-portable.zip` | Portable version |
| `SHA256SUMS.txt` | SHA-256 checksums, when available |

### Windows SmartScreen

The Windows installer and portable application may be unsigned.

Windows SmartScreen may display a warning when downloading or
running an unsigned or unfamiliar application.

Only download releases from the official lhtPDF repository
or the project website.

## Build from source

### Requirements

- Rust stable with the Windows MSVC target.
- Visual Studio C++ Build Tools.
- Windows SDK.
- WiX Toolset 5.0.2 for MSI packaging.
- Internet access to download OCR model assets.

### Build GUI and CLI

Run the following command from the repository root:

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc -p pdfcraft -p pdfcraft-cli
```

The original Rust crate names remain `pdfcraft` for source compatibility.

### Build Windows MSI and Portable ZIP

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File packaging/windows/package.ps1 -Arch x64
```

The packaging script generates MSI and Portable ZIP distributions.

## Languages

The lhtPDF interface supports:

- English
- Tiếng Việt (Vietnamese)

## Version

Current lhtPDF Windows product version: **10.2.2**.

Internal Rust crate versions may differ from the Windows product version.

## Original project and attribution

lhtPDF is an independently maintained derivative of
the open-source **PdfCraft** project.

Original source repository:

https://github.com/storytold/pdfcraft

lhtPDF is not an official ArtCraft release.

Copyright notices, contributor attribution, and applicable
third-party licenses from the original project are preserved.

For reference, see [README-UPSTREAM.md](README-UPSTREAM.md).

## License

The underlying PdfCraft source code is dual-licensed under:

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

See [NOTICE](NOTICE) and [ATTRIBUTION.md](ATTRIBUTION.md)
for additional copyright and third-party attribution.

ArtCraft names, trademarks and logos are not covered by
the source code license.

## Support

Website: https://letuan.io.vn
