// QMC1 mask adapted from Presburger/qmc-decoder (MIT).
// QMC2 cipher and key envelope adapted from nukemiko/libtakiyasha (MIT).
// Full notices are in licenses/QQ-Music-adapters.txt.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

const MAX_KEY: u64 = 16 * 1024;
pub(super) enum Cipher {
    Legacy([u8; 128]),
    Map(Vec<u8>),
    Rc4 {
        key: Vec<u8>,
        state: Vec<u8>,
        hash: u32,
    },
}
impl Cipher {
    pub(super) fn legacy() -> Self {
        let table = [
            [0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52],
            [0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e],
            [0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51],
            [0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9],
            [0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0],
            [0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4],
            [0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92],
            [0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1],
        ];
        let (mut x, mut y, mut dx) = (-1_i32, 8_usize, 1_i32);
        let mut mask = [0; 128];
        for byte in &mut mask {
            *byte = if x < 0 {
                dx = 1;
                y = (8 - y) % 8;
                0xc3
            } else if x > 6 {
                dx = -1;
                y = 7 - y;
                0xd8
            } else {
                table[y][x as usize]
            };
            x += dx;
        }
        Self::Legacy(mask)
    }
    pub(super) fn from_key(key: Vec<u8>) -> Result<Self> {
        ensure!(
            !key.is_empty() && key.len() <= 4096,
            "invalid QQ music key length"
        );
        if key.len() <= 300 {
            return Ok(Self::Map(key));
        }
        ensure!(!key.contains(&0), "unsupported QQ music RC4 key");
        let mut state: Vec<u8> = (0..key.len()).map(|i| i as u8).collect();
        let mut j = 0;
        for i in 0..key.len() {
            j = (j + state[i] as usize + key[i] as usize) % key.len();
            state.swap(i, j);
        }
        let mut hash = 1_u32;
        for &byte in &key {
            let next = hash.wrapping_mul(byte as u32);
            if next <= hash {
                break;
            }
            hash = next;
        }
        Ok(Self::Rc4 { key, state, hash })
    }
    pub(super) fn apply(&mut self, bytes: &mut [u8], offset: u64) {
        match self {
            Self::Legacy(mask) => {
                for (i, byte) in bytes.iter_mut().enumerate() {
                    let pos = offset + i as u64;
                    let pos = if pos > 0x7fff { pos % 0x7fff } else { pos };
                    *byte ^= mask[pos as usize % 128];
                }
            }
            Self::Map(key) => {
                for (i, byte) in bytes.iter_mut().enumerate() {
                    let pos = offset + i as u64;
                    let pos = if pos > 0x7fff { pos % 0x7fff } else { pos };
                    let index = ((pos * pos + 71214) % key.len() as u64) as usize;
                    let rotate = ((index & 7) + 4) % 8;
                    // The format uses equal left/right shifts, not rotate_left.
                    *byte ^=
                        (((key[index] as u16) << rotate) | ((key[index] as u16) >> rotate)) as u8;
                }
            }
            Self::Rc4 { key, state, hash } => {
                let skip = |value: u64| -> usize {
                    ((*hash as f64 / ((value + 1) as f64 * key[value as usize % key.len()] as f64)
                        * 100.0) as u64
                        % key.len() as u64) as usize
                };
                let mut done = 0;
                while done < bytes.len() {
                    let pos = offset + done as u64;
                    if pos < 128 {
                        bytes[done] ^= key[skip(pos)];
                        done += 1;
                        continue;
                    }
                    let count = (5120 - pos as usize % 5120).min(bytes.len() - done);
                    let discard = pos as usize % 5120 + skip(pos / 5120);
                    let mut box_ = state.clone();
                    let (mut j, mut k) = (0, 0);
                    for i in 0..discard + count {
                        j = (j + 1) % key.len();
                        k = (k + box_[j] as usize) % key.len();
                        box_.swap(j, k);
                        if i >= discard {
                            bytes[done + i - discard] ^=
                                box_[(box_[j] as usize + box_[k] as usize) % key.len()];
                        }
                    }
                    done += count;
                }
            }
        }
    }
}

pub(super) fn unwrap_key(ekey: &[u8]) -> Result<Vec<u8>> {
    ensure!(ekey.len() as u64 <= MAX_KEY, "QQ music ekey is too large");
    let data = STANDARD
        .decode(ekey)
        .context("invalid QQ music ekey encoding")?;
    ensure!(
        !data.starts_with(b"QQMusic EncV2,Key:"),
        "QQ music EncV2 key envelope is not supported yet"
    );
    ensure!(data.len() >= 24, "QQ music ekey is truncated");
    let mut tea_key = [0; 16];
    for i in 0..8 {
        tea_key[i * 2] = ((106.0 + i as f64 * 0.1).tan().abs() * 100.0) as u8;
        tea_key[i * 2 + 1] = data[i];
    }
    let decoded = tc_tea::decrypt(&data[8..], tea_key).context("invalid QQ music ekey")?;
    let mut key = data[..8].to_vec();
    key.extend(decoded);
    Ok(key)
}

fn external_key(source: &Path, provided: Option<&str>) -> Result<Vec<u8>> {
    if let Some(key) = provided {
        return unwrap_key(key.trim().as_bytes());
    }
    let mut name = source.as_os_str().to_os_string();
    name.push(".ekey");
    let file = File::open(PathBuf::from(name)).map_err(|_| anyhow::anyhow!(
        "QQ music needs a song key: place its Base64 ekey in <original filename>.ekey beside the audio file"))?;
    ensure!(
        file.metadata()?.len() <= MAX_KEY,
        "QQ music ekey file is too large"
    );
    let mut text = String::new();
    file.take(MAX_KEY + 1)
        .read_to_string(&mut text)
        .context("read QQ music ekey file")?;
    unwrap_key(text.trim().as_bytes())
}

pub(super) fn probe(
    input: &mut File,
    length: u64,
    source: &Path,
    provided: Option<&str>,
) -> Result<(Cipher, u64)> {
    input.seek(SeekFrom::End(-16))?;
    let mut tail = [0; 16];
    input.read_exact(&mut tail)?;
    if &tail[8..] == b"musicex\0" {
        let size = u32::from_le_bytes(tail[..4].try_into().unwrap()) as u64;
        let version = u32::from_le_bytes(tail[4..8].try_into().unwrap());
        ensure!(version == 1, "unsupported QQ music musicex version");
        ensure!(
            (192..=65536).contains(&size) && size + 16 < length,
            "invalid QQ music musicex footer"
        );
        return Ok((
            Cipher::from_key(external_key(source, provided)?)?,
            length - size - 16,
        ));
    }
    if &tail[12..] == b"QTag" || &tail[12..] == b"STag" {
        let size = u32::from_be_bytes(tail[8..12].try_into().unwrap()) as u64;
        ensure!(
            size > 0 && size <= MAX_KEY && size + 8 < length,
            "invalid QQ music tag length"
        );
        let payload = length - size - 8;
        let key = if &tail[12..] == b"STag" || provided.is_some() {
            external_key(source, provided)?
        } else {
            input.seek(SeekFrom::Start(payload))?;
            let mut tag = vec![0; size as usize];
            input.read_exact(&mut tag)?;
            let parts: Vec<_> = tag.split(|&v| v == b',').collect();
            ensure!(parts.len() == 3, "invalid QQ music QTag");
            unwrap_key(parts[0])?
        };
        return Ok((Cipher::from_key(key)?, payload));
    }
    let extension = source
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if extension.starts_with("qmc") {
        return Ok((Cipher::legacy(), length));
    }
    let size = u32::from_le_bytes(tail[12..].try_into().unwrap()) as u64;
    if size > 0 && size <= MAX_KEY && size + 4 < length {
        let payload = length - size - 4;
        input.seek(SeekFrom::Start(payload))?;
        let mut encoded = vec![0; size as usize];
        input.read_exact(&mut encoded)?;
        let key = if provided.is_some() {
            external_key(source, provided)?
        } else {
            unwrap_key(&encoded)?
        };
        return Ok((Cipher::from_key(key)?, payload));
    }
    bail!("unsupported QQ music footer: cannot determine the encryption variant")
}
