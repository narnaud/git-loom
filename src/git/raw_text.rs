//! Git output loom handles as text but must hand back byte for byte.
//!
//! A patch carries file contents in whatever encoding they have, and loom
//! keeps patches as `String`s — in its state file, in parked files, through
//! the hunk parsers. `from_utf8_lossy` would replace every byte that is not
//! UTF-8 for good, so a Latin-1 line saved and replayed comes back corrupted.
//! [`decode`] instead maps each such byte to a char of its own, which
//! [`encode`] turns back into that byte where the text leaves loom.

/// Escapes stand for bytes `0x00..=0xFF` as the last 256 code points of
/// plane 16 (private use), which no real text uses: one that does is escaped
/// itself, byte by byte, so the round trip holds for any input.
const ESCAPE_BASE: u32 = 0x10_FF00;

/// Decode `bytes` losslessly: valid UTF-8 as itself, anything else as escapes.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        push_escaping(&mut out, chunk.valid());
        out.extend(chunk.invalid().iter().map(|&b| escape(b)));
    }
    out
}

/// The bytes [`decode`] read `text` from.
pub fn encode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut buf = [0; 4];
    for c in text.chars() {
        match (c as u32).checked_sub(ESCAPE_BASE) {
            Some(byte) => out.push(byte as u8),
            None => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
        }
    }
    out
}

/// `text` as a person or an agent should read it: each escape as U+FFFD, what
/// a lossy decode shows. Never for text handed back to git.
pub fn for_display(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.chars().any(is_escape) {
        return std::borrow::Cow::Borrowed(text);
    }
    String::from_utf8_lossy(&encode(text)).into_owned().into()
}

fn is_escape(c: char) -> bool {
    c as u32 >= ESCAPE_BASE
}

fn push_escaping(out: &mut String, text: &str) {
    let mut buf = [0; 4];
    for c in text.chars() {
        if is_escape(c) {
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push(escape(b));
            }
        } else {
            out.push(c);
        }
    }
}

fn escape(byte: u8) -> char {
    char::from_u32(ESCAPE_BASE + u32::from(byte)).expect("plane 16 is all scalar values")
}

#[cfg(test)]
#[path = "raw_text_test.rs"]
mod tests;
