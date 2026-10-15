use super::{decode, encode, for_display};

#[test]
fn utf8_text_is_kept_as_is() {
    let text = "naïve — 日本\n";
    assert_eq!(decode(text.as_bytes()), text);
    assert_eq!(encode(text), text.as_bytes());
}

#[test]
fn bytes_that_are_not_utf8_round_trip() {
    for bytes in [
        &b"na\xefve\n"[..],
        b"\xff\xfe\x00\x80",
        b"truncated \xe6\x97",
        b"\x82\xa0 shift-jis",
    ] {
        assert_eq!(encode(&decode(bytes)), bytes, "{bytes:?}");
    }
}

/// A char of the escape range in real input is escaped too, or encoding would
/// turn it into a single byte.
#[test]
fn the_escape_range_itself_round_trips() {
    let text = "\u{10FF41}\u{10FFFF}";
    assert_eq!(encode(&decode(text.as_bytes())), text.as_bytes());
}

/// Every one- and two-byte input, valid or not: the round trip is the whole
/// contract.
#[test]
fn every_short_input_round_trips() {
    for a in 0..=255u8 {
        assert_eq!(encode(&decode(&[a])), [a]);
        for b in 0..=255u8 {
            assert_eq!(encode(&decode(&[a, b])), [a, b], "{a:#x} {b:#x}");
        }
    }
}

/// Exactly what a lossy decode shows: a truncated sequence is one U+FFFD.
#[test]
fn display_matches_a_lossy_decode() {
    let bytes = b"caf\xe9 \xe6\x97 ok \xf4\x8f\xbf\xbf";
    assert_eq!(for_display(&decode(bytes)), String::from_utf8_lossy(bytes));
}

#[test]
fn display_shows_escapes_as_replacement_chars() {
    assert_eq!(for_display(&decode(b"caf\xe9 ok")), "caf\u{FFFD} ok");
    assert!(matches!(
        for_display("plain"),
        std::borrow::Cow::Borrowed(_)
    ));
}
