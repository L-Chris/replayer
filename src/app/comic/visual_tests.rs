//! Offline render of the actual comic UI, without a window or network services.
use super::*;
use eframe::{egui_wgpu, wgpu};

fn fixture(ctx: &egui::Context) -> State {
    let mut library = Library::open(std::path::Path::new(":memory:")).unwrap();
    let mut covers = HashMap::new();
    let titles = [
        "旅人手记",
        "雨后的车站",
        "远山来信",
        "风之旅",
        "长夜行",
        "海边的房间",
    ];
    let books: Vec<_> = (0..18)
        .map(|i| {
            let book = Book {
                provider: "fixture".into(),
                site: "fixture".into(),
                site_name: "漫画之家".into(),
                title: format!("{} {}", titles[i % titles.len()], i + 1),
                author: "山川遥".into(),
                url: format!("https://example.invalid/book/{i}"),
                status: "连载中".into(),
                description: "一封来自远方的信，让少年踏上寻找故乡的旅途。".into(),
                ..Default::default()
            };
            let chapters = (0..128)
                .map(|n| Chapter {
                    id: format!("chapter-{n}"),
                    name: format!("第 {} 话 · 远方的故事", n + 1),
                    url: format!("https://example.invalid/{i}/{n}"),
                    order: n,
                    ..Default::default()
                })
                .collect::<Vec<_>>();
            library
                .save(Entry {
                    book: book.clone(),
                    favorite: i % 3 == 0,
                    chapters,
                    progress: Some(Progress {
                        chapter: "chapter-11".into(),
                        page: 7,
                        page_count: 31,
                        fraction: 0.0,
                    }),
                    last_read: 100 + i as u64,
                    new_unread: if i % 3 == 0 {
                        HashSet::from(["chapter-127".into()])
                    } else {
                        HashSet::new()
                    },
                    ..Default::default()
                })
                .unwrap();
            let tile = image::RgbaImage::from_fn(100, 172, |x, y| {
                let band = ((x / 20 + y / 40 + i as u32) % 3) as u8;
                image::Rgba([
                    42 + band * 16,
                    65 + ((i * 11) % 55) as u8,
                    75 + band * 20,
                    255,
                ])
            });
            covers.insert(
                book.key(),
                Picture::new(
                    ctx,
                    &format!("fixture-{i}"),
                    Bitmap {
                        width: 100,
                        height: 172,
                        tiles: vec![tile],
                    },
                ),
            );
            book
        })
        .collect();
    State {
        library: Some(library),
        source: None,
        sites: vec![domain::Site {
            id: "fixture".into(),
            name: "漫画之家".into(),
        }],
        site_job: None,
        init_error: None,
        tab: Tab::Shelf,
        view: View::Catalog,
        keyword: "旅人".into(),
        site_filter: String::new(),
        groups: vec![SearchGroup {
            site: domain::Site {
                id: "fixture".into(),
                name: "漫画之家".into(),
            },
            books,
            error: None,
        }],
        search: Vec::new(),
        selected: None,
        detail_job: None,
        reader: None,
        status: String::new(),
        covers,
        cover_jobs: HashMap::new(),
        cover_errors: HashSet::new(),
        reverse: false,
        unread_only: false,
        chapter_filter: Some(domain::ChapterKind::Chapter),
        update_queue: VecDeque::new(),
        update_job: None,
        update_done: 0,
        update_total: 0,
        update_after: Instant::now(),
        automatic_checked: true,
        translations: VecDeque::new(),
        translation: None,
        translation_done: 0,
        translation_total: 0,
        translation_failed: 0,
    }
}

#[test]
fn comic_search_shows_completed_source_without_waiting_for_another() {
    let ctx = egui::Context::default();
    let mut state = fixture(&ctx);
    state.groups.clear();
    let site = domain::Site {
        id: "fast".into(),
        name: "Fast".into(),
    };
    let result_site = site.clone();
    state.search = vec![
        (
            site.clone(),
            Job::spawn(move |_| async move {
                Ok(vec![SearchGroup {
                    site: result_site,
                    books: vec![Book {
                        title: "ready".into(),
                        ..Default::default()
                    }],
                    error: None,
                }])
            }),
        ),
        (
            site,
            Job::spawn(move |_| async move { std::future::pending().await }),
        ),
    ];
    let mut settings = crate::settings::Settings::default();
    settings.comics.check_updates = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while state.groups.is_empty() && Instant::now() < deadline {
        state.poll(&ctx, &settings);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.groups[0].books[0].title, "ready");
    assert_eq!(state.search.len(), 1);
    state.search_for(String::new());
    assert!(state.search.is_empty());
    assert!(state.groups.is_empty());
}

#[test]
fn comic_chapter_navigation_keeps_editions_separate() {
    use domain::ChapterKind::{Chapter as Single, Volume};
    let chapters: Vec<_> = [Single, Volume, Single, Volume, Single]
        .into_iter()
        .map(|kind| Chapter {
            kind,
            ..Default::default()
        })
        .collect();
    assert_eq!(chapter_neighbors(&chapters, 0), [None, Some(2)]);
    assert_eq!(chapter_neighbors(&chapters, 2), [Some(0), Some(4)]);
    assert_eq!(chapter_neighbors(&chapters, 3), [Some(1), None]);
    assert_eq!(chapter_neighbors(&chapters, 4), [Some(2), None]);
    assert_eq!(chapter_neighbors(&chapters, 99), [None, None]);
    let policy = ImagePolicy {
        concurrency: 2,
        ahead: 1,
        behind: 0,
        ..Default::default()
    };
    assert_eq!(page_requests(5, 10, vec![5, 6], policy), vec![5, 6]);
    assert_eq!(
        page_requests(5, 10, vec![4, 5, 6, 7], policy),
        vec![5, 4, 6, 7]
    );
    let ctx = egui::Context::default();
    let mut state = fixture(&ctx);
    state.chapter_filter = Some(Volume);
    state.resume(
        state.entries()[0].clone(),
        &crate::settings::Settings::default(),
    );
    assert_eq!(state.chapter_filter, Some(Single));
}

fn reader_fixture(ctx: &egui::Context) -> State {
    let mut state = fixture(ctx);
    let entry = state.entries()[0].clone();
    let page = Picture::new(
        ctx,
        "reader-fixture",
        Bitmap {
            width: 850,
            height: 1200,
            tiles: vec![image::RgbaImage::from_fn(850, 1200, |x, y| {
                let value = if x % 280 < 6 || y % 360 < 6 { 50 } else { 220 };
                image::Rgba([value, value, value, 255])
            })],
        },
    );
    state.reader = Some(Reader {
        book: entry.book,
        chapter: entry.chapters[11].clone(),
        index: 11,
        urls: vec!["https://example.invalid/page".into(); 31],
        current: 7,
        fraction: 0.0,
        mode: ReadingMode::RightToLeft,
        zoom: 1.0,
        display: Display::Original,
        sidebar: false,
        sidebar_follow: true,
        restore: true,
        dirty: false,
        completed: false,
        assets: HashMap::from([(
            7,
            Asset {
                path: PathBuf::new(),
                hash: String::new(),
                original: page,
                translated: None,
                last_used: Instant::now(),
            },
        )]),
        visible: vec![7],
        jobs: HashMap::new(),
        errors: HashMap::new(),
        sizes: HashMap::from([(7, (850.0, 1200.0))]),
        pages_job: None,
        pages_error: None,
        last_saved: Instant::now(),
        toolbar_until: Instant::now() + Duration::from_secs(3600),
    });
    state
}

#[test]
fn reader_scroll_grace_retains_work_but_distant_seeks_get_a_slot() {
    let ctx = egui::Context::default();
    let mut state = reader_fixture(&ctx);
    let reader = state.reader.as_mut().unwrap();
    let now = Instant::now();
    for index in [8, 9] {
        reader.jobs.insert(
            index,
            PageJob {
                job: Job::spawn(|_| async { std::future::pending().await }),
                last_requested: now,
            },
        );
    }
    reader.maintain_requests(&[7], 2, now + Duration::from_millis(400));
    assert_eq!(
        reader.jobs.len(),
        2,
        "briefly leaving the viewport cancelled a request"
    );
    reader.maintain_requests(&[7, 8], 2, now + Duration::from_millis(900));
    assert!(reader.jobs.contains_key(&8));
    assert!(!reader.jobs.contains_key(&9));
    reader.jobs.insert(
        9,
        PageJob {
            job: Job::spawn(|_| async { std::future::pending().await }),
            last_requested: now + Duration::from_millis(900),
        },
    );
    reader.current = 20;
    reader.maintain_requests(&[20, 21], 2, now + Duration::from_millis(900));
    assert_eq!(
        reader.jobs.len(),
        1,
        "the new current page must not wait for obsolete requests"
    );
    reader.current = 20;
    // Pages beyond the old +/-4 window remain resident while under budget.
    reader.trim_assets();
    assert!(reader.assets.contains_key(&7));
}

fn reader_frame(
    ctx: &egui::Context,
    state: &mut State,
    time: &mut f64,
    pointer: egui::Pos2,
    wheel: f32,
) -> egui::FullOutput {
    *time += 1.0 / 60.0;
    let mut events = vec![egui::Event::PointerMoved(pointer)];
    if wheel != 0.0 {
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: vec2(0.0, wheel),
            modifiers: egui::Modifiers::NONE,
        });
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |ui| state.reader_ui(ui, &crate::settings::Settings::default(), &mut false),
    );
    // These input/layout tests inspect shapes and do not upload GPU textures.
    output.textures_delta.clear();
    output
}

#[test]
fn reader_wheel_scrolls_over_pages_and_empty_margins_without_scrolling_the_chapter_panel() {
    let ctx = egui::Context::default();
    design::apply(&ctx);
    let mut state = reader_fixture(&ctx);
    let reader = state.reader.as_mut().unwrap();
    reader.mode = ReadingMode::Vertical;
    reader.sidebar = true;
    reader.zoom = 0.6;
    reader.current = 0;
    let mut time = 0.0;
    let progress = |state: &State| {
        let reader = state.reader.as_ref().unwrap();
        reader.current as f32 + reader.fraction
    };
    let mut scroll = |state: &mut State, pointer: egui::Pos2, delta: f32| {
        for _ in 0..30 {
            reader_frame(&ctx, state, &mut time, pointer, 0.0);
        }
        reader_frame(&ctx, state, &mut time, pointer, delta);
        for _ in 0..30 {
            reader_frame(&ctx, state, &mut time, pointer, 0.0);
        }
    };
    // This point is to the right of the reduced page, inside the reading canvas.
    scroll(&mut state, pos2(900.0, 300.0), -320.0);
    let margin = progress(&state);
    assert!(
        margin > 0.1,
        "wheel over the page margin did not scroll: {margin}"
    );
    scroll(&mut state, pos2(500.0, 300.0), -320.0);
    let page = progress(&state);
    assert!(page > margin + 0.1, "wheel over the page did not scroll");
    scroll(&mut state, pos2(500.0, 300.0), 240.0);
    let back = progress(&state);
    assert!(back < page - 0.1, "upward wheel did not scroll back");
    scroll(&mut state, pos2(1150.0, 300.0), -240.0);
    assert!(
        (progress(&state) - back).abs() < 0.01,
        "chapter panel scrolled the reader"
    );
}

#[test]
fn reader_pages_and_comparison_pairs_are_centered_in_the_scroll_viewport() {
    for mode in [ReadingMode::Vertical, ReadingMode::LeftToRight] {
        for display in [Display::Original, Display::Translated, Display::Compare] {
            for zoom in [0.6, 1.0, 1.5] {
                let ctx = egui::Context::default();
                design::apply(&ctx);
                let mut state = reader_fixture(&ctx);
                let reader = state.reader.as_mut().unwrap();
                reader.mode = mode;
                reader.display = display;
                reader.zoom = zoom;
                let asset = reader.assets.get_mut(&7).unwrap();
                asset.translated = Some(Picture {
                    height: asset.original.height,
                    tiles: asset.original.tiles.clone(),
                });
                let texture = asset.original.tiles[0].id();
                let mut time = 0.0;
                for _ in 0..30 {
                    reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), 0.0);
                }
                let output = reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), 0.0);
                let mut bounds = Rect::NOTHING;
                let mut viewport = Rect::NOTHING;
                for shape in output.shapes {
                    if let egui::Shape::Mesh(mesh) = shape.shape
                        && mesh.texture_id == texture
                    {
                        for vertex in &mesh.vertices {
                            bounds.extend_with(vertex.pos);
                        }
                        viewport = shape.clip_rect;
                    }
                }
                assert!(bounds.is_positive(), "reader fixture page was not visible");
                assert!(
                    (bounds.center().x - viewport.center().x).abs() <= 2.0,
                    "page is off center: zoom={zoom}, vertical={}, compare={}, page={bounds:?}, viewport={viewport:?}",
                    mode == ReadingMode::Vertical,
                    display == Display::Compare
                );
                if mode == ReadingMode::LeftToRight && display == Display::Original && zoom > 1.0 {
                    reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), -240.0);
                    for _ in 0..30 {
                        reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), 0.0);
                    }
                    let output = reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), 0.0);
                    let top = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                                Some(mesh.calc_bounds().min.y)
                            }
                            _ => None,
                        })
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        top < bounds.min.y - 20.0,
                        "zoomed paged image did not scroll vertically"
                    );
                    assert_eq!(state.reader.as_ref().unwrap().current, 7);
                }
            }
        }
    }
}

struct Capture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
}
impl Capture {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let adapter = runtime
            .block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
        let (device, queue) = runtime
            .block_on(adapter.request_device(&Default::default()))
            .unwrap();
        let renderer =
            egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
        Self {
            device,
            queue,
            renderer,
        }
    }
    fn save(
        &mut self,
        ctx: &egui::Context,
        size: [u32; 2],
        path: &std::path::Path,
        ui: impl FnMut(&mut Ui),
    ) {
        self.save_with_pointer(ctx, size, path, None, ui);
    }

    fn save_with_pointer(
        &mut self,
        ctx: &egui::Context,
        size: [u32; 2],
        path: &std::path::Path,
        pointer: Option<egui::Pos2>,
        mut ui: impl FnMut(&mut Ui),
    ) {
        for frame in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(size[0] as f32, size[1] as f32),
                    )),
                    time: Some(frame as f64),
                    events: pointer.map_or_else(
                        || vec![egui::Event::PointerGone],
                        |pos| vec![egui::Event::PointerMoved(pos)],
                    ),
                    ..Default::default()
                },
                |root| {
                    root.painter().rect_filled(root.max_rect(), 0, BACKGROUND);
                    ui(root);
                },
            );
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    self.renderer
                        .update_texture(&self.device, &self.queue, *id, delta);
                }
            }
            let jobs = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: size,
                pixels_per_point: output.pixels_per_point,
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("UI visual review"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let mut commands = self.renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                &jobs,
                &screen,
            );
            {
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &texture.create_view(&Default::default()),
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    })
                    .forget_lifetime();
                self.renderer.render(&mut pass, &jobs, &screen);
            }
            let stride = (size[0] * 4).div_ceil(256) * 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (stride * size[1]) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
            commands.push(encoder.finish());
            self.queue.submit(commands);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                tx.send(result).unwrap()
            });
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            rx.recv().unwrap().unwrap();
            if frame == 2 {
                let bytes = buffer.get_mapped_range(..).unwrap();
                let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
                for row in bytes.chunks(stride as usize) {
                    pixels.extend_from_slice(&row[..size[0] as usize * 4]);
                }
                image::RgbaImage::from_raw(size[0], size[1], pixels)
                    .unwrap()
                    .save(path)
                    .unwrap();
            }
            for id in &output.textures_delta.free {
                self.renderer.free_texture(id);
            }
            output.textures_delta.clear();
        }
    }
}

#[test]
#[ignore = "Requires a GPU; renders offline UI fixtures to target/ui-review"]
fn comic_catalog_visuals() {
    let directory = std::path::Path::new("target/ui-review");
    std::fs::create_dir_all(directory).unwrap();
    let mut capture = Capture::new();
    for width in [1030, 628, 354] {
        let ctx = egui::Context::default();
        design::apply(&ctx);
        setup_fonts(&ctx);
        let mut state = fixture(&ctx);
        let settings = crate::settings::Settings::default();
        for (name, tab) in [
            ("shelf", Tab::Shelf),
            ("search", Tab::Search),
            ("updates", Tab::Updates),
            ("history", Tab::History),
        ] {
            state.tab = tab;
            capture.save(
                &ctx,
                [width, 580],
                &directory.join(format!("comic-{name}-{width}.png")),
                |ui| {
                    state.catalog(ui, &settings);
                },
            );
        }
        state.tab = Tab::Search;
        capture.save_with_pointer(
            &ctx,
            [width, 580],
            &directory.join(format!("comic-hover-{width}.png")),
            Some(pos2(70.0, 180.0)),
            |ui| state.catalog(ui, &settings),
        );
        state.selected = Some(state.entries()[0].book.clone());
        capture.save(
            &ctx,
            [width, 650],
            &directory.join(format!("comic-detail-{width}.png")),
            |ui| state.detail_ui(ui, &settings),
        );
        if width == 354 {
            capture.save(
                &ctx,
                [width, 342],
                &directory.join("comic-detail-short.png"),
                |ui| state.detail_ui(ui, &settings),
            );
        }
        let mut entry = state.entries()[0].clone();
        let old_key = entry.book.key();
        entry.book.provider = "komiic".into();
        entry.book.site = "komiic".into();
        entry.book.site_name = "Komiic".into();
        entry.book.title = "葬送的芙莉莲".into();
        entry.book.url = "https://komiic.com/comic/217".into();
        for (index, chapter) in entry.chapters.iter_mut().enumerate() {
            chapter.kind = if index < 100 {
                domain::ChapterKind::Chapter
            } else {
                domain::ChapterKind::Volume
            };
            chapter.name = format!(
                "第 {} {}",
                index % 100 + 1,
                if chapter.kind == domain::ChapterKind::Volume {
                    "卷"
                } else {
                    "话"
                }
            );
        }
        if let Some(cover) = state.covers.remove(&old_key) {
            state.covers.insert(entry.book.key(), cover);
        }
        state.selected = Some(entry.book.clone());
        state.library.as_mut().unwrap().save(entry).unwrap();
        for (name, kind) in [
            ("chapters", Some(domain::ChapterKind::Chapter)),
            ("volumes", Some(domain::ChapterKind::Volume)),
            ("all", None),
        ] {
            state.chapter_filter = kind;
            capture.save(
                &ctx,
                [width, 650],
                &directory.join(format!("komiic-detail-{name}-{width}.png")),
                |ui| state.detail_ui(ui, &settings),
            );
        }
    }
    let ctx = egui::Context::default();
    design::apply(&ctx);
    setup_fonts(&ctx);
    let mut state = reader_fixture(&ctx);
    let settings = crate::settings::Settings::default();
    for size in [[1280, 800], [960, 640], [760, 540]] {
        capture.save(
            &ctx,
            size,
            &directory.join(format!("comic-reader-{}.png", size[0])),
            |ui| state.reader_ui(ui, &settings, &mut false),
        );
    }
    // A queued fixture request displays progress without spawning a model call.
    state.translation_total = 31;
    state.translation_done = 12;
    state.translations.push_back(TranslationRequest {
        book: String::new(),
        chapter: String::new(),
        index: 0,
        path: PathBuf::new(),
        hash: String::new(),
        context: String::new(),
        settings: settings.comics.clone(),
        language: settings.language,
        llm: settings.llm.clone(),
    });
    ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
    capture.save_with_pointer(
        &ctx,
        [1280, 800],
        &directory.join("comic-reader-progress-hover.png"),
        Some(pos2(1247.0, 768.0)),
        |ui| state.reader_ui(ui, &settings, &mut false),
    );
    let english = crate::settings::Settings {
        language: Language::English,
        ..settings.clone()
    };
    for size in [[1280, 800], [760, 540]] {
        capture.save(
            &ctx,
            size,
            &directory.join(format!("comic-reader-english-{}.png", size[0])),
            |ui| state.reader_ui(ui, &english, &mut false),
        );
    }
    let mut value = String::new();
    for (width, zoom, name) in [
        (1280, 0.6, "reduced"),
        (1280, 1.5, "zoomed"),
        (760, 0.6, "narrow"),
    ] {
        let reader = state.reader.as_mut().unwrap();
        reader.mode = ReadingMode::Vertical;
        reader.zoom = zoom;
        reader.current = 7;
        reader.fraction = 0.0;
        reader.restore = true;
        capture.save(
            &ctx,
            [width, 800],
            &directory.join(format!("comic-reader-vertical-{name}.png")),
            |ui| state.reader_ui(ui, &settings, &mut false),
        );
    }
    let mut password = "example-key".to_owned();
    capture.save(&ctx, [628, 320], &directory.join("controls.png"), |ui| {
        ui.horizontal(|ui| {
            design::search_field(ui, &mut value, 420.0, "搜索媒体、音乐或漫画");
            design::control(ui, "搜索", None, design::Control::Primary, 76.0);
        });
        ui.label("服务地址");
        ui.add(design::text_edit(&mut value).hint_text("https://api.example.com/v1"));
        ui.label("API Key");
        ui.add(design::text_edit(&mut password).password(true));
        ui.horizontal(|ui| {
            ui.label("翻译语言");
            egui::ComboBox::from_id_salt("fixture-language")
                .selected_text("跟随界面语言")
                .show_ui(ui, |ui| {
                    ui.label("简体中文");
                });
        });
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
        design::favorite_at(
            ui,
            ui.id().with("balanced-heart"),
            Rect::from_center_size(pos2(row.max.x - 20.0, row.center().y), vec2(32.0, 32.0)),
            false,
        );
    });
    let mut selected = 1;
    capture.save_with_pointer(
        &ctx,
        [628, 200],
        &directory.join("compact-controls-hover.png"),
        Some(pos2(50.0, 20.0)),
        |ui| {
            ui.horizontal(|ui| {
                design::selectable_value(ui, &mut selected, 0, "本地音乐");
                design::selectable_value(ui, &mut selected, 1, "在线搜索");
            });
            ui.horizontal(|ui| {
                design::control(
                    ui,
                    "检查更新",
                    Some(|p, r, c| draw_repeat(p, r, c, false)),
                    design::Control::Outline,
                    0.0,
                );
                egui::ComboBox::from_id_salt("fixture-source")
                    .selected_text("全部来源")
                    .show_ui(ui, |ui| {
                        ui.label("全部来源");
                    });
                egui::ComboBox::from_id_salt("fixture-music-source")
                    .selected_text("网易云")
                    .show_ui(ui, |ui| {
                        ui.label("网易云");
                    });
            });
            design::selectable_value(ui, &mut selected, 2, "漫画相关");
        },
    );
}
