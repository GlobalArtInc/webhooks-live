const GROWTH_CAP_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text { after_cr: bool },
    LineStart,
    Dot,
    DotCr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    NeedMore,
    Done { consumed: usize },
    TooLarge,
}

#[derive(Debug)]
pub struct DataCollector {
    data: Vec<u8>,
    state: State,
    max: usize,
}

impl DataCollector {
    pub fn new(max: usize) -> Self {
        Self {
            data: Vec::new(),
            state: State::LineStart,
            max,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    fn emit(&mut self, byte: u8) -> bool {
        if self.data.len() >= self.max {
            return false;
        }
        if self.data.len() == self.data.capacity() {
            let step = self.data.len().clamp(4096, GROWTH_CAP_BYTES);
            self.data
                .reserve_exact(step.min(self.max - self.data.len()));
        }
        self.data.push(byte);
        true
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Feed {
        for (position, &byte) in chunk.iter().enumerate() {
            let step = match self.state {
                State::Text { after_cr } => {
                    let next = if byte == b'\n' && after_cr {
                        State::LineStart
                    } else {
                        State::Text {
                            after_cr: byte == b'\r',
                        }
                    };
                    self.emit(byte).then_some(next)
                }
                State::LineStart => match byte {
                    b'.' => Some(State::Dot),
                    _ => self.emit(byte).then_some(State::Text {
                        after_cr: byte == b'\r',
                    }),
                },
                State::Dot => match byte {
                    b'\r' => Some(State::DotCr),
                    _ => self.emit(byte).then_some(State::Text { after_cr: false }),
                },
                State::DotCr => {
                    if byte == b'\n' {
                        return Feed::Done {
                            consumed: position + 1,
                        };
                    }
                    if !self.emit(b'\r') {
                        return Feed::TooLarge;
                    }
                    self.emit(byte).then_some(State::Text {
                        after_cr: byte == b'\r',
                    })
                }
            };
            match step {
                Some(next) => self.state = next,
                None => return Feed::TooLarge,
            }
        }
        Feed::NeedMore
    }
}
