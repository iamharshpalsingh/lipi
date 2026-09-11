//! The `encoding` module: Base64, URL encoding and hex, for text (as UTF-8).
//! Decoding follows the same rules as JavaScript builds.

use crate::builtins::{self, text};
use crate::interp::{Flow, Interpreter};
use crate::value::*;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD};
use base64::engine::DecodePaddingMode;
use base64::{alphabet, Engine};

fn utf8(it: &Interpreter, a: &Args, bytes: Vec<u8>, what: &str) -> Result<Value, Flow> {
    String::from_utf8(bytes)
        .map(Value::string)
        .map_err(|_| it.err("LIP5008", format!("{what} isn't valid UTF-8 text"), a.span, Some("It decodes to bytes that aren't text.".into())))
}

/// Characters JavaScript's encodeURIComponent leaves as they are.
fn url_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c)
}

fn base64_decode(it: &Interpreter, a: &Args) -> Result<Value, Flow> {
    // Like the browser's atob: spaces are ignored and the = padding is optional.
    let mut t: String = text(it, a, 0, "text")?.chars().filter(|c| !matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')).collect();
    if t.len() % 4 == 0 {
        let pad = t.len() - t.trim_end_matches('=').len();
        t.truncate(t.len() - pad.min(2));
    }
    let valid = t.len() % 4 != 1 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/');
    let engine = GeneralPurpose::new(&alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::RequireNone).with_decode_allow_trailing_bits(true));
    match engine.decode(&t) {
        Ok(bytes) if valid => utf8(it, a, bytes, "the decoded Base64"),
        _ => Err(it.err("LIP5008", "this isn't valid Base64", a.span, Some("Base64 uses the letters A-Z and a-z, digits, + and /, with = at the end, like \"SGVsbG8=\".".into()))),
    }
}

fn url_decode(it: &Interpreter, a: &Args) -> Result<Value, Flow> {
    let t: Vec<char> = text(it, a, 0, "text")?.chars().collect();
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == '%' {
            let h: String = t[i + 1..(i + 3).min(t.len())].iter().collect();
            if h.len() != 2 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(it.err("LIP5008", "this isn't valid URL encoding", a.span, Some("A % must be followed by two hex digits, like %20 for a space.".into())));
            }
            bytes.push(u8::from_str_radix(&h, 16).expect("two hex digits"));
            i += 3;
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(t[i].encode_utf8(&mut buf).as_bytes());
            i += 1;
        }
    }
    utf8(it, a, bytes, "the decoded URL text")
}

fn hex_decode(it: &Interpreter, a: &Args) -> Result<Value, Flow> {
    let t = text(it, a, 0, "text")?;
    if t.len() % 2 != 0 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(it.err("LIP5008", "this isn't valid hex", a.span, Some("Hex uses pairs of the digits 0-9 and a-f, like \"48656c6c6f\".".into())));
    }
    let bytes = (0..t.len()).step_by(2).map(|i| u8::from_str_radix(&t[i..i + 2], 16).expect("hex digits")).collect();
    utf8(it, a, bytes, "the decoded hex")
}

pub fn module() -> Value {
    builtins::module(
        "encoding",
        vec![
            ("base64Encode", Value::native("base64Encode", |it, a| Ok(Value::string(STANDARD.encode(text(it, a, 0, "text")?.as_bytes()))))),
            ("base64Decode", Value::native("base64Decode", |it, a| base64_decode(it, a))),
            (
                "urlEncode",
                Value::native("urlEncode", |it, a| {
                    let mut out = String::new();
                    for c in text(it, a, 0, "text")?.chars() {
                        if url_safe(c) {
                            out.push(c);
                        } else {
                            let mut buf = [0u8; 4];
                            for b in c.encode_utf8(&mut buf).as_bytes() {
                                out.push_str(&format!("%{b:02X}"));
                            }
                        }
                    }
                    Ok(Value::string(out))
                }),
            ),
            ("urlDecode", Value::native("urlDecode", |it, a| url_decode(it, a))),
            (
                "hexEncode",
                Value::native("hexEncode", |it, a| Ok(Value::string(text(it, a, 0, "text")?.bytes().map(|b| format!("{b:02x}")).collect()))),
            ),
            ("hexDecode", Value::native("hexDecode", |it, a| hex_decode(it, a))),
        ],
    )
}
