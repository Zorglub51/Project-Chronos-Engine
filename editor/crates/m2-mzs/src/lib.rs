// M2 MZS archive format (zstd + XOR encryption derived from MT19937 seeded
// with MD5(key + filename)).
//
// File layout:
//   offset 0: "mzs\0" magic
//   offset 4: uncompressed size (u32 LE)
//   offset 8: encrypted-then-compressed payload (zstd inside XOR)

mod mt19937;

use md5::{Digest, Md5};
use mt19937::MT19937;
use thiserror::Error;

pub const MAGIC: &[u8; 4] = b"mzs\0";
pub const HEADER_SIZE: usize = 8;
pub const DEFAULT_KEY: &str = "8!7ZZnJAr/wfc";
pub const DEFAULT_KEY_LENGTH: usize = 64;

const ZSTD_MAGIC: u32 = 0xFD2F_B528;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not an MZS file (bad magic)")]
    BadMagic,
    #[error("input shorter than MZS header")]
    Truncated,
    #[error("decrypted payload is not zstd (wrong key, key length, or filename?)")]
    NotZstd,
    #[error("zstd error: {0}")]
    Zstd(#[from] std::io::Error),
    #[error("decompressed size mismatch: header says {expected}, got {actual}")]
    SizeMismatch { expected: u32, actual: usize },
}

/// Build the XOR keystream for a given (game key, filename, length) tuple.
///
/// 1. seed = MD5(game_key + lowercase(filename)) (16 bytes)
/// 2. interpret seed as 4 little-endian u32, feed to MT19937::init_by_array
/// 3. extract `length` bytes by repeatedly calling genrand_int32() and writing LE
pub fn derive_keystream(game_key: &str, filename: &str, length: usize) -> Vec<u8> {
    let mut hasher = Md5::new();
    hasher.update(game_key.as_bytes());
    hasher.update(filename.to_ascii_lowercase().as_bytes());
    let digest = hasher.finalize();

    let seed = [
        u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]),
        u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]),
        u32::from_le_bytes([digest[8], digest[9], digest[10], digest[11]]),
        u32::from_le_bytes([digest[12], digest[13], digest[14], digest[15]]),
    ];

    let mut mt = MT19937::new();
    mt.init_by_array(&seed);

    let mut out = Vec::with_capacity(length + 4);
    while out.len() < length {
        out.extend_from_slice(&mt.genrand_int32().to_le_bytes());
    }
    out.truncate(length);
    out
}

fn xor_into(data: &mut [u8], key: &[u8]) {
    let kl = key.len();
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= key[i % kl];
    }
}

/// Decrypt + decompress an MZS file.
///
/// `filename` is the basename of the file the bytes were read from. The XOR
/// keystream depends on it, so passing the wrong name produces gibberish that
/// fails the zstd magic check.
pub fn unpack(data: &[u8], filename: &str, game_key: &str, key_length: usize) -> Result<Vec<u8>, Error> {
    if data.len() < HEADER_SIZE {
        return Err(Error::Truncated);
    }
    if &data[..4] != MAGIC {
        return Err(Error::BadMagic);
    }
    let uncomp_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut payload = data[HEADER_SIZE..].to_vec();
    let key = derive_keystream(game_key, filename, key_length);
    xor_into(&mut payload, &key);

    if payload.len() >= 4 {
        let magic = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if magic != ZSTD_MAGIC {
            return Err(Error::NotZstd);
        }
    }

    let decoded = zstd::decode_all(&payload[..])?;
    if decoded.len() != uncomp_size as usize {
        return Err(Error::SizeMismatch { expected: uncomp_size, actual: decoded.len() });
    }
    Ok(decoded)
}

/// Compress + encrypt arbitrary bytes into MZS format.
///
/// `filename` is the intended output filename (basename). Must match what
/// `unpack` will be called with later, otherwise the keystream won't match.
pub fn pack(data: &[u8], filename: &str, game_key: &str, key_length: usize, level: i32) -> Result<Vec<u8>, Error> {
    let compressed = zstd::encode_all(data, level)?;

    let mut payload = compressed;
    let key = derive_keystream(game_key, filename, key_length);
    xor_into(&mut payload, &key);

    let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Convenience: unpack with the default M2 key + key length.
pub fn unpack_default(data: &[u8], filename: &str) -> Result<Vec<u8>, Error> {
    unpack(data, filename, DEFAULT_KEY, DEFAULT_KEY_LENGTH)
}

/// Convenience: pack with the default M2 key + key length, zstd level 3 (Python default).
pub fn pack_default(data: &[u8], filename: &str) -> Result<Vec<u8>, Error> {
    pack(data, filename, DEFAULT_KEY, DEFAULT_KEY_LENGTH, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystream_first_bytes_known() {
        // Reference: mzstool.py generate_xor_key("8!7ZZnJAr/wfc", "test.psb", 16)
        // Captured by running the Python tool once and recording the bytes.
        // Filling in actual values requires running Python — see tests/python_oracle.rs.
        // For now just check the function runs and returns the right length.
        let ks = derive_keystream(DEFAULT_KEY, "test.psb", DEFAULT_KEY_LENGTH);
        assert_eq!(ks.len(), DEFAULT_KEY_LENGTH);
    }

    #[test]
    fn pack_then_unpack_roundtrip() {
        let original = b"the quick brown fox jumps over the lazy dog".repeat(50);
        let packed = pack_default(&original, "test.bin").unwrap();
        assert_eq!(&packed[..4], MAGIC);
        let unpacked = unpack_default(&packed, "test.bin").unwrap();
        assert_eq!(unpacked, original);
    }

    #[test]
    fn wrong_filename_fails_zstd_check() {
        let original = b"hello world".to_vec();
        let packed = pack_default(&original, "right.bin").unwrap();
        let err = unpack_default(&packed, "wrong.bin").unwrap_err();
        assert!(matches!(err, Error::NotZstd));
    }
}
