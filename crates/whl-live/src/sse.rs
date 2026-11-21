use bytes::Bytes;

pub const PING_FRAME: &[u8] = b": ping\n\n";

pub fn retry_frame(millis: u64) -> Bytes {
    Bytes::from(format!("retry: {millis}\n\n"))
}

pub fn ping_frame() -> Bytes {
    Bytes::from_static(PING_FRAME)
}

pub fn event_frame(name: &str, id: Option<i64>, data: &str) -> Bytes {
    let id = id.map(|id| id.to_string());
    event_frame_with_text_id(name, id.as_deref(), data)
}

pub fn event_frame_with_text_id(name: &str, id: Option<&str>, data: &str) -> Bytes {
    let mut frame = String::with_capacity(name.len() + data.len() + 48);
    frame.push_str("event: ");
    frame.push_str(name);
    frame.push('\n');
    if let Some(id) = id {
        frame.push_str("id: ");
        frame.push_str(id);
        frame.push('\n');
    }
    for line in data.split(['\n', '\r']) {
        frame.push_str("data: ");
        frame.push_str(line);
        frame.push('\n');
    }
    frame.push('\n');
    Bytes::from(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_a_capture_with_its_id() {
        let frame = event_frame("capture", Some(7), r#"{"seq":7}"#);
        assert_eq!(&frame[..], b"event: capture\nid: 7\ndata: {\"seq\":7}\n\n");
    }

    #[test]
    fn frames_an_event_with_a_text_id() {
        let frame = event_frame_with_text_id("capture", Some("abc_-9"), "{}");
        assert_eq!(&frame[..], b"event: capture\nid: abc_-9\ndata: {}\n\n");
    }

    #[test]
    fn frames_an_event_without_an_id() {
        let frame = event_frame("bye", None, "{}");
        assert_eq!(&frame[..], b"event: bye\ndata: {}\n\n");
    }

    #[test]
    fn splits_line_breaks_in_data_into_separate_data_lines() {
        let frame = event_frame("x", None, "a\r\nb\nc");
        assert_eq!(
            &frame[..],
            b"event: x\ndata: a\ndata: \ndata: b\ndata: c\n\n"
        );
    }

    #[test]
    fn frames_retry_and_ping() {
        assert_eq!(&retry_frame(2000)[..], b"retry: 2000\n\n");
        assert_eq!(&ping_frame()[..], b": ping\n\n");
    }
}
