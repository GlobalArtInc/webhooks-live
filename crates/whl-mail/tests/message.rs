use whl_mail::message::{
    MAX_DEPTH, MAX_HEADER_BLOCK_BYTES, MAX_LINES, MAX_PARTS, ParseFailure, PartFailure, PartView,
    decode_part, parse_message,
};

fn multipart(parts: usize) -> String {
    let mut raw = String::from(
        "From: a@example.org\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n",
    );
    for number in 0..parts {
        raw.push_str(&format!(
            "--b\r\nContent-Type: text/plain\r\n\r\npart {number}\r\n"
        ));
    }
    raw.push_str("--b--\r\n");
    raw
}

fn nested(depth: usize) -> String {
    let mut raw = String::from("From: a@example.org\r\nMIME-Version: 1.0\r\n");
    for level in 0..depth {
        raw.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"b{level}\"\r\n\r\n--b{level}\r\n"
        ));
    }
    raw.push_str("Content-Type: text/plain\r\n\r\ninnermost\r\n");
    for level in (0..depth).rev() {
        raw.push_str(&format!("--b{level}--\r\n"));
    }
    raw
}

#[test]
fn more_than_fifty_parts_are_cut_and_flagged() {
    let parsed = parse_message(multipart(MAX_PARTS + 30).as_bytes()).expect("a parsed message");

    assert!(parsed.parts.len() <= MAX_PARTS);
    assert!(parsed.parts_truncated);
}

#[test]
fn a_message_within_the_part_limit_is_not_flagged() {
    let parsed = parse_message(multipart(3).as_bytes()).expect("a parsed message");

    assert_eq!(parsed.parts.len(), 3);
    assert!(!parsed.parts_truncated);
}

#[test]
fn nesting_beyond_the_depth_limit_is_parsed_without_opening_the_excess() {
    let parsed = parse_message(nested(MAX_DEPTH + 12).as_bytes()).expect("a parsed message");

    assert!(parsed.parts.len() <= MAX_PARTS);
}

#[test]
fn more_than_the_line_limit_is_refused_before_parsing() {
    let raw = format!("Subject: x\r\n\r\n{}", "\n".repeat(MAX_LINES + 1));

    assert_eq!(
        parse_message(raw.as_bytes()),
        Err(ParseFailure::TooManyLines)
    );
}

#[test]
fn a_header_block_over_the_limit_is_refused_before_parsing() {
    let mut raw = String::new();
    while raw.len() <= MAX_HEADER_BLOCK_BYTES {
        raw.push_str("X-Filler: value\r\n");
    }

    assert_eq!(
        parse_message(raw.as_bytes()),
        Err(ParseFailure::HeaderBlockTooLarge)
    );
}

#[test]
fn garbage_without_a_header_block_is_unparseable_and_does_not_panic() {
    assert_eq!(parse_message(b""), Err(ParseFailure::Unparseable));
    assert!(parse_message(&[0xFF, 0x00, b'\r', 0xFE, b'\n']).is_err());
}

#[test]
fn a_base64_part_is_decoded_on_demand_and_an_unknown_index_is_not_found() {
    let raw = "From: a@example.org\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";

    let part = decode_part(raw.as_bytes(), 0, PartView::Decoded).expect("the part decodes");
    let missing = decode_part(raw.as_bytes(), 1, PartView::Decoded);

    assert_eq!(part.data, b"%PDF-1.4\n");
    assert_eq!(part.filename.as_deref(), Some("report.pdf"));
    assert!(!part.is_text);
    assert_eq!(missing, Err(PartFailure::NotFound));
}
