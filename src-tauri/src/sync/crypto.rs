//! 同步包二进制格式与加密。
//!
//! 布局（全部小端）：
//! ```text
//! magic "PTSYNC01" (8)
//! iterations u32 LE (4)
//! salt (16)
//! nonce (12)
//! ciphertext + tag (AES-256-GCM)
//! ```
//!
//! - 密钥：`pbkdf2_hmac::<Sha256>(sync_passphrase, salt, iterations, 32)`，
//!   上传时 iterations 固定 [`ITERATIONS`]（600_000）。
//! - AAD：`magic || iterations LE || salt`（28 字节），把头部参数绑进认证范围——
//!   篡改任一头部字节都会让 GCM 认证失败。
//! - 每次上传都生成新的随机 salt 与 nonce。
//!
//! 解密失败（认证失败 / 截断 / 参数异常）统一返回 `sync_decrypt_failed`，
//! 不回显底层错误细节、不泄漏密文片段。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::Sha256;

use super::SyncError;

/// 包魔数。
pub const MAGIC: &[u8; 8] = b"PTSYNC01";
/// 上传时使用的 PBKDF2 迭代次数。
pub const ITERATIONS: u32 = 600_000;
/// salt 长度。
pub const SALT_LEN: usize = 16;
/// nonce 长度。
pub const NONCE_LEN: usize = 12;
/// 头部长度 = magic + iterations + salt + nonce。
pub const HEADER_LEN: usize = 8 + 4 + SALT_LEN + NONCE_LEN;
/// GCM 认证标签长度。
pub const TAG_LEN: usize = 16;
/// 解密时允许的迭代次数上限（防御被构造的超大迭代导致长时间阻塞）。
const MAX_ITERATIONS: u32 = 10_000_000;

/// 由口令与 salt 派生 32 字节密钥。
fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut key);
    key
}

/// 构造 AAD：`magic || iterations LE || salt`。
fn aad(iterations: u32, salt: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(8 + 4 + SALT_LEN);
    aad.extend_from_slice(MAGIC);
    aad.extend_from_slice(&iterations.to_le_bytes());
    aad.extend_from_slice(salt);
    aad
}

/// 用给定迭代次数加密（生产入口固定 [`ITERATIONS`]；测试用低迭代避免拖慢 CI）。
fn encrypt_with_iterations(
    passphrase: &str,
    plaintext: &[u8],
    iterations: u32,
) -> Result<Vec<u8>, SyncError> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase, &salt, iterations);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| SyncError::internal("同步包加密初始化失败"))?;
    let aad = aad(iterations, &salt);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| SyncError::internal("同步包加密失败"))?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&iterations.to_le_bytes());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// 加密载荷：每次调用生成新的随机 salt / nonce。
pub fn encrypt(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
    if passphrase.is_empty() {
        return Err(SyncError::passphrase_missing());
    }
    encrypt_with_iterations(passphrase, plaintext, ITERATIONS)
}

/// 解密载荷。任何异常（魔数 / 参数 / 截断 / 认证失败）统一为 `sync_decrypt_failed`。
pub fn decrypt(passphrase: &str, data: &[u8]) -> Result<Vec<u8>, SyncError> {
    if passphrase.is_empty() {
        return Err(SyncError::passphrase_missing());
    }
    // 头部 + 至少一个 tag 才算一个完整包；不足一律按解密失败处理（不回显长度）。
    if data.len() < HEADER_LEN + TAG_LEN {
        return Err(SyncError::decrypt_failed());
    }
    if &data[..8] != MAGIC {
        return Err(SyncError::decrypt_failed());
    }
    let iterations = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    if iterations == 0 || iterations > MAX_ITERATIONS {
        return Err(SyncError::decrypt_failed());
    }
    let salt = &data[12..12 + SALT_LEN];
    let nonce_bytes = &data[12 + SALT_LEN..HEADER_LEN];
    let ciphertext = &data[HEADER_LEN..];

    let key = derive_key(passphrase, salt, iterations);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SyncError::decrypt_failed())?;
    let aad = aad(iterations, salt);
    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| SyncError::decrypt_failed())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用低迭代口令派生，避免每次加解密都跑 60 万次 PBKDF2。
    const TEST_ITERATIONS: u32 = 1_000;
    const PASSPHRASE: &str = "correct horse battery staple";

    fn sample() -> Vec<u8> {
        serde_json::json!({
            "version": 1,
            "exported_at": "2026-01-01T00:00:00.000000Z",
            "history": [{"work_id": 1, "kind": "illust"}],
            "accounts": null
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn production_roundtrip() {
        let plaintext = sample();
        let packed = encrypt(PASSPHRASE, &plaintext).unwrap();
        // 头部形状：magic + 固定迭代次数
        assert_eq!(&packed[..8], MAGIC);
        assert_eq!(
            u32::from_le_bytes([packed[8], packed[9], packed[10], packed[11]]),
            ITERATIONS
        );
        assert!(packed.len() > HEADER_LEN + TAG_LEN);
        let restored = decrypt(PASSPHRASE, &packed).unwrap();
        assert_eq!(restored, plaintext);
        // 每次加密的 salt / nonce 必须不同
        let again = encrypt(PASSPHRASE, &plaintext).unwrap();
        assert_ne!(&packed[12..HEADER_LEN], &again[12..HEADER_LEN]);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let packed = encrypt_with_iterations(PASSPHRASE, &sample(), TEST_ITERATIONS).unwrap();
        let error = decrypt("wrong passphrase", &packed).unwrap_err();
        assert_eq!(error.code, "sync_decrypt_failed");
        // 错误文案不回显底层细节
        assert!(!error.message.contains("wrong passphrase"));
    }

    /// 头部任一字节篡改都必须失败（每次仍在合法迭代次数范围内也仍然失败）。
    #[test]
    fn tampered_header_byte_fails() {
        let packed = encrypt_with_iterations(PASSPHRASE, &sample(), TEST_ITERATIONS).unwrap();
        for index in 0..HEADER_LEN {
            let mut tampered = packed.clone();
            tampered[index] ^= 0x01;
            assert_eq!(
                decrypt(PASSPHRASE, &tampered).unwrap_err().code,
                "sync_decrypt_failed",
                "头部第 {index} 字节被篡改仍解密成功"
            );
        }
        // 密文 / tag 篡改同样失败
        let last = packed.len() - 1;
        let mut tampered = packed.clone();
        tampered[last] ^= 0x01;
        assert_eq!(
            decrypt(PASSPHRASE, &tampered).unwrap_err().code,
            "sync_decrypt_failed"
        );
    }

    /// 截断（含只剩头部）与空输入都按解密失败处理，且不回显底层错误。
    #[test]
    fn truncated_or_empty_fails() {
        let packed = encrypt_with_iterations(PASSPHRASE, &sample(), TEST_ITERATIONS).unwrap();
        for len in [
            0,
            1,
            HEADER_LEN - 1,
            HEADER_LEN,
            HEADER_LEN + TAG_LEN - 1,
            packed.len() - 1,
        ] {
            let error = decrypt(PASSPHRASE, &packed[..len]).unwrap_err();
            assert_eq!(error.code, "sync_decrypt_failed", "长度 {len} 未按失败处理");
        }
        // 魔数错误
        let mut bad = packed.clone();
        bad[0] = b'X';
        assert_eq!(
            decrypt(PASSPHRASE, &bad).unwrap_err().code,
            "sync_decrypt_failed"
        );
        // 迭代次数为 0 / 超上限：参数异常直接失败，不做昂贵的口令派生
        let mut zero = packed.clone();
        zero[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            decrypt(PASSPHRASE, &zero).unwrap_err().code,
            "sync_decrypt_failed"
        );
        let mut huge = packed.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decrypt(PASSPHRASE, &huge).unwrap_err().code,
            "sync_decrypt_failed"
        );
    }

    #[test]
    fn empty_passphrase_is_rejected() {
        assert_eq!(
            encrypt("", b"x").unwrap_err().code,
            "sync_passphrase_missing"
        );
        assert_eq!(
            decrypt("", b"0123456789").unwrap_err().code,
            "sync_passphrase_missing"
        );
    }
}
