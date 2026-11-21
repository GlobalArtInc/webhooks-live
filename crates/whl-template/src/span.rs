#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub length: usize,
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn locate(source: &str, range: ByteRange) -> Position {
    let start = range.start.min(source.len());
    let end = range.end.clamp(start, source.len());
    let before = source.get(..start).unwrap_or("");
    let newlines = before.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before
        .get(line_start..)
        .map_or(0, |line| line.chars().count());
    let length = source
        .get(start..end)
        .map_or(0, |text| text.chars().count());
    Position {
        offset: start,
        line: saturating_u32(newlines + 1),
        column: saturating_u32(column + 1),
        length,
    }
}
