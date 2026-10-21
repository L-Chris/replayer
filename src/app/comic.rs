use super::*;
use crate::{
    comics::{
        self as domain, Book, Chapter, Detail, ReadingMode, SearchGroup,
        cache::{Bitmap, CachedImage},
        library::{Entry, Library, Progress},
        source::{ComicSource, MangaYun},
    },
    jobs::Job,
};
use std::collections::{HashSet, VecDeque};
mod catalog;
mod preferences;
mod reader;

const PAGE_CONCURRENCY: usize = 4;

fn page_requests(current: usize, count: usize, visible: Vec<usize>) -> Vec<usize> {
    let mut requested = Vec::new();
    for index in std::iter::once(current)
        .chain(visible)
        .chain((1..=3).map(|ahead| current.saturating_add(ahead)))
        .chain(current.checked_sub(1))
    {
        if index < count && !requested.contains(&index) {
            requested.push(index);
        }
    }
    requested
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Shelf,
    Search,
    Updates,
    History,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Catalog,
    Detail,
    Reader,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Display {
    Original,
    Translated,
    Compare,
}
struct Picture {
    height: f32,
    tiles: Vec<egui::TextureHandle>,
}
impl Picture {
    fn bytes(&self) -> usize {
        self.tiles
            .iter()
            .map(|t| t.size()[0] * t.size()[1] * 4)
            .sum()
    }
    fn new(ctx: &egui::Context, id: &str, bitmap: Bitmap) -> Self {
        let tiles = bitmap
            .tiles
            .into_iter()
            .enumerate()
            .map(|(i, tile)| {
                ctx.load_texture(
                    format!("{id}-{i}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [tile.width() as usize, tile.height() as usize],
                        tile.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                )
            })
            .collect();
        Self {
            height: bitmap.height as f32,
            tiles,
        }
    }
    fn paint(&self, ui: &Ui, rect: Rect) {
        let mut y = rect.min.y;
        for tile in &self.tiles {
            let height = rect.height() * tile.size()[1] as f32 / self.height;
            let part = Rect::from_min_size(pos2(rect.min.x, y), vec2(rect.width(), height));
            if ui.clip_rect().intersects(part) {
                ui.painter().image(
                    tile.id(),
                    part,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            y += height;
        }
    }
}
struct Asset {
    path: PathBuf,
    hash: String,
    original: Picture,
    translated: Option<Picture>,
}
struct ImageResult {
    original: CachedImage,
    translated: Option<CachedImage>,
}
struct Reader {
    book: Book,
    chapter: Chapter,
    index: usize,
    urls: Vec<String>,
    current: usize,
    fraction: f32,
    mode: ReadingMode,
    zoom: f32,
    display: Display,
    sidebar: bool,
    sidebar_follow: bool,
    restore: bool,
    dirty: bool,
    completed: bool,
    assets: HashMap<usize, Asset>,
    jobs: HashMap<usize, Job<ImageResult>>,
    errors: HashMap<usize, String>,
    sizes: HashMap<usize, (f32, f32)>,
    pages_job: Option<Job<Vec<String>>>,
    pages_error: Option<String>,
    last_saved: Instant,
    toolbar_until: Instant,
}
#[derive(Clone)]
struct TranslationRequest {
    book: String,
    chapter: String,
    index: usize,
    path: PathBuf,
    hash: String,
    context: String,
    settings: domain::Settings,
    language: Language,
    llm: crate::settings::LlmSettings,
}
struct TranslationJob {
    request: TranslationRequest,
    job: Job<PathBuf>,
}
pub(super) struct State {
    library: Option<Library>,
    source: Option<Arc<dyn ComicSource>>,
    sites: Vec<domain::Site>,
    site_job: Option<Job<Vec<domain::Site>>>,
    init_error: Option<String>,
    tab: Tab,
    view: View,
    keyword: String,
    site_filter: String,
    groups: Vec<SearchGroup>,
    search: Option<Job<Vec<SearchGroup>>>,
    selected: Option<Book>,
    detail_job: Option<Job<Detail>>,
    reader: Option<Reader>,
    status: String,
    covers: HashMap<String, Picture>,
    cover_jobs: HashMap<String, Job<CachedImage>>,
    cover_errors: HashSet<String>,
    reverse: bool,
    unread_only: bool,
    update_queue: VecDeque<Book>,
    update_job: Option<(String, Job<Detail>)>,
    update_done: usize,
    update_total: usize,
    update_after: Instant,
    automatic_checked: bool,
    translations: VecDeque<TranslationRequest>,
    translation: Option<TranslationJob>,
    translation_done: usize,
    translation_total: usize,
    translation_failed: usize,
}
impl State {
    pub(super) fn continue_tile(
        &mut self,
        ui: &mut Ui,
        width: f32,
        height: f32,
        settings: &crate::settings::Settings,
    ) -> bool {
        let l = settings.language;
        let entry = self
            .entries()
            .iter()
            .filter(|e| e.progress.is_some())
            .max_by_key(|e| e.last_read)
            .cloned();
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        ui.painter().rect_filled(rect, 12, SURFACE);
        let mut inner = workspace::child(ui, "continue-comic", rect.shrink(18.0));
        inner.style_mut().interaction.selectable_labels = false;
        inner.label(
            RichText::new(l.text("继续阅读", "Continue reading"))
                .color(ACCENT)
                .small(),
        );
        inner.add_space(12.0);
        if let Some(entry) = entry {
            self.ensure_cover(&entry.book);
            inner.horizontal_top(|ui| {
                let cover_width = (width * 0.32).min((height - 80.0) / 1.5);
                let (cover, _) =
                    ui.allocate_exact_size(vec2(cover_width, cover_width * 1.5), Sense::hover());
                if let Some(picture) = self.covers.get(&entry.book.key()) {
                    picture.paint(ui, cover);
                }
                ui.vertical(|ui| {
                    ui.set_max_width((width - cover_width - 60.0).max(60.0));
                    ui.add(
                        egui::Label::new(RichText::new(&entry.book.title).size(20.0).strong())
                            .truncate(),
                    );
                    ui.label(RichText::new(&entry.book.site_name).small().color(MUTED));
                    if let Some(progress) = &entry.progress {
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                l.text("读到第", "Page"),
                                progress.page + 1
                            ))
                            .color(MUTED),
                        );
                    }
                });
            });
            if response.clicked() {
                self.resume(entry, settings);
                return true;
            }
        } else {
            inner.label(
                RichText::new(l.text("故事，随时翻开", "A new story awaits"))
                    .size(22.0)
                    .strong(),
            );
            inner.label(
                RichText::new(l.text(
                    "发现漫画，收藏下一段旅程",
                    "Discover comics and save your next adventure",
                ))
                .color(MUTED),
            );
            inner.add_space(16.0);
            inner.label(RichText::new(l.text("浏览漫画 →", "Explore comics →")).color(ACCENT));
            if response.clicked() {
                self.search_for(String::new());
                return true;
            }
        }
        false
    }
    pub(super) fn reading(&self) -> bool {
        self.view == View::Reader
    }
    pub(super) fn toolbar_visible(&self) -> bool {
        self.reader
            .as_ref()
            .is_some_and(|r| Instant::now() < r.toolbar_until)
    }
    pub(super) fn detail_open(&self) -> bool {
        self.view == View::Detail
    }
    pub(super) fn search_for(&mut self, keyword: String) {
        self.flush();
        self.view = View::Catalog;
        self.tab = Tab::Search;
        self.keyword = keyword;
        if !self.keyword.trim().is_empty()
            && let Some(source) = self.source.clone()
        {
            let keyword = self.keyword.trim().to_owned();
            self.groups.clear();
            self.search = Some(Job::spawn(
                move |_| async move { source.search(&keyword).await },
            ));
        }
    }
    pub(super) fn home_preview(
        &mut self,
        ui: &mut Ui,
        settings: &crate::settings::Settings,
    ) -> bool {
        let entries: Vec<_> = self
            .entries()
            .iter()
            .filter(|e| e.progress.is_some())
            .cloned()
            .collect();
        self.preview_entries(ui, settings, entries)
    }
    pub(super) fn favorite_preview(
        &mut self,
        ui: &mut Ui,
        settings: &crate::settings::Settings,
    ) -> bool {
        let entries: Vec<_> = self
            .entries()
            .iter()
            .filter(|e| e.favorite)
            .cloned()
            .collect();
        self.preview_entries(ui, settings, entries)
    }
    fn preview_entries(
        &mut self,
        ui: &mut Ui,
        settings: &crate::settings::Settings,
        mut entries: Vec<Entry>,
    ) -> bool {
        if entries.is_empty() {
            return false;
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.last_read));
        ui.label(
            RichText::new(settings.language.text("漫画书架", "Comic library"))
                .size(21.0)
                .strong(),
        );
        ui.add_space(12.0);
        self.book_grid(
            ui,
            &entries
                .into_iter()
                .take(6)
                .map(|e| e.book)
                .collect::<Vec<_>>(),
            settings,
        )
    }
    pub(super) fn reload_images(&mut self) {
        if let Some(reader) = &mut self.reader {
            reader.assets.clear();
            reader.jobs.clear();
            reader.errors.clear();
        }
    }
    pub fn new() -> Self {
        let library = Library::open(&domain::root().join("library.sqlite"));
        let source = MangaYun::connect();
        let init_error = library
            .as_ref()
            .err()
            .map(ToString::to_string)
            .or_else(|| source.as_ref().err().map(ToString::to_string));
        let site_job = source.as_ref().ok().map(|source| {
            let source = source.clone();
            Job::spawn(move |_| async move { source.sites().await })
        });
        Self {
            sites: Vec::new(),
            site_job,
            library: library.ok(),
            source: source.ok(),
            init_error,
            tab: Tab::Shelf,
            view: View::Catalog,
            keyword: String::new(),
            site_filter: String::new(),
            groups: Vec::new(),
            search: None,
            selected: None,
            detail_job: None,
            reader: None,
            status: String::new(),
            covers: HashMap::new(),
            cover_jobs: HashMap::new(),
            cover_errors: HashSet::new(),
            reverse: false,
            unread_only: false,
            update_queue: VecDeque::new(),
            update_job: None,
            update_done: 0,
            update_total: 0,
            update_after: Instant::now(),
            automatic_checked: false,
            translations: VecDeque::new(),
            translation: None,
            translation_done: 0,
            translation_total: 0,
            translation_failed: 0,
        }
    }
    fn entries(&self) -> &[Entry] {
        self.library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or_default()
    }
    fn selected_entry(&self) -> Option<Entry> {
        let key = self.selected.as_ref()?.key();
        self.library.as_ref()?.get(&key).cloned()
    }
    fn save(&mut self, entry: Entry) {
        if let Some(library) = &mut self.library
            && let Err(error) = library.save(entry)
        {
            self.status = error.to_string();
        }
    }
    pub fn flush(&mut self) {
        let Some(reader) = &mut self.reader else {
            return;
        };
        if !reader.dirty || !reader.assets.contains_key(&reader.current) {
            return;
        }
        let Some(library) = &mut self.library else {
            return;
        };
        let Some(mut entry) = library.get(&reader.book.key()).cloned() else {
            return;
        };
        entry.progress = Some(Progress {
            chapter: reader.chapter.key().into(),
            page: reader.current,
            fraction: reader.fraction.clamp(0.0, 1.0),
            page_count: reader.urls.len(),
        });
        entry.last_read = domain::now();
        entry.mode = Some(reader.mode);
        entry.zoom = reader.zoom;
        if reader.completed {
            entry.mark_read(reader.chapter.key());
        }
        match library.save(entry) {
            Ok(()) => {
                reader.dirty = false;
                reader.last_saved = Instant::now();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context, settings: &crate::settings::Settings) {
        if let Some(result) = self.site_job.as_ref().and_then(Job::poll) {
            self.site_job = None;
            if let Ok(sites) = result {
                self.sites = sites;
            }
        }
        if let Some(result) = self.search.as_ref().and_then(Job::poll) {
            self.search = None;
            match result {
                Ok(groups) => self.groups = groups,
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(result) = self.detail_job.as_ref().and_then(Job::poll) {
            self.detail_job = None;
            match result {
                Ok(detail) => {
                    self.selected = Some(detail.book.clone());
                    if let Some(lib) = &mut self.library
                        && let Err(error) = lib.refresh(detail)
                    {
                        self.status = error.to_string();
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(reader) = &mut self.reader {
            if let Some(result) = reader.pages_job.as_ref().and_then(Job::poll) {
                reader.pages_job = None;
                match result {
                    Ok(urls) => {
                        reader.current = reader.current.min(urls.len().saturating_sub(1));
                        reader.urls = urls;
                        reader.restore = true;
                    }
                    Err(error) => reader.pages_error = Some(error.to_string()),
                }
            }
            let done: Vec<_> = reader
                .jobs
                .iter()
                .filter_map(|(index, job)| job.poll().map(|result| (*index, result)))
                .collect();
            for (index, result) in done {
                reader.jobs.remove(&index);
                match result {
                    Ok(result) => {
                        let size = (
                            result.original.bitmap.width as f32,
                            result.original.bitmap.height as f32,
                        );
                        if index <= reader.current && reader.sizes.get(&index) != Some(&size) {
                            reader.restore = true;
                        }
                        reader.sizes.insert(index, size);
                        let asset = Asset {
                            path: result.original.path,
                            hash: result.original.hash,
                            original: Picture::new(
                                ctx,
                                &format!("comic-{index}"),
                                result.original.bitmap,
                            ),
                            translated: result.translated.map(|image| {
                                Picture::new(
                                    ctx,
                                    &format!("comic-translated-{index}"),
                                    image.bitmap,
                                )
                            }),
                        };
                        reader.assets.insert(index, asset);
                        if index == reader.current {
                            reader.dirty = true;
                        }
                    }
                    Err(error) => {
                        reader.errors.insert(index, error.to_string());
                    }
                }
            }
            let current = reader.current;
            reader
                .assets
                .retain(|index, _| index.abs_diff(current) <= 4);
            while reader
                .assets
                .values()
                .map(|a| a.original.bytes() + a.translated.as_ref().map_or(0, Picture::bytes))
                .sum::<usize>()
                > 128 * 1024 * 1024
            {
                let farthest = reader
                    .assets
                    .keys()
                    .filter(|index| **index != current)
                    .max_by_key(|index| index.abs_diff(current))
                    .copied();
                if let Some(index) = farthest {
                    reader.assets.remove(&index);
                } else {
                    break;
                }
            }
            if reader.dirty && reader.last_saved.elapsed() > Duration::from_secs(1) {
                self.flush();
            }
        }
        let covers: Vec<_> = self
            .cover_jobs
            .iter()
            .filter_map(|(key, job)| job.poll().map(|r| (key.clone(), r)))
            .collect();
        for (key, result) in covers {
            self.cover_jobs.remove(&key);
            match result {
                Ok(image) => {
                    if self.covers.len() >= 80
                        && let Some(old) = self.covers.keys().next().cloned()
                    {
                        self.covers.remove(&old);
                    }
                    self.covers
                        .insert(key.clone(), Picture::new(ctx, &key, image.bitmap));
                }
                Err(_) => {
                    self.cover_errors.insert(key);
                }
            }
        }
        if !self.automatic_checked {
            self.automatic_checked = true;
            if settings.comics.check_updates {
                self.begin_updates(false);
            }
        }
        if self.update_job.is_none()
            && Instant::now() >= self.update_after
            && let Some(book) = self.update_queue.pop_front()
            && let Some(source) = &self.source
        {
            let key = book.key();
            let source = source.clone();
            self.update_job = Some((
                key,
                Job::spawn(move |_| async move { source.detail(&book).await }),
            ));
        }
        if let Some((key, result)) = self
            .update_job
            .as_ref()
            .and_then(|(key, job)| job.poll().map(|r| (key.clone(), r)))
        {
            self.update_job = None;
            self.update_done += 1;
            self.update_after = Instant::now() + Duration::from_secs(2);
            if let Some(lib) = &mut self.library {
                let result = match result {
                    Ok(detail) => lib.refresh(detail),
                    Err(error) => {
                        if let Some(mut entry) = lib.get(&key).cloned() {
                            entry.check_error = Some(error.to_string());
                            entry.checked = domain::now();
                            lib.save(entry)
                        } else {
                            Ok(())
                        }
                    }
                };
                if let Err(error) = result {
                    self.status = error.to_string();
                }
            }
        }
        self.poll_translations(ctx, settings);
        if self.search.is_some()
            || self.detail_job.is_some()
            || self.update_job.is_some()
            || self.translation.is_some()
            || !self.cover_jobs.is_empty()
            || self
                .reader
                .as_ref()
                .is_some_and(|r| r.pages_job.is_some() || !r.jobs.is_empty())
        {
            ctx.request_repaint_after(Duration::from_millis(60));
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if settings.comics.check_updates
            && self.update_job.is_none()
            && self.update_queue.is_empty()
        {
            // Last attempts are persisted, so reopening does not repeatedly hit failures.
            if self
                .entries()
                .iter()
                .any(|e| e.favorite && domain::now().saturating_sub(e.checked) >= 6 * 3600)
            {
                self.begin_updates(false);
            }
        }
    }
    fn begin_updates(&mut self, force: bool) {
        if self.update_job.is_some() || !self.update_queue.is_empty() {
            return;
        }
        self.update_queue = self
            .entries()
            .iter()
            .filter(|e| {
                e.favorite && (force || domain::now().saturating_sub(e.checked) >= 6 * 3600)
            })
            .map(|e| e.book.clone())
            .collect();
        self.update_total = self.update_queue.len();
        self.update_done = 0;
    }
    fn open_book(&mut self, book: Book) {
        self.flush();
        self.reader = None;
        self.view = View::Detail;
        self.selected = Some(book.clone());
        self.status.clear();
        if let Some(source) = self.source.clone() {
            self.detail_job = Some(Job::spawn(
                move |_| async move { source.detail(&book).await },
            ));
        }
    }
    fn read_chapter(&mut self, index: usize, settings: &crate::settings::Settings, resume: bool) {
        self.flush();
        self.translations.clear();
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let Some(chapter) = entry.chapters.get(index).cloned() else {
            return;
        };
        let Some(source) = self.source.clone() else {
            return;
        };
        let book = entry.book.clone();
        let request_book = book.clone();
        let request_chapter = chapter.clone();
        let progress = entry
            .progress
            .filter(|p| resume && p.chapter == chapter.key())
            .unwrap_or_default();
        self.reader = Some(Reader {
            book,
            chapter,
            index,
            urls: Vec::new(),
            current: progress.page,
            fraction: progress.fraction,
            mode: entry.mode.unwrap_or(settings.comics.mode),
            zoom: if entry.zoom > 0.0 {
                entry.zoom.clamp(0.5, 3.0)
            } else {
                1.0
            },
            display: Display::Original,
            sidebar: true,
            sidebar_follow: true,
            restore: true,
            dirty: false,
            completed: false,
            assets: HashMap::new(),
            jobs: HashMap::new(),
            errors: HashMap::new(),
            sizes: HashMap::new(),
            pages_job: Some(Job::spawn(move |_| async move {
                source.pages(&request_book, &request_chapter, false).await
            })),
            pages_error: None,
            last_saved: Instant::now(),
            toolbar_until: Instant::now() + Duration::from_secs(3),
        });
        self.view = View::Reader;
        self.status.clear();
    }
    fn resume(&mut self, entry: Entry, settings: &crate::settings::Settings) {
        let index = entry
            .progress
            .as_ref()
            .and_then(|p| entry.chapters.iter().position(|c| c.key() == p.chapter))
            .unwrap_or(0);
        self.selected = Some(entry.book);
        self.read_chapter(index, settings, true);
    }
    fn ensure_cover(&mut self, book: &Book) {
        let key = book.key();
        if book.cover.is_empty()
            || self.covers.contains_key(&key)
            || self.cover_jobs.contains_key(&key)
            || self.cover_errors.contains(&key)
            || self.cover_jobs.len() >= 2
        {
            return;
        }
        let Some(source) = &self.source else { return };
        let Ok(url) = source.image_request(&book.site, &book.cover) else {
            return;
        };
        let directory = domain::root().join("cache/covers");
        let cache_key = format!("cover:{}", book.cover);
        self.cover_jobs.insert(
            key,
            Job::spawn(move |_| async move {
                domain::cache::fetch(&directory, &cache_key, url, 240).await
            }),
        );
    }
    fn ensure_page(&mut self, index: usize, settings: &crate::settings::Settings) {
        let Some(reader) = &mut self.reader else {
            return;
        };
        if reader.jobs.len() >= PAGE_CONCURRENCY
            || reader.assets.contains_key(&index)
            || reader.jobs.contains_key(&index)
            || reader.errors.contains_key(&index)
        {
            return;
        }
        let Some(url) = reader.urls.get(index) else {
            return;
        };
        let Some(source) = &self.source else { return };
        let url = match source.image_request(&reader.book.site, url) {
            Ok(url) => url,
            Err(error) => {
                reader.errors.insert(index, error.to_string());
                return;
            }
        };
        let key = format!("{}:{}:{index}", reader.book.key(), reader.chapter.key());
        let context = reader.book.title.clone();
        let configuration = domain::translation::Config::load_with(
            &settings.comics,
            &settings.llm,
            settings.language,
        )
        .ok();
        let directory = domain::root();
        reader.jobs.insert(
            index,
            Job::spawn(move |_| async move {
                let original =
                    domain::cache::fetch(&directory.join("cache/pages"), &key, url, 1800).await?;
                let translated = configuration.and_then(|config| {
                    domain::cache::load(
                        directory.join("translations").join(format!(
                            "{}.png",
                            config.cache_key(&original.hash, &context)
                        )),
                        1800,
                    )
                    .ok()
                });
                Ok(ImageResult {
                    original,
                    translated,
                })
            }),
        );
    }
    fn queue_translation(&mut self, all: bool, settings: &crate::settings::Settings) {
        if let Err(error) = domain::translation::Config::load_with(
            &settings.comics,
            &settings.llm,
            settings.language,
        ) {
            self.status = error.to_string();
            return;
        }
        let Some(reader) = &self.reader else { return };
        let indices: Vec<_> = if all {
            (reader.current..reader.urls.len())
                .chain(0..reader.current)
                .collect()
        } else {
            vec![reader.current]
        };
        if self.translation.is_none() && self.translations.is_empty() {
            self.translation_done = 0;
            self.translation_failed = 0;
            self.translation_total = 0;
        }
        for index in indices {
            let request = TranslationRequest {
                book: reader.book.key(),
                chapter: reader.chapter.key().into(),
                index,
                path: PathBuf::new(),
                hash: String::new(),
                context: reader.book.title.clone(),
                settings: settings.comics.clone(),
                language: settings.language,
                llm: settings.llm.clone(),
            };
            if self
                .translations
                .iter()
                .any(|r| r.book == request.book && r.chapter == request.chapter && r.index == index)
                || self.translation.as_ref().is_some_and(|r| {
                    r.request.book == request.book
                        && r.request.chapter == request.chapter
                        && r.request.index == index
                })
            {
                continue;
            }
            self.translations.push_back(request);
            self.translation_total += 1;
        }
    }
    fn poll_translations(&mut self, ctx: &egui::Context, settings: &crate::settings::Settings) {
        if let Some(result) = self.translation.as_ref().and_then(|t| t.job.poll()) {
            let active = self.translation.take().unwrap();
            self.translation_done += 1;
            match result {
                Ok(path) => {
                    if let Some(reader) = &mut self.reader
                        && reader.book.key() == active.request.book
                        && reader.chapter.key() == active.request.chapter
                    {
                        // Loading the cached translation again happens off the GUI thread.
                        if let Some(asset) = reader.assets.get(&active.request.index)
                            && domain::translation::Config::load_with(
                                &settings.comics,
                                &settings.llm,
                                settings.language,
                            )
                            .ok()
                            .is_some_and(|config| {
                                path.file_stem().and_then(|s| s.to_str())
                                    == Some(
                                        config.cache_key(&asset.hash, &reader.book.title).as_str(),
                                    )
                            })
                        {
                            let original = asset.path.clone();
                            let translated = path;
                            reader.jobs.insert(
                                active.request.index,
                                Job::spawn(move |_| async move {
                                    Ok(ImageResult {
                                        original: domain::cache::load(original, 1800)?,
                                        translated: Some(domain::cache::load(translated, 1800)?),
                                    })
                                }),
                            );
                        }
                    }
                }
                Err(error) => {
                    self.translation_failed += 1;
                    self.status = error.to_string();
                }
            }
        }
        if self.translation.is_some() {
            return;
        }
        let Some(mut request) = self.translations.front().cloned() else {
            return;
        };
        let Some(reader) = &self.reader else {
            self.translations.clear();
            return;
        };
        if reader.book.key() != request.book || reader.chapter.key() != request.chapter {
            self.translations.pop_front();
            return;
        }
        if let Some(asset) = reader.assets.get(&request.index) {
            request.path = asset.path.clone();
            request.hash = asset.hash.clone();
        } else if reader.errors.contains_key(&request.index) {
            self.translations.pop_front();
            self.translation_done += 1;
            self.translation_failed += 1;
            return;
        } else {
            // Distant chapter translations load independently of the bounded
            // reading prefetch window; navigation keeps priority nearby.
            if reader.current.abs_diff(request.index) > 4 {
                let Some(source) = &self.source else { return };
                let Some(url) = reader.urls.get(request.index) else {
                    return;
                };
                let url = match source.image_request(&reader.book.site, url) {
                    Ok(url) => url,
                    Err(error) => {
                        self.status = error.to_string();
                        self.translations.pop_front();
                        self.translation_done += 1;
                        self.translation_failed += 1;
                        return;
                    }
                };
                let directory = domain::root();
                let key = format!("{}:{}:{}", request.book, request.chapter, request.index);
                let config = match domain::translation::Config::load_with(
                    &request.settings,
                    &request.llm,
                    request.language,
                ) {
                    Ok(c) => c,
                    Err(error) => {
                        self.status = error.to_string();
                        self.translations.clear();
                        return;
                    }
                };
                self.translations.pop_front();
                let job_request = request.clone();
                let job = Job::spawn(move |progress| async move {
                    let image =
                        domain::cache::fetch(&directory.join("cache/pages"), &key, url, 1800)
                            .await?;
                    domain::translation::translate(
                        config,
                        &image.path,
                        &image.hash,
                        &job_request.context,
                        &directory.join("translations"),
                        progress,
                    )
                    .await
                });
                self.translation = Some(TranslationJob { request, job });
                return;
            }
            self.ensure_page(request.index, settings);
            return;
        }
        let config = match domain::translation::Config::load_with(
            &request.settings,
            &request.llm,
            request.language,
        ) {
            Ok(c) => c,
            Err(error) => {
                self.status = error.to_string();
                self.translations.clear();
                return;
            }
        };
        self.translations.pop_front();
        let copy = request.clone();
        let directory = domain::root().join("translations");
        self.translation = Some(TranslationJob {
            request,
            job: Job::spawn(move |progress| async move {
                domain::translation::translate(
                    config,
                    &copy.path,
                    &copy.hash,
                    &copy.context,
                    &directory,
                    progress,
                )
                .await
            }),
        });
        ctx.request_repaint();
    }
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        settings: &mut crate::settings::Settings,
        settings_open: &mut bool,
    ) {
        if let Some(error) = &self.init_error {
            ui.colored_label(Color32::LIGHT_RED, error);
            return;
        }
        match self.view {
            View::Catalog => self.catalog(ui, settings),
            View::Detail => self.detail_ui(ui, settings),
            View::Reader => {
                self.reader_ui(ui, settings, settings_open);
                return;
            }
        }
        if !self.status.is_empty() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(Color32::LIGHT_RED, &self.status);
                if ui
                    .small_button(settings.language.text("关闭提示", "Dismiss"))
                    .clicked()
                {
                    self.status.clear();
                }
            });
        }
        if self.translation_total > 0 {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} {}/{} · {} {}",
                    settings.language.text("图片翻译", "Image translation"),
                    self.translation_done,
                    self.translation_total,
                    self.translation_failed,
                    settings.language.text("页失败", "failed pages")
                ));
                if let Some(active) = &self.translation {
                    ui.spinner();
                    ui.label(format!(
                        "{} {}",
                        active.job.bytes.load(std::sync::atomic::Ordering::Relaxed),
                        settings.language.text("块完成", "segments done")
                    ));
                }
                if (self.translation.is_some() || !self.translations.is_empty())
                    && ui
                        .button(settings.language.text("取消翻译", "Cancel translation"))
                        .clicked()
                {
                    self.translation = None;
                    self.translations.clear();
                    self.translation_total = self.translation_done;
                }
            });
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_loading_prioritizes_current_then_visible_then_read_ahead() {
        assert_eq!(page_requests(5, 20, vec![4, 5, 6]), vec![5, 4, 6, 7, 8]);
        assert_eq!(page_requests(0, 20, vec![0]), vec![0, 1, 2, 3]);
        assert_eq!(page_requests(19, 20, vec![19]), vec![19, 18]);
        assert!(page_requests(0, 0, vec![]).is_empty());
        let new = page_requests(12, 20, vec![12]);
        let old = page_requests(0, 20, vec![0]);
        assert!(
            old.iter().all(|index| !new.contains(index)),
            "jumping must release every stale prefetch slot"
        );
    }
}
