//! CLI hostile-input and supported-package tests.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#[cfg(test)]
mod tests {
    use misterdiagram_opc::{MAX_PART, PackageOut, RelsOut};
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use std::process::Command;
    fn package(kind: &str) -> Vec<u8> {
        let mut p = PackageOut::default();
        p.xml_part("visio/document.xml", kind, b"<VisioDocument xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"/>".to_vec());
        let mut r = RelsOut::default();
        r.add("http://schemas.microsoft.com/visio/2010/relationships/document", "visio/document.xml", false);
        p.rels("", &r);
        p.finish().unwrap()
    }
    fn invoke(bytes: &[u8], ext: &str) -> std::process::Output {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/info-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{}-{}.{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), ext));
        std::fs::write(&p, bytes).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_misterdiagram")).args(["info"]).arg(&p).output().unwrap();
        std::fs::remove_file(p).unwrap();
        o
    }
    #[test]
    fn supported_package_types_and_root_relationships() {
        for (ext, kind) in [
            ("vsdx", "drawing"),
            ("vssx", "stencil"),
            ("vstx", "template"),
            ("vsdm", "drawing.macroEnabled"),
            ("vssm", "stencil.macroEnabled"),
            ("vstm", "template.macroEnabled"),
        ] {
            let o = invoke(&package(&format!("application/vnd.ms-visio.{kind}.main+xml")), ext);
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            let s = String::from_utf8(o.stdout).unwrap();
            assert!(s.contains("visio/document.xml\tapplication/vnd.ms-visio."));
            assert!(s.contains("relationship\trId1\tdocument\tvisio/document.xml"));
        }
    }
    #[test]
    fn info_escapes_package_name_controls_without_changing_tsv_columns() {
        let mut p = PackageOut::default();
        p.xml_part(
            "visio/document.xml",
            "application/vnd.ms-visio.drawing.main+xml",
            b"<VisioDocument xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"/>".to_vec(),
        );
        p.binary("notes/alert\u{1b}[31m\nnext\tfield.bin", "application/octet-stream", vec![1]);
        let mut r = RelsOut::default();
        r.add("http://schemas.microsoft.com/visio/2010/relationships/document", "visio/document.xml", false);
        p.rels("", &r);

        let output = invoke(&p.finish().unwrap(), "vsdx");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("notes/alert\\u{1b}[31m\\nnext\\tfield.bin\tapplication/octet-stream\t1"));
        assert!(text.lines().all(|line| line.split('\t').count() == 3 || line.starts_with("relationship\t")));
    }
    #[test]
    fn info_rejects_oversized_compressed_file_before_reading_it() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/info-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("sparse-{}.vsdx", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_PART + 1).unwrap();
        drop(file);

        let output = Command::new(env!("CARGO_BIN_EXE_misterdiagram")).args(["info"]).arg(&path).output().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains("compressed package exceeds size cap"));
    }
    #[test]
    fn info_escapes_package_name_in_errors() {
        let mut archive = zip::ZipWriter::new_append(Cursor::new(package("application/vnd.ms-visio.drawing.main+xml"))).unwrap();
        archive.start_file("alert\u{1b}[31m\nnext.bin", zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(b"x").unwrap();
        let bytes = archive.finish().unwrap().into_inner();

        let output = invoke(&bytes, "vsdx");
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains('\u{1b}'));
        assert_eq!(error.lines().count(), 1);
        assert!(error.contains("alert\\u{1b}[31m\\nnext.bin"));
    }
    #[test]
    fn shipped_2011_core_namespace_is_supported() {
        // ENTITY_M/U and OBJREL_M/U shipped stencils use the documented 2011 core URI.
        let mut p = PackageOut::default();
        p.xml_part(
            "visio/document.xml",
            "application/vnd.ms-visio.stencil.main+xml",
            b"<VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2011/1/core'/>".to_vec(),
        );
        let mut r = RelsOut::default();
        r.add("http://schemas.microsoft.com/visio/2010/relationships/document", "visio/document.xml", false);
        p.rels("", &r);
        let o = invoke(&p.finish().unwrap(), "vssx");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    #[test]
    fn rejects_unrelated_namespaces_and_malformed_document() {
        for (uri, xml) in [
            ("https://example.invalid/document", "<VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main'/>"),
            ("http://schemas.microsoft.com/visio/2010/relationships/document", "<VisioDocument xmlns='urn:not-visio'/>"),
            (
                "http://schemas.microsoft.com/visio/2010/relationships/document",
                "<VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main' bad=unquoted/>",
            ),
        ] {
            let mut p = PackageOut::default();
            p.xml_part("visio/document.xml", "application/vnd.ms-visio.drawing.main+xml", xml.as_bytes().to_vec());
            let mut r = RelsOut::default();
            r.add(uri, "visio/document.xml", false);
            p.rels("", &r);
            let o = invoke(&p.finish().unwrap(), "vsdx");
            assert!(!o.status.success());
            assert_eq!(String::from_utf8(o.stderr).unwrap().lines().count(), 1);
        }
    }
    #[test]
    fn truncated_arbitrary_and_zip_bomb_fail_cleanly() {
        let valid = package("application/vnd.ms-visio.drawing.main+xml");
        let mut bomb = valid.clone();
        // A ZIP bomb's declared expanded size must be rejected before allocating or inflating it.
        let at = bomb.windows(4).position(|w| w == b"PK\x01\x02").unwrap() + 24;
        bomb[at..at + 4].copy_from_slice(&((MAX_PART + 1) as u32).to_le_bytes());
        for data in [b"not a zip".as_slice(), &valid[..valid.len() / 2], &bomb, &package("application/xml")] {
            let o = invoke(data, "vsdx");
            assert!(!o.status.success());
            let e = String::from_utf8(o.stderr).unwrap();
            assert_eq!(e.lines().count(), 1, "{e}");
            assert!(e.starts_with("error: "));
            assert!(!e.contains("panicked"));
        }
        assert!(!invoke(&valid, "zip").status.success());
    }
}
