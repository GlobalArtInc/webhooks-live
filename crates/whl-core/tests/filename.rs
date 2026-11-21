use whl_core::filename::safe_filename;

#[test]
fn names_follow_the_sanitising_table() {
    let table = [
        (
            "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21",
            "bin",
            "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21.bin",
        ),
        ("report v1.final", "bin", "report_v1.final.bin"),
        ("../../etc/passwd", "bin", "_.._etc_passwd.bin"),
        ("..\\win", "bin", "_win.bin"),
        ("..hidden", "bin", "hidden.bin"),
        ("...", "bin", "capture.bin"),
        ("", "bin", "capture.bin"),
        ("a\"b", "bin", "a_b.bin"),
        ("a;b=c", "bin", "a_b_c.bin"),
        ("line\r\nbreak", "bin", "line__break.bin"),
        ("nul\0byte", "bin", "nul_byte.bin"),
        (
            "\u{043f}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}",
            "bin",
            "______.bin",
        ),
        ("\u{1f600}", "bin", "_.bin"),
        ("name", "", "name"),
        ("name", "..", "name"),
        ("name", "b/i n", "name.b_i_n"),
        ("name", ".tar.gz", "name.tar.gz"),
    ];
    for (stem, extension, expected) in table {
        assert_eq!(safe_filename(stem, extension), expected, "{stem:?}");
    }
}

#[test]
fn output_only_holds_safe_characters_and_is_bounded() {
    let hostile = "\"; filename=evil.html\r\nX: y/\\\u{0}\u{7f}%00".repeat(20);
    let name = safe_filename(&hostile, &hostile);
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    );
    assert!(name.len() <= 64 + 1 + 16);
    assert!(!name.starts_with('.'));
}
