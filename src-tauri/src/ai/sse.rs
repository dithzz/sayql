//! Server-sent events: splits a byte stream into the JSON payloads of its `data:` lines.

use serde_json::Value;

#[derive(Default)]
pub(super) struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    /// Feeds a chunk and returns the payloads of every event it completed.
    /// Events may be split across chunks at any byte, including inside UTF-8 sequences.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        // Some servers end lines with CRLF; a CR inside a JSON payload is always escaped.
        self.buf.extend(chunk.iter().filter(|&&b| b != b'\r'));
        let mut out = Vec::new();
        // Events are separated by a blank line.
        while let Some(end) = self.buf.windows(2).position(|w| w == b"\n\n").map(|p| p + 2) {
            let event: Vec<u8> = self.buf.drain(..end).collect();
            for line in String::from_utf8_lossy(&event).lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    if let Ok(v) = serde_json::from_str(data.trim()) {
                        out.push(v);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let stream = "event: a\ndata: {\"type\":\"x\",\"t\":\"é\"}\n\nevent: b\ndata: {\"type\":\"y\"}\n\n";
        let bytes = stream.as_bytes();
        // Split inside the two-byte "é".
        let cut = stream.find('é').unwrap() + 1;
        let mut p = SseParser::default();
        let mut got = p.push(&bytes[..cut]);
        assert!(got.is_empty());
        got.extend(p.push(&bytes[cut..]));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0]["t"], "é");
        assert_eq!(got[1]["type"], "y");
    }

    #[test]
    fn accepts_crlf_line_endings_and_skips_done() {
        let mut p = SseParser::default();
        let got = p.push(b"data: {\"choices\":[]}\r\n\r\ndata: [DONE]\r\n\r\n");
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn ignores_non_data_lines_and_bad_json() {
        let mut p = SseParser::default();
        assert!(p.push(b": ping\n\ndata: not json\n\n").is_empty());
    }
}
