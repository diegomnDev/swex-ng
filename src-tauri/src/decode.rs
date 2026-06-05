//! Pure-Rust port of SWEX's `smon_decryptor.js`.
//!
//! Original (app/proxy/smon_decryptor.js):
//!   key   = <16 bytes from the native addon `key()`>
//!   algo  = aes-128-cbc, IV = 16 zero bytes
//!   request:  base64 -> aes-decrypt -> JSON
//!   response: base64 -> aes-decrypt -> zlib.inflate -> JSON
//!
//! The only secret is the 16-byte key. In SWEX it lived in a precompiled
//! `.node` (one per platform/arch), which is exactly what tied the app to a
//! given architecture. Here it is loaded at runtime as hex, so the binary is
//! architecture-independent and builds natively for arm64.

use aes::Aes128;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockModeDecrypt, KeyIvInit};
use flate2::read::ZlibDecoder;
use std::io::Read;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    // Part of the public error surface; key-missing is currently reported as a
    // plain string at the command layer (lib.rs), so this variant isn't built yet.
    #[allow(dead_code)]
    #[error("decryption key not configured (set SWEX_KEY or drop key.hex)")]
    MissingKey,
    #[error("key must be 16 bytes (32 hex chars), got {0}")]
    BadKeyLen(usize),
    #[error("invalid hex key: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("aes/padding error")]
    Crypto,
    #[error("inflate: {0}")]
    Inflate(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

const ZERO_IV: [u8; 16] = [0u8; 16];

fn aes_decrypt(key: &[u8; 16], b64_text: &str) -> Result<Vec<u8>, DecodeError> {
    let ciphertext = B64.decode(b64_text.trim())?;
    let pt = Aes128CbcDec::new(key.into(), &ZERO_IV.into())
        .decrypt_padded_vec::<Pkcs7>(&ciphertext)
        .map_err(|_| DecodeError::Crypto)?;
    Ok(pt)
}

/// request: base64 -> aes-decrypt -> JSON
///
/// Mirrors `smon_decryptor.js` `decrypt_request`. Kept as public protocol API
/// (and exercised by tests); the proxy only needs the response path today.
#[allow(dead_code)]
pub fn decrypt_request(key: &[u8; 16], b64_text: &str) -> Result<serde_json::Value, DecodeError> {
    let plain = aes_decrypt(key, b64_text)?;
    Ok(serde_json::from_slice(&plain)?)
}

/// response: base64 -> aes-decrypt -> zlib.inflate -> JSON
pub fn decrypt_response(key: &[u8; 16], b64_text: &str) -> Result<serde_json::Value, DecodeError> {
    let deflated = aes_decrypt(key, b64_text)?;
    let mut inflated = Vec::new();
    ZlibDecoder::new(&deflated[..]).read_to_end(&mut inflated)?;
    Ok(serde_json::from_slice(&inflated)?)
}

/// Parse a 32-char hex string into the 16-byte key.
pub fn parse_key_hex(hex_str: &str) -> Result<[u8; 16], DecodeError> {
    let bytes = hex::decode(hex_str.trim())?;
    if bytes.len() != 16 {
        return Err(DecodeError::BadKeyLen(bytes.len()));
    }
    let mut k = [0u8; 16];
    k.copy_from_slice(&bytes);
    Ok(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hermetic fixtures generated with a DUMMY key (NOT the real game secret) via
    // Node's crypto/zlib — the same primitives as smon_decryptor.js — so this
    // proves the algorithm (aes-128-cbc zero-IV PKCS7 + zlib inflate + JSON)
    // without committing the real key. To verify the REAL key against a REAL
    // gateway_c2.php capture, see the env-gated test below.
    const DUMMY_KEY_HEX: &str = "00112233445566778899aabbccddeeff";
    // JSON {command:HubUserLogin, wizard_info{12345,TestWiz}, building_list[..], ..}
    // -> zlib deflate -> aes-128-cbc(zeroIV,PKCS7) -> base64
    const RESP_B64: &str = "zAYVdyALojp68uAvhtuqtRIhlIdDJryPGA82iZITqXXFJwVcgf1csJ0CO+4x+f/GPDXkffhYCEf2jkf8b4h0dpDJBEhKQh3HjyouxABNZq6jnrteRX3luV16rH7W4bNIMQTh3XEWlO6uCSCsZSCtskwrmBM5/Sb94zus2DibteBuY0kTlOg5lmVl1g2OUkkT";
    // same JSON but aes only (no deflate) — the request path
    const REQ_B64: &str = "7bwz6MoDwKaj+m/UX2UMW0Xicf4aGIRr6/E6oBhGBIB52PWj55ceIZd/kiooBzsE7I04lBdylCyS3s27VLgWDxe2DHYU8Eaw3lkqS3pf/S56Bl3DSehF+ey/Mjv0nRt4g3R+aBiTdICtRaUCZ2v33bQo8u17CdIRaLhSwHeQxDCleue30x+8p3Pf7XdKdNx00a7hQGnplUPjBHUweQKM5CH5ISFN4HMrKtpP/fmaGtXe3Logk6NS28cNbEymzrBPVfortgEEUTwS2Q6Iqn8FsV/HUTWAqbDtSyb6UfwydCM=";

    #[test]
    fn parse_key_hex_ok() {
        let k = parse_key_hex(DUMMY_KEY_HEX).unwrap();
        assert_eq!(k[0], 0x00);
        assert_eq!(k[15], 0xff);
    }

    #[test]
    fn parse_key_hex_trims_whitespace() {
        assert!(parse_key_hex("  00112233445566778899aabbccddeeff\n").is_ok());
    }

    #[test]
    fn parse_key_hex_rejects_wrong_length() {
        assert!(matches!(
            parse_key_hex("0011"),
            Err(DecodeError::BadKeyLen(2))
        ));
    }

    #[test]
    fn parse_key_hex_rejects_non_hex() {
        assert!(matches!(parse_key_hex("zz"), Err(DecodeError::Hex(_))));
    }

    #[test]
    fn decrypt_response_inflate_path() {
        let key = parse_key_hex(DUMMY_KEY_HEX).unwrap();
        let json = decrypt_response(&key, RESP_B64).unwrap();
        assert_eq!(json["command"], "HubUserLogin");
        assert_eq!(json["wizard_info"]["wizard_id"], 12345);
        assert_eq!(json["wizard_info"]["wizard_name"], "TestWiz");
        assert!(
            json.get("building_list").is_some(),
            "building_list must survive"
        );
    }

    #[test]
    fn decrypt_request_path() {
        let key = parse_key_hex(DUMMY_KEY_HEX).unwrap();
        let json = decrypt_request(&key, REQ_B64).unwrap();
        assert_eq!(json["command"], "HubUserLogin");
    }

    #[test]
    fn wrong_key_fails_cleanly() {
        // A different key must not panic — it errors (bad padding or bad inflate).
        let bad = [0u8; 16];
        assert!(decrypt_response(&bad, RESP_B64).is_err());
    }

    // Real-key / real-capture verification. Skipped unless you provide both:
    //   SWEX_KEY        = 32-char hex of the real game key
    //   SWEX_CAPTURE    = path to a file containing the base64 body of a real
    //                     gateway_c2.php *response* (HubUserLogin/GuestLogin).
    // This is the strict bar from the project brief ("descifrar JSON real").
    #[test]
    fn real_capture_decrypts_when_provided() {
        let (Ok(key_hex), Ok(cap_path)) =
            (std::env::var("SWEX_KEY"), std::env::var("SWEX_CAPTURE"))
        else {
            eprintln!("skipped: set SWEX_KEY and SWEX_CAPTURE to run the real-traffic check");
            return;
        };
        let key = parse_key_hex(&key_hex).expect("SWEX_KEY must be 32 hex chars");
        let b64 = std::fs::read_to_string(&cap_path).expect("read SWEX_CAPTURE");
        let json = decrypt_response(&key, &b64).expect("real capture must decrypt");
        assert!(
            json.get("command").is_some(),
            "decrypted JSON must have a command"
        );
    }
}
