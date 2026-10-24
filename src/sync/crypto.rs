//! AES-256-GCM with independent random salt/nonce for every secret revision.
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    aead, pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
const ITERATIONS: u32 = 600_000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Sealed {
    pub version: u32,
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}
fn key(phrase: &str, salt: &[u8]) -> Result<aead::LessSafeKey> {
    let mut bytes = [0; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        std::num::NonZeroU32::new(ITERATIONS).unwrap(),
        salt,
        phrase.as_bytes(),
        &mut bytes,
    );
    let key = aead::UnboundKey::new(&aead::AES_256_GCM, &bytes)
        .map_err(|_| anyhow::anyhow!("Cannot create sync key"));
    bytes.fill(0);
    Ok(aead::LessSafeKey::new(key?))
}
pub fn seal(phrase: &str, identity: &str, plain: &[u8]) -> Result<Sealed> {
    ensure!(
        phrase.chars().count() >= 12,
        "同步口令至少需要 12 个字符 / Sync passphrase needs at least 12 characters"
    );
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    let rng = SystemRandom::new();
    rng.fill(&mut salt)
        .and_then(|()| rng.fill(&mut nonce))
        .map_err(|_| anyhow::anyhow!("Cannot generate sync nonce"))?;
    let mut data = plain.to_vec();
    key(phrase, &salt)?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(identity.as_bytes()),
            &mut data,
        )
        .map_err(|_| anyhow::anyhow!("Cannot encrypt sync credentials"))?;
    Ok(Sealed {
        version: 1,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(data),
    })
}
pub fn open(phrase: &str, identity: &str, sealed: &Sealed) -> Result<Vec<u8>> {
    ensure!(sealed.version == 1, "Unsupported encrypted sync format");
    let salt = STANDARD.decode(&sealed.salt)?;
    let nonce: [u8; 12] = STANDARD
        .decode(&sealed.nonce)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid sync nonce"))?;
    ensure!(salt.len() == 16, "Invalid sync salt");
    let mut data = STANDARD.decode(&sealed.ciphertext)?;
    let plain = key(phrase, &salt)?
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(identity.as_bytes()),
            &mut data,
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "同步口令不正确或密文已损坏 / Wrong sync passphrase or damaged encrypted data"
            )
        })?;
    Ok(plain.to_vec())
}
