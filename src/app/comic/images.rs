//! Share background-prepared pixels across texture eviction and concurrent loads.
use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use anyhow::Result;
use std::{
    path::Path,
    sync::{Mutex, OnceLock, Weak},
    time::SystemTime,
};

pub(super) struct Prepared {
    pub path: PathBuf,
    pub hash: String,
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Arc<egui::ColorImage>>,
}
impl Prepared {
    pub fn bitmap(bitmap: Bitmap) -> Self {
        Self {
            path: PathBuf::new(),
            hash: String::new(),
            width: bitmap.width,
            height: bitmap.height,
            tiles: bitmap
                .tiles
                .into_iter()
                .map(|tile| {
                    Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                        [tile.width() as usize, tile.height() as usize],
                        tile.bytes(),
                    ))
                })
                .collect(),
        }
    }
    fn bytes(&self) -> usize {
        self.tiles.iter().map(|t| t.pixels.len() * 4).sum()
    }
}
type Key = (PathBuf, u32);
type Signature = (u64, SystemTime);
struct Entry {
    image: Arc<Prepared>,
    signature: Signature,
    used: u64,
}
struct Cache {
    entries: HashMap<Key, Entry>,
    flights: HashMap<Key, Weak<tokio::sync::Mutex<()>>>,
    clock: u64,
    bytes: usize,
    budget: usize,
}
impl Cache {
    fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            flights: HashMap::new(),
            clock: 0,
            bytes: 0,
            budget,
        }
    }
    fn get(&mut self, key: &Key, signature: Option<Signature>) -> Option<Arc<Prepared>> {
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(key) {
            if Some(entry.signature) == signature {
                entry.used = self.clock;
                return Some(entry.image.clone());
            }
            self.bytes -= entry.image.bytes();
            self.entries.remove(key);
        }
        None
    }
    fn insert(&mut self, key: Key, image: Arc<Prepared>, signature: Signature) {
        if image.bytes() > self.budget {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.image.bytes();
        }
        while self.bytes + image.bytes() > self.budget {
            let Some(old) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.bytes -= self.entries.remove(&old).unwrap().image.bytes();
        }
        self.clock += 1;
        self.bytes += image.bytes();
        self.entries.insert(
            key,
            Entry {
                image,
                signature,
                used: self.clock,
            },
        );
    }
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::new(128 * 1024 * 1024)))
}
fn signature(path: &Path) -> Option<Signature> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}
fn flight(key: &Key) -> Arc<tokio::sync::Mutex<()>> {
    let mut cache = cache().lock().unwrap();
    cache.flights.retain(|_, weak| weak.strong_count() > 0);
    if let Some(gate) = cache.flights.get(key).and_then(Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    cache.flights.insert(key.clone(), Arc::downgrade(&gate));
    gate
}
fn lookup(key: &Key) -> Option<Arc<Prepared>> {
    let signature = signature(&key.0);
    cache().lock().unwrap().get(key, signature)
}
fn prepare(image: CachedImage, width: u32) -> Arc<Prepared> {
    let mut result = Prepared::bitmap(image.bitmap);
    result.path = image.path;
    result.hash = image.hash;
    let result = Arc::new(result);
    if let Some(signature) = signature(&result.path) {
        cache()
            .lock()
            .unwrap()
            .insert((result.path.clone(), width), result.clone(), signature);
    }
    result
}
pub(super) async fn fetch(
    directory: PathBuf,
    key: String,
    request: domain::source::ImageRequest,
    width: u32,
) -> Result<Arc<Prepared>> {
    let path = directory.join(format!("{}.img", domain::digest(key.as_bytes())));
    let key_path = (path, width);
    let gate = flight(&key_path);
    let _guard = gate.lock().await;
    if let Some(image) = tokio::task::spawn_blocking(move || lookup(&key_path)).await? {
        return Ok(image);
    }
    let image = domain::cache::fetch(&directory, &key, request, width).await?;
    domain::cache::background(move || Ok(prepare(image, width))).await
}
pub(super) async fn load(path: PathBuf, width: u32) -> Result<Arc<Prepared>> {
    let key = (path.clone(), width);
    let gate = flight(&key);
    let _guard = gate.lock().await;
    domain::cache::background(move || {
        if let Some(image) = lookup(&key) {
            return Ok(image);
        }
        Ok(prepare(domain::cache::load(path, width)?, width))
    })
    .await
}
pub(super) fn clear() {
    let mut cache = cache().lock().unwrap();
    cache.entries.clear();
    cache.bytes = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn prepared() -> Arc<Prepared> {
        Arc::new(Prepared {
            path: PathBuf::new(),
            hash: String::new(),
            width: 2,
            height: 2,
            tiles: vec![Arc::new(egui::ColorImage::new(
                [2, 2],
                vec![Color32::WHITE; 4],
            ))],
        })
    }
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
    #[test]
    fn prepared_pixels_use_a_byte_budget_and_lru_invalidation() {
        let mut cache = Cache::new(32);
        let key = |name: &str| (PathBuf::from(name), 1800);
        let signature = (10, SystemTime::UNIX_EPOCH);
        cache.insert(key("a"), prepared(), signature);
        cache.insert(key("b"), prepared(), signature);
        assert!(cache.get(&key("a"), Some(signature)).is_some());
        cache.insert(key("c"), prepared(), signature);
        assert!(cache.get(&key("b"), Some(signature)).is_none());
        assert_eq!(cache.bytes, 32);
        assert!(cache.get(&key("a"), None).is_none());
        assert_eq!(cache.bytes, 16);
        assert!(
            cache
                .get(&key("c"), Some((20, SystemTime::UNIX_EPOCH)))
                .is_none()
        );
        assert_eq!(cache.bytes, 0);
    }
    #[test]
    fn concurrent_page_loads_share_one_download_and_prepared_pixels() {
        runtime().block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let app = axum::Router::new().route(
                "/image",
                axum::routing::get(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        let mut bytes = std::io::Cursor::new(Vec::new());
                        crate::pixels::black(32, 64)
                            .write_to(&mut bytes, "png")
                            .unwrap();
                        bytes.into_inner()
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url: reqwest::Url = format!("http://{}/image", listener.local_addr().unwrap())
                .parse()
                .unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let directory = PathBuf::from("target/comic-performance/tests")
                .join(uuid::Uuid::new_v4().to_string());
            let (a, b) = tokio::join!(
                fetch(directory.clone(), "page".into(), url.clone().into(), 1800),
                fetch(directory.clone(), "page".into(), url.into(), 1800)
            );
            let (a, b) = (a.unwrap(), b.unwrap());
            assert!(Arc::ptr_eq(&a, &b));
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert!(Arc::ptr_eq(&a, &load(a.path.clone(), 1800).await.unwrap()));
            assert!(!Arc::ptr_eq(&a, &load(a.path.clone(), 240).await.unwrap()));
            std::fs::remove_file(&a.path).unwrap();
            assert!(load(a.path.clone(), 1800).await.is_err());
            std::fs::remove_dir_all(directory).unwrap();
            server.abort();
        });
    }
    #[test]
    #[ignore = "Benchmarks the local real-world comic page without network access"]
    fn prepared_image_cache_benchmark() {
        runtime().block_on(async {
            let path = std::fs::read_dir("target/komiic-research/live-cache").unwrap().flatten().next().unwrap().path();
            clear();
            let start = Instant::now();
            let image = load(path.clone(),1800).await.unwrap();
            let cold = start.elapsed().as_secs_f64()*1000.0;
            let start = Instant::now();
            for _ in 0..100 { assert!(Arc::ptr_eq(&image,&load(path.clone(),1800).await.unwrap())); }
            let warm = start.elapsed().as_secs_f64()*10.0;
            println!("Prepared Komiic page: cold={cold:.3}ms, warm={warm:.3}ms (100 cache hits; no decode or conversion)");
            std::fs::create_dir_all("target/comic-performance").unwrap();
            std::fs::write("target/comic-performance/prepared-benchmark.json",serde_json::to_vec_pretty(&serde_json::json!({"cold_ms":cold,"warm_ms":warm})).unwrap()).unwrap();
        });
    }
}
