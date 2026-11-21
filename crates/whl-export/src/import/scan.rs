pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    Done(usize),
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    TooDeep,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Unknown,
    Text,
    Container,
    Bare,
}

#[derive(Debug, Clone, Copy)]
pub struct ValueScan {
    position: usize,
    depth: usize,
    in_text: bool,
    escaped: bool,
    shape: Shape,
}

pub fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

impl ValueScan {
    pub fn new() -> Self {
        Self {
            position: 0,
            depth: 0,
            in_text: false,
            escaped: false,
            shape: Shape::Unknown,
        }
    }

    pub fn advance(&mut self, bytes: &[u8], finished: bool) -> Result<Scan, Fault> {
        if self.shape == Shape::Unknown {
            let Some(&first) = bytes.first() else {
                return if finished {
                    Err(Fault::Incomplete)
                } else {
                    Ok(Scan::More)
                };
            };
            match first {
                b'"' => {
                    self.shape = Shape::Text;
                    self.in_text = true;
                    self.position = 1;
                }
                b'{' | b'[' => {
                    self.shape = Shape::Container;
                    self.depth = 1;
                    self.position = 1;
                }
                _ => self.shape = Shape::Bare,
            }
        }
        while self.position < bytes.len() {
            let byte = bytes[self.position];
            match self.shape {
                Shape::Text => {
                    if self.step_text(byte) {
                        return Ok(Scan::Done(self.position + 1));
                    }
                }
                Shape::Container => {
                    if self.in_text {
                        self.step_text(byte);
                    } else {
                        match byte {
                            b'"' => self.in_text = true,
                            b'{' | b'[' => {
                                self.depth += 1;
                                if self.depth > MAX_DEPTH {
                                    return Err(Fault::TooDeep);
                                }
                            }
                            b'}' | b']' => {
                                self.depth = self.depth.saturating_sub(1);
                                if self.depth == 0 {
                                    return Ok(Scan::Done(self.position + 1));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Shape::Bare => {
                    if is_space(byte) || matches!(byte, b',' | b'}' | b']') {
                        return Ok(Scan::Done(self.position));
                    }
                }
                Shape::Unknown => return Err(Fault::Incomplete),
            }
            self.position += 1;
        }
        if finished {
            return match self.shape {
                Shape::Bare if self.position > 0 => Ok(Scan::Done(self.position)),
                _ => Err(Fault::Incomplete),
            };
        }
        Ok(Scan::More)
    }

    fn step_text(&mut self, byte: u8) -> bool {
        if self.escaped {
            self.escaped = false;
            return false;
        }
        match byte {
            b'\\' => {
                self.escaped = true;
                false
            }
            b'"' => {
                self.in_text = false;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole(text: &str) -> Result<Scan, Fault> {
        ValueScan::new().advance(text.as_bytes(), false)
    }

    #[test]
    fn a_container_ends_at_its_matching_bracket_and_ignores_brackets_in_strings() {
        let text = r#"{"a":"}]\"","b":[1,{"c":2}]} tail"#;
        assert_eq!(whole(text), Ok(Scan::Done(text.len() - " tail".len())));
    }

    #[test]
    fn a_value_split_across_chunks_resumes_where_it_stopped() {
        let text = br#"{"a":"x\\"}"#;
        let mut scan = ValueScan::new();
        assert_eq!(scan.advance(&text[..4], false), Ok(Scan::More));
        assert_eq!(scan.advance(&text[..8], false), Ok(Scan::More));
        assert_eq!(scan.advance(text, false), Ok(Scan::Done(text.len())));
    }

    #[test]
    fn nesting_beyond_the_limit_is_a_fault() {
        let deep = "[".repeat(MAX_DEPTH + 1);
        assert_eq!(whole(&deep), Err(Fault::TooDeep));
        let allowed = "[".repeat(MAX_DEPTH);
        assert_eq!(whole(&allowed), Ok(Scan::More));
    }

    #[test]
    fn a_bare_value_ends_at_a_delimiter_or_at_the_end_of_input() {
        assert_eq!(whole("12,"), Ok(Scan::Done(2)));
        assert_eq!(ValueScan::new().advance(b"12", true), Ok(Scan::Done(2)));
        assert_eq!(
            ValueScan::new().advance(br#"{"a":"#, true),
            Err(Fault::Incomplete)
        );
    }
}
