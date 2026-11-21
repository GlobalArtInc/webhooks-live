use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use whl_store::CaptureDetail;

use crate::error::ExportError;

pub const HEADER_ROW: &str = "id,seq,kind,received_at,method,path,query_string,client_ip,content_type,body_size,response_status,headers_json,body_preview";
pub const NEWLINE: &str = "\r\n";

const PREVIEW_BYTES: usize = 4096;
const BASE64_PREFIX: &str = "base64:";
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];
const QUOTE_TRIGGERS: [char; 4] = [',', '"', '\r', '\n'];

pub fn row(detail: &CaptureDetail, body: &[u8]) -> Result<String, ExportError> {
    let summary = &detail.summary;
    let headers_json = serde_json::to_string(&detail.headers)?;
    let cells = [
        summary.id.clone(),
        summary.seq.to_string(),
        summary.kind.as_str().to_owned(),
        summary.received_at.clone(),
        summary.method.clone(),
        summary.path.clone(),
        summary.query_string.clone(),
        summary.client_ip.clone(),
        summary.content_type.clone().unwrap_or_default(),
        summary.body_size.to_string(),
        summary.response_status.to_string(),
        headers_json,
        preview(body),
    ];
    Ok(cells
        .iter()
        .map(|cell| escape(cell))
        .collect::<Vec<_>>()
        .join(","))
}

pub fn escape(text: &str) -> String {
    let neutral = if text.starts_with(FORMULA_STARTS) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if neutral.contains(QUOTE_TRIGGERS) {
        format!("\"{}\"", neutral.replace('"', "\"\""))
    } else {
        neutral
    }
}

fn preview(body: &[u8]) -> String {
    let head = &body[..body.len().min(PREVIEW_BYTES)];
    match std::str::from_utf8(head) {
        Ok(text) => text.to_owned(),
        Err(error) if body.len() > PREVIEW_BYTES && error.error_len().is_none() => {
            String::from_utf8_lossy(&head[..error.valid_up_to()]).into_owned()
        }
        Err(_) => format!("{BASE64_PREFIX}{}", STANDARD.encode(head)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_starting_with_a_formula_character_get_a_leading_quote() {
        for text in ["=HYPERLINK(\"x\")", "+1", "-2", "@SUM", "\tcmd", "\rcmd"] {
            let cell = escape(text);
            assert!(
                cell.starts_with('\'') || cell.starts_with("\"'"),
                "{text:?} -> {cell:?}"
            );
        }
        assert_eq!(escape("plain"), "plain");
    }

    #[test]
    fn cells_with_separators_or_quotes_are_quoted_and_quotes_doubled() {
        assert_eq!(escape("a,b"), "\"a,b\"");
        assert_eq!(escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn a_binary_body_previews_as_base64_with_its_prefix() {
        assert_eq!(preview(&[0xff, 0x00, 0x41]), "base64:/wBB");
    }
}
