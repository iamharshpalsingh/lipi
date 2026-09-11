//! The `crypto` module: hashing, password storage and secure random values.

use crate::builtins::{opt_num, text};
use crate::interp::Interpreter;
use crate::value::{Args, Value};
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

const ITERATIONS: u32 = 100_000;
const SCHEME: &str = "pbkdf2-sha256";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_bytes(it: &Interpreter, a: &Args, n: usize) -> Result<Vec<u8>, crate::interp::Flow> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|e| it.error(format!("couldn't get secure random bytes: {e}"), a.span, None))?;
    Ok(buf)
}

/// Compare without leaking how many leading bytes matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn derive(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut out);
    out
}

pub fn entries() -> Vec<(&'static str, Value)> {
    vec![
        (
            "sha256",
            Value::native("sha256", |it, a| Ok(Value::string(hex(&Sha256::digest(text(it, a, 0, "text")?.as_bytes()))))),
        ),
        (
            "hmacSha256",
            Value::native("hmacSha256", |it, a| {
                let key = text(it, a, 0, "key")?;
                let message = text(it, a, 1, "text")?;
                let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
                mac.update(message.as_bytes());
                Ok(Value::string(hex(&mac.finalize().into_bytes())))
            }),
        ),
        (
            "hashPassword",
            Value::native("hashPassword", |it, a| {
                let password = text(it, a, 0, "password")?;
                let salt = random_bytes(it, a, 16)?;
                let hash = derive(password.as_bytes(), &salt, ITERATIONS);
                Ok(Value::string(format!(
                    "{SCHEME}${ITERATIONS}${}${}",
                    STANDARD_NO_PAD.encode(salt),
                    STANDARD_NO_PAD.encode(hash)
                )))
            }),
        ),
        (
            "verifyPassword",
            Value::native("verifyPassword", |it, a| {
                let password = text(it, a, 0, "password")?;
                let stored = text(it, a, 1, "hash")?;
                let parts: Vec<&str> = stored.split('$').collect();
                let parsed = match parts.as_slice() {
                    [scheme, iterations, salt, hash] if *scheme == SCHEME => iterations
                        .parse::<u32>()
                        .ok()
                        .zip(STANDARD_NO_PAD.decode(salt).ok())
                        .zip(STANDARD_NO_PAD.decode(hash).ok()),
                    _ => None,
                };
                let Some(((iterations, salt), expected)) = parsed else {
                    return Err(it.err(
                        "LIP5008",
                        "this isn't a password hash made by crypto.hashPassword",
                        a.span,
                        Some("Store the result of crypto.hashPassword(password) and pass that here.".into()),
                    ));
                };
                let actual = derive(password.as_bytes(), &salt, iterations);
                Ok(Value::Bool(constant_time_eq(&actual, &expected)))
            }),
        ),
        (
            "randomToken",
            Value::native("randomToken", |it, a| {
                let n = opt_num(it, a, 0, "bytes")?.unwrap_or(32.0).clamp(1.0, 1024.0) as usize;
                Ok(Value::string(URL_SAFE_NO_PAD.encode(random_bytes(it, a, n)?)))
            }),
        ),
        (
            "uuid",
            Value::native("uuid", |it, a| {
                let mut b = random_bytes(it, a, 16)?;
                b[6] = (b[6] & 0x0f) | 0x40; // version 4
                b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
                let h = hex(&b);
                Ok(Value::string(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])))
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert_ne!(derive(b"pw", b"salt-one", 10), derive(b"pw", b"salt-two", 10));
    }
}
