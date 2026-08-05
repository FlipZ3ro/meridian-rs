#[cfg(test)]
mod tests {
    #[test]
    fn readme_has_visual_overview_and_operator_sections() {
        let readme = include_str!("../README.md");
        for required in [
            "![Meridian RS architecture](docs/assets/meridian-rs-architecture.png)",
            "## Operator quickstart",
            "## Command center",
            "## System map",
            "```mermaid",
            "## Readable logs",
            "MERIDIAN_LOG_STYLE=pretty",
        ] {
            assert!(readme.contains(required), "README missing: {required}");
        }
    }

    #[test]
    fn architecture_png_has_expected_dimensions() {
        let png = include_bytes!("../docs/assets/meridian-rs-architecture.png");

        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "asset is not a PNG");
        let width = u32::from_be_bytes(png[16..20].try_into().expect("PNG width bytes"));
        let height = u32::from_be_bytes(png[20..24].try_into().expect("PNG height bytes"));
        assert_eq!((width, height), (1678, 937));
        assert!(
            png.len() > 100_000,
            "architecture PNG looks unexpectedly small"
        );
    }

    #[test]
    fn readme_roadmap_has_no_unfinished_checkboxes() {
        let readme = include_str!("../README.md");

        assert!(
            !readme.contains("- [ ]"),
            "README still has unchecked roadmap items"
        );
    }
}
