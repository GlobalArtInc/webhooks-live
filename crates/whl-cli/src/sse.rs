pub const MAX_LINE_BYTES: usize = 1 << 20;
pub const MAX_EVENT_BYTES: usize = 4 << 20;

const DEFAULT_EVENT_NAME: &str = "message";
const BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub name: String,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SseError {
    #[error("an event stream line is longer than {MAX_LINE_BYTES} bytes")]
    LineTooLong,
    #[error("an event stream event is larger than {MAX_EVENT_BYTES} bytes")]
    EventTooLarge,
}

#[derive(Debug, Default)]
pub struct SseParser {
    line: Vec<u8>,
    after_cr: bool,
    started: bool,
    name: Vec<u8>,
    data: Vec<u8>,
    has_data: bool,
    id: Option<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut events = Vec::new();
        for &byte in chunk {
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\n' => events.extend(self.finish_line()?),
                b'\r' => {
                    self.after_cr = true;
                    events.extend(self.finish_line()?);
                }
                _ => {
                    if self.line.len() >= MAX_LINE_BYTES {
                        return Err(SseError::LineTooLong);
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(events)
    }

    fn finish_line(&mut self) -> Result<Option<SseEvent>, SseError> {
        let mut line = std::mem::take(&mut self.line);
        if !self.started {
            self.started = true;
            if line.starts_with(BOM) {
                line.drain(..BOM.len());
            }
        }
        if line.is_empty() {
            return Ok(self.dispatch());
        }
        if line[0] == b':' {
            return Ok(None);
        }
        let (field, value) = match line.iter().position(|byte| *byte == b':') {
            Some(colon) => {
                let value = &line[colon + 1..];
                (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
            }
            None => (&line[..], &[][..]),
        };
        match field {
            b"event" => self.name = value.to_vec(),
            b"data" => {
                let separator = usize::from(self.has_data);
                if self.data.len() + separator + value.len() > MAX_EVENT_BYTES {
                    return Err(SseError::EventTooLarge);
                }
                if self.has_data {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
                self.has_data = true;
            }
            b"id" if !value.contains(&0) => {
                self.id = Some(String::from_utf8_lossy(value).into_owned());
            }
            _ => {}
        }
        Ok(None)
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let name = std::mem::take(&mut self.name);
        let data = std::mem::take(&mut self.data);
        let id = self.id.take();
        if !std::mem::take(&mut self.has_data) {
            return None;
        }
        let name = if name.is_empty() {
            DEFAULT_EVENT_NAME.to_owned()
        } else {
            String::from_utf8_lossy(&name).into_owned()
        };
        Some(SseEvent {
            name,
            data: String::from_utf8_lossy(&data).into_owned(),
            id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[u8]) -> Vec<SseEvent> {
        SseParser::new().feed(input).expect("valid stream")
    }

    #[test]
    fn parses_a_capture_frame_as_the_server_writes_it() {
        let events = parse(b"event: capture\nid: 7\ndata: {\"seq\":7}\n\n");
        assert_eq!(
            events,
            vec![SseEvent {
                name: "capture".to_owned(),
                data: "{\"seq\":7}".to_owned(),
                id: Some("7".to_owned()),
            }]
        );
    }

    #[test]
    fn parses_a_frame_without_an_id() {
        let events = parse(b"event: bye\ndata: {}\n\n");
        assert_eq!(events[0].name, "bye");
        assert_eq!(events[0].id, None);
    }

    #[test]
    fn joins_the_data_lines_the_server_splits_on_line_breaks() {
        let events = parse(b"event: x\ndata: a\ndata: \ndata: b\ndata: c\n\n");
        assert_eq!(events[0].data, "a\n\nb\nc");
    }

    #[test]
    fn ignores_the_retry_frame_and_the_ping_comment() {
        let events = parse(b"retry: 2000\n\n: ping\n\nevent: ready\ndata: {}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "ready");
    }

    #[test]
    fn reassembles_an_event_split_at_every_byte() {
        let input = b"event: capture\r\nid: 9\r\ndata: {\"seq\":9}\r\n\r\n: ping\n\n";
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        for byte in input {
            events.extend(parser.feed(std::slice::from_ref(byte)).expect("valid"));
        }
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"seq\":9}");
        assert_eq!(events[0].id.as_deref(), Some("9"));
    }

    #[test]
    fn accepts_a_bare_carriage_return_as_a_line_end() {
        let events = parse(b"event: a\rdata: 1\r\r");
        assert_eq!(events[0].name, "a");
        assert_eq!(events[0].data, "1");
    }

    #[test]
    fn names_an_event_without_a_name_message() {
        let events = parse(b"data: x\n\n");
        assert_eq!(events[0].name, "message");
    }

    #[test]
    fn refuses_a_line_over_the_limit() {
        let mut input = b"data: ".to_vec();
        input.resize(MAX_LINE_BYTES + 16, b'a');
        assert_eq!(SseParser::new().feed(&input), Err(SseError::LineTooLong));
    }

    #[test]
    fn refuses_an_event_over_the_limit() {
        let mut parser = SseParser::new();
        let line = format!("data: {}\n", "a".repeat(MAX_LINE_BYTES - 16));
        let mut outcome = Ok(Vec::new());
        for _ in 0..8 {
            outcome = parser.feed(line.as_bytes());
            if outcome.is_err() {
                break;
            }
        }
        assert_eq!(outcome, Err(SseError::EventTooLarge));
    }
}
