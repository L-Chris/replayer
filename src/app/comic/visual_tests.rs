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
        cover_used: HashMap::new(),
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
        last_update_started: None,
        automatic_checked: true,
        translations: VecDeque::new(),
        translation: None,
        translation_done: 0,
        translation_total: 0,
        translation_failed: 0,
    }
}

#[test]
fn visible_covers_keep_their_textures_when_a_large_search_fills_the_cache() {
    let ctx = egui::Context::default();
    design::apply(&ctx);
    setup_fonts(&ctx);
    let mut state = fixture(&ctx);
    state.covers.clear();
    let template = state.groups[0].books[0].clone();
    let books: Vec<_> = (0..140)
        .map(|i| Book {
            url: format!("https://example.invalid/large-search/{i}"),
            ..template.clone()
        })
        .collect();
    let insert = |state: &mut State, key: String, used: u64| {
        state.covers.insert(
            key.clone(),
            Picture::new(
                &ctx,
                &key,
                Bitmap {
                    width: 3,
                    height: 4,
                    tiles: vec![image::RgbaImage::from_pixel(3, 4, image::Rgba([90; 4]))],
                },
            ),
        );
        state.cover_used.insert(key, used);
    };
    for book in &books {
        insert(&mut state, book.key(), 0);
    }
    let settings = crate::settings::Settings::default();
    let mut visible = HashMap::new();
    for frame in 0..20 {
        // Simulate background uploads after the cache is already full.
        if frame > 4 {
            insert(
                &mut state,
                format!("background-{frame}"),
                ctx.cumulative_frame_nr(),
            );
        }
        let mut drawn = HashMap::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1654.0, 710.0))),
                ..Default::default()
            },
            |ui| {
                state.poll(&ctx, &settings);
                design::scroll().show(ui, |ui| {
                    state.book_grid(ui, &books, &settings);
                });
                drawn = state
                    .cover_used
                    .iter()
                    .filter(|(_, used)| **used == ctx.cumulative_frame_nr())
                    .filter_map(|(key, _)| {
                        state
                            .covers
                            .get(key)
                            .map(|p| (key.clone(), p.tiles[0].id()))
                    })
                    .filter(|(key, _)| !key.starts_with("background-"))
                    .collect();
            },
        );
        output.textures_delta.clear();
        if frame == 1 {
            assert!(
                drawn.len() >= 20 && drawn.len() < COVER_CACHE_COUNT,
                "unexpected visible cover count: {}",
                drawn.len()
            );
            visible = drawn;
        } else if frame > 1 {
            assert_eq!(
                drawn, visible,
                "visible cover textures changed at frame {frame}"
            );
        }
    }
    assert!(state.covers.len() <= COVER_CACHE_COUNT);
    assert!(
        state.cover_jobs.is_empty(),
        "cached covers must not be requested again"
    );
}

#[test]
fn favorite_updates_start_once_even_when_saved_checks_are_recent() {
    let ctx = egui::Context::default();
    let mut state = fixture(&ctx);
    let settings = crate::settings::Settings::default();
    assert!(settings.comics.check_updates);
    state.automatic_checked = false;
    for entry in &mut state.library.as_mut().unwrap().entries {
        entry.checked = domain::now();
    }
    let favorites = state.entries().iter().filter(|e| e.favorite).count();
    state.poll(&ctx, &settings);
    assert_eq!(state.update_queue.len(), favorites);
    assert!(state.update_queue.iter().all(|b| {
        state
            .library
            .as_ref()
            .unwrap()
            .get(&b.key())
            .unwrap()
            .favorite
    }));
    state.update_queue.clear();
    // Even after the interval has elapsed, polling alone must not start work.
    state.last_update_started = Some(Instant::now() - UPDATE_INTERVAL);
    state.poll(&ctx, &settings);
    assert!(state.update_queue.is_empty());
}

#[test]
fn favorite_home_checks_throttle_attempts_and_do_not_duplicate_active_batches() {
    let ctx = egui::Context::default();
    let mut state = fixture(&ctx);
    let mut settings = crate::settings::Settings::default();
    let now = Instant::now();
    state.refresh_on_home(&settings, now);
    let count = state.update_queue.len();
    assert!(count > 0);
    state.refresh_on_home(&settings, now + UPDATE_INTERVAL);
    assert_eq!(state.update_queue.len(), count);
    assert_eq!(state.last_update_started, Some(now));
    state.update_queue.clear();
    // Applies equally after success or failure; no completed result is required.
    state.refresh_on_home(&settings, now + UPDATE_INTERVAL - Duration::from_millis(1));
    assert!(state.update_queue.is_empty());
    state.refresh_on_home(&settings, now + UPDATE_INTERVAL);
    assert_eq!(state.update_queue.len(), count);
    state.update_queue.clear();
    settings.comics.check_updates = false;
    state.refresh_on_home(&settings, now + UPDATE_INTERVAL * 2);
    assert!(state.update_queue.is_empty());
    state.begin_updates(now + UPDATE_INTERVAL);
    assert_eq!(
        state.update_queue.len(),
        count,
        "manual checks remain available"
    );
}

#[test]
fn returning_to_home_triggers_favorite_checks_but_repeated_navigation_is_throttled() {
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    app.workspace.page = crate::app::workspace::Page::Comic;
    app.navigate(crate::app::workspace::Page::Home);
    assert!(!app.comic.update_queue.is_empty());
    app.comic.update_queue.clear();
    app.navigate(crate::app::workspace::Page::Music);
    app.navigate(crate::app::workspace::Page::Home);
    assert!(app.comic.update_queue.is_empty());
    app.comic.last_update_started = Some(Instant::now() - UPDATE_INTERVAL);
    app.navigate(crate::app::workspace::Page::Music);
    app.navigate(crate::app::workspace::Page::Home);
    assert!(!app.comic.update_queue.is_empty());
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
    for pointer in [pos2(500.0, 20.0), pos2(500.0, 782.0)] {
        scroll(&mut state, pointer, -240.0);
        assert!(
            (progress(&state) - back).abs() < 0.01,
            "wheel on an overlay toolbar scrolled the page"
        );
    }
}

#[test]
fn reader_toolbar_visibility_does_not_resize_or_move_the_page() {
    for sidebar in [false, true] {
        let ctx = egui::Context::default();
        design::apply(&ctx);
        let mut state = reader_fixture(&ctx);
        let reader = state.reader.as_mut().unwrap();
        reader.sidebar = sidebar;
        reader.toolbar_until = Instant::now() - Duration::from_secs(1);
        let texture = reader.assets[&7].original.tiles[0].id();
        let geometry = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some((mesh.calc_bounds(), shape.clip_rect))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let mut time = 0.0;
        for _ in 0..30 {
            reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 300.0), 0.0);
        }
        assert!(!state.toolbar_visible());
        let hidden = geometry(reader_frame(
            &ctx,
            &mut state,
            &mut time,
            pos2(500.0, 300.0),
            0.0,
        ));
        assert!(!hidden.is_empty());
        // The canvas extends behind both bars, retaining only an 8pt edge inset.
        assert_eq!(hidden[0].1.min.y, 8.0);
        assert!(hidden[0].1.max.y > 764.0);
        for _ in 0..30 {
            reader_frame(&ctx, &mut state, &mut time, pos2(500.0, 20.0), 0.0);
        }
        assert!(state.toolbar_visible());
        let visible = geometry(reader_frame(
            &ctx,
            &mut state,
            &mut time,
            pos2(500.0, 300.0),
            0.0,
        ));
        assert_eq!(hidden, visible, "showing toolbars changed page geometry");
    }
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

#[test]
#[ignore = "Requires a GPU; renders AI settings fixtures without network requests"]
fn ai_settings_visuals() {
    let directory = std::path::Path::new("target/ui-review");
    std::fs::create_dir_all(directory).unwrap();
    let mut capture = Capture::new();
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    for width in [1030, 628] {
        for (name, tab) in [
            ("general", crate::app::PrefTab::General),
            ("about", crate::app::PrefTab::About),
            ("providers", crate::app::PrefTab::Providers),
            ("subtitles", crate::app::PrefTab::Subtitles),
            ("images", crate::app::PrefTab::Comics),
            ("music", crate::app::PrefTab::MusicSources),
            ("cloud", crate::app::PrefTab::Cloud),
        ] {
            app.preferences_tab = tab;
            capture.save(
                &ctx,
                [width, 700],
                &directory.join(format!("settings-{name}-{width}.png")),
                |ui| app.preferences_page(ui),
            );
            if tab == crate::app::PrefTab::Cloud {
                let original = app.preferences_draft.clone();
                app.preferences_draft.sync.enabled = true;
                app.preferences_draft.sync.sync_keys = true;
                app.preferences_draft.sync.username = "reader@example.invalid".into();
                app.preferences_draft.sync.password = "fixture-cloud-password".into();
                app.preferences_draft.sync.passphrase = "fixture-sync-password".into();
                capture.save(
                    &ctx,
                    [width, 700],
                    &directory.join(format!("settings-cloud-keys-{width}.png")),
                    |ui| app.preferences_page(ui),
                );
                app.preferences_draft = original;
            }
        }
    }
}

#[test]
#[ignore = "Requires a GPU; renders the icon library in the actual workspace without network requests"]
fn icon_library_visuals() {
    let directory = std::path::Path::new("target/ui-review");
    std::fs::create_dir_all(directory).unwrap();
    let mut capture = Capture::new();
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    app.library.records.clear();
    for (name, page) in [
        ("home", crate::app::workspace::Page::Home),
        ("comics", crate::app::workspace::Page::Comic),
        ("settings", crate::app::workspace::Page::Settings),
    ] {
        app.workspace.page = page;
        app.settings_open = page == crate::app::workspace::Page::Settings;
        app.preferences_tab = crate::app::PrefTab::Providers;
        capture.save(
            &ctx,
            [1280, 800],
            &directory.join(format!("icons-{name}.png")),
            |ui| app.workspace_ui(ui),
        );
    }
    app.workspace.page = crate::app::workspace::Page::Comic;
    app.settings_open = false;
    app.comic = reader_fixture(&ctx);
    app.comic.view = View::Reader;
    capture.save(
        &ctx,
        [1920, 900],
        &directory.join("icons-reader.png"),
        |ui| app.workspace_ui(ui),
    );
    eprintln!("Embedded Phosphor subset: {} bytes", icons::FONT.len());
}

pub(in crate::app) fn settings_fixture(ctx: &egui::Context) -> crate::app::App {
    let mut app = crate::app::App::with_state(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        vec![],
        crate::settings::Settings::default(),
        fixture(ctx),
    );
    let mut settings = crate::settings::Settings::default();
    settings.ai.providers.push(crate::ai::Provider {
        id: "fixture-provider".into(),
        name: "我的 AI 服务".into(),
        base_url: "https://api.example.com/v1".into(),
        api_key: "not-a-real-key".into(),
        models: vec!["qwen/omni-flash".into(), "qwen/image-edit".into()],
    });
    settings.llm.selected = crate::ai::ModelRef {
        provider: "fixture-provider".into(),
        model: "qwen/omni-flash".into(),
    };
    settings.comics.image_selected = crate::ai::ModelRef {
        provider: "fixture-provider".into(),
        model: "qwen/image-edit".into(),
    };
    app.settings = settings.clone();
    app.preferences_draft = settings;
    app
}

#[test]
fn fullscreen_pages_fill_the_viewport_hide_window_chrome_and_escape_without_leaving() {
    use crate::app::workspace::Page;
    for scale in [1.0, 1.5, 2.0] {
        for page in [
            Page::Home,
            Page::Listening,
            Page::Playing,
            Page::Comic,
            Page::Settings,
        ] {
            let ctx = egui::Context::default();
            let mut app = settings_fixture(&ctx);
            app.queue = Default::default();
            app.queue_inactive = Default::default();
            app.library.records.clear();
            app.workspace.page = page;
            app.settings_open = page == Page::Settings;
            if page == Page::Comic {
                app.comic = reader_fixture(&ctx);
                app.comic.view = View::Reader;
            }
            ctx.set_pixels_per_point(scale);
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 900.0));
            let mut frame = eframe::Frame::_new_kittest();
            for phase in 0..4 {
                let mut raw = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
                viewport.fullscreen = Some(true);
                viewport.maximized = Some(false);
                if phase == 3 {
                    raw.events.push(egui::Event::Key {
                        key: Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let mut output = ctx.run_ui(raw, |ui| eframe::App::ui(&mut app, ui, &mut frame));
                output.textures_delta.clear();
                if phase == 2 {
                    assert!(
                        output.shapes.iter().any(|s| matches!(&s.shape,
                        Shape::Rect(r) if r.rect == screen && r.fill.a() == 255)),
                        "the page background must cover the complete viewport"
                    );
                    if matches!(page, Page::Home | Page::Listening | Page::Settings) {
                        assert!(
                            output.shapes.iter().any(|s| matches!(&s.shape,
                            Shape::Rect(r) if r.rect.width() == screen.width()
                                && r.rect.height() == 100.0 && r.rect.bottom() == screen.bottom()
                                && r.fill == SURFACE)),
                            "the playback bar must touch the bottom edge"
                        );
                    }
                    for index in 0..3 {
                        assert!(
                            ctx.read_response(Id::new(("window-button", index)))
                                .is_none()
                        );
                    }
                    assert!(ctx.read_response(Id::new("window-drag-region")).is_none());
                    assert_eq!(window::controls_width(&ctx), 0.0);
                }
                if phase == 3 {
                    assert!(
                        app.workspace.page == page,
                        "Escape must exit fullscreen without navigating away"
                    );
                    if page == Page::Comic {
                        assert!(app.comic.reading());
                    }
                    assert!(
                        output.viewport_output[&egui::ViewportId::ROOT]
                            .commands
                            .contains(&egui::ViewportCommand::Fullscreen(false))
                    );
                }
                output.textures_delta.clear();
            }
        }
    }
}

fn settings_frame(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    app.workspace.page = crate::app::workspace::Page::Settings;
    app.settings_open = true;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))),
            events,
            ..Default::default()
        },
        |ui| app.workspace_ui(ui),
    );
    output.textures_delta.clear();
    output
}
fn settings_text(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        Shape::Text(shape) if shape.galley.text() == text => {
            Some(shape.pos + shape.galley.size() * 0.5)
        }
        _ => None,
    })
}

#[test]
fn home_comic_shelf_only_shows_favorites_without_favorite_controls() {
    let ctx = egui::Context::default();
    design::apply(&ctx);
    let mut state = fixture(&ctx);
    // A favorite without reading progress must still be on the home shelf.
    let mut unread = state.entries()[0].clone();
    unread.progress = None;
    unread.last_read = 0;
    state.library.as_mut().unwrap().save(unread).unwrap();
    for show_controls in [false, true] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                if show_controls {
                    state.favorite_preview(ui, &Default::default());
                } else {
                    state.home_preview(ui, &Default::default());
                }
            },
        );
        output.textures_delta.clear();
        for entry in state.entries() {
            assert_eq!(
                settings_text(&output, &entry.book.title).is_some(),
                entry.favorite
            );
        }
        let controls = output.shapes.iter().filter(|s| matches!(&s.shape,
            Shape::Rect(r) if r.rect.size() == vec2(32.0, 32.0) && r.fill == Color32::from_black_alpha(170))).count();
        assert_eq!(controls, if show_controls { 6 } else { 0 });
    }
}

fn app_frame(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut frame = eframe::Frame::_new_kittest();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| eframe::App::ui(app, ui, &mut frame),
    );
    output.textures_delta.clear();
    output
}

#[test]
fn history_delete_button_and_undo_update_recent_and_home_without_changing_the_queue() {
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-data")
        .join(uuid::Uuid::new_v4().to_string());
    let path = directory.join("library.json");
    app.library = crate::app::library::Library::open(path);
    app.library.remember(crate::app::library::Record {
        path: "fixture-song.flac".into(),
        title: "History fixture".into(),
        played: 100,
        position: 40.0,
        duration: 200.0,
        music: true,
        ..Default::default()
    });
    app.queue = Default::default();
    app.queue_inactive = Default::default();
    app.queue
        .items
        .push(crate::queue::Item::file("fixture-song.flac".into()));
    app.workspace.page = workspace::Page::Recent;
    let size = vec2(1280.0, 800.0);
    let mut output = app_frame(&ctx, &mut app, size, vec![]);
    for _ in 0..2 {
        output = app_frame(&ctx, &mut app, size, vec![]);
    }
    let row = settings_text(&output, "History fixture").unwrap();
    output = app_frame(&ctx, &mut app, size, vec![egui::Event::PointerMoved(row)]);
    let trash = settings_text(&output, icons::TRASH).expect("Delete action appears on hover");
    for pressed in [true, false] {
        app_frame(
            &ctx,
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(trash),
                egui::Event::PointerButton {
                    pos: trash,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(app.library.records[0].played, 0);
    assert!(app.history_undo.is_some());
    assert_eq!(app.queue.items.len(), 1);
    output = app_frame(&ctx, &mut app, size, vec![]);
    assert!(settings_text(&output, "History fixture").is_none());
    let undo = settings_text(&output, "撤销").unwrap();
    for pressed in [true, false] {
        app_frame(
            &ctx,
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(undo),
                egui::Event::PointerButton {
                    pos: undo,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(app.library.records[0].played, 100);
    app.remove_history("fixture-song.flac");
    app.workspace.page = workspace::Page::Home;
    output = app_frame(&ctx, &mut app, size, vec![]);
    assert!(settings_text(&output, "History fixture").is_none());
    drop(app);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "Requires a GPU; renders changed home, lyrics, history and update layouts"]
fn media_update_visuals() {
    let directory = PathBuf::from("target/ui-review");
    std::fs::create_dir_all(&directory).unwrap();
    let mut capture = Capture::new();
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    app.library.records.clear();
    app.queue = Default::default();
    app.queue_inactive = Default::default();
    let mut frame = eframe::Frame::_new_kittest();
    app.workspace.page = workspace::Page::Listening;
    app.title = "远方的声音 - 山川遥".into();
    app.loading = true;
    app.artwork = Some(app.comic.covers.values().next().unwrap().tiles[0].clone());
    app.lyrics = crate::lyrics::Lyrics::parse(
        "[00:00.00]词：山川遥\n[00:05.00]曲：山川遥\n[00:10.00]当风吹过远方的山谷\n[00:15.00]我听见熟悉的声音\n[00:20.00]沿着星光走向你\n[00:25.00]故事在这里继续\n[00:30.00]回忆像一封远方的信\n[00:35.00]留在此刻的旋律里",
    );
    capture.save(
        &ctx,
        [1280, 800],
        &directory.join("music-lyrics-fixed.png"),
        |ui| eframe::App::ui(&mut app, ui, &mut frame),
    );
    app.updater.offer_fixture();
    capture.save(
        &ctx,
        [1280, 800],
        &directory.join("update-dialog.png"),
        |ui| eframe::App::ui(&mut app, ui, &mut frame),
    );
    app.updater.dialog_visible = false;
    app.loading = false;
    app.workspace.page = workspace::Page::Home;
    capture.save(
        &ctx,
        [1280, 800],
        &directory.join("home-favorite-comics.png"),
        |ui| eframe::App::ui(&mut app, ui, &mut frame),
    );
}

#[test]
fn populated_lyrics_never_cover_tabs_and_info_remains_clickable_at_every_scale() {
    for scale in [1.0, 1.5, 2.0] {
        for (size, queue) in [
            (vec2(1280.0, 800.0), false),
            (vec2(960.0, 600.0), true),
            (vec2(800.0, 600.0), true),
        ] {
            let ctx = egui::Context::default();
            let mut app = settings_fixture(&ctx);
            ctx.set_pixels_per_point(scale);
            app.workspace.page = workspace::Page::Listening;
            app.queue_open = queue;
            app.lyrics = crate::lyrics::Lyrics::parse(
                &(0..100)
                    .map(|i| format!("[{i:02}:00.00]Lyric line {i}\n"))
                    .collect::<String>(),
            );
            let mut output = app_frame(&ctx, &mut app, size, vec![]);
            for _ in 0..2 {
                output = app_frame(&ctx, &mut app, size, vec![]);
            }
            let info = settings_text(&output, "信息").expect("Info tab remains visible");
            let lyric_background = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::Rect(r)
                        if r.fill == BACKGROUND && r.corner_radius == CornerRadius::same(12) =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .expect("Populated lyric panel is rendered");
            assert!(lyric_background.top() >= info.y + 16.0 + 16.0);
            for pressed in [true, false] {
                app_frame(
                    &ctx,
                    &mut app,
                    size,
                    vec![
                        egui::Event::PointerMoved(info),
                        egui::Event::PointerButton {
                            pos: info,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(app.workspace.listening_tab, 1);
        }
    }
}

#[test]
fn update_modal_is_global_localized_and_escape_does_not_navigate_underneath() {
    for page in [workspace::Page::Listening, workspace::Page::Comic] {
        let ctx = egui::Context::default();
        let mut app = settings_fixture(&ctx);
        app.workspace.page = page;
        if page == workspace::Page::Comic {
            app.comic = reader_fixture(&ctx);
            app.comic.view = View::Reader;
        }
        app.updater.offer_fixture();
        let mut output = app_frame(&ctx, &mut app, vec2(1280.0, 800.0), vec![]);
        for _ in 0..2 {
            output = app_frame(&ctx, &mut app, vec2(1280.0, 800.0), vec![]);
        }
        assert!(settings_text(&output, "发现新版本").is_some());
        assert!(settings_text(&output, "修复按钮遮挡。").is_some());
        assert!(settings_text(&output, "Keep controls visible.").is_none());
        app_frame(
            &ctx,
            &mut app,
            vec2(1280.0, 800.0),
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.updater.dialog_visible);
        assert!(app.workspace.page == page);
        if page == workspace::Page::Comic {
            assert!(app.comic.reading());
        }
    }
}
fn settings_click(ctx: &egui::Context, app: &mut crate::app::App, point: egui::Pos2) {
    for pressed in [true, false] {
        settings_frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    for _ in 0..20 {
        settings_frame(ctx, app, vec![]);
    }
}

#[test]
fn settings_save_is_visible_only_for_edits_including_api_keys() {
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    app.preferences_tab = crate::app::PrefTab::Providers;
    for _ in 0..3 {
        settings_frame(&ctx, &mut app, vec![]);
    }
    let output = settings_frame(&ctx, &mut app, vec![]);
    assert!(settings_text(&output, "保存").is_none());
    assert!(settings_text(&output, "搜索媒体、音乐或漫画").is_none());
    let key = app.preferences_draft.ai.providers[0].api_key.clone();
    app.preferences_draft.ai.providers[0].api_key = "changed-fixture-key".into();
    for _ in 0..3 {
        settings_frame(&ctx, &mut app, vec![]);
    }
    assert!(settings_text(&settings_frame(&ctx, &mut app, vec![]), "保存").is_some());
    app.preferences_draft.ai.providers[0].api_key = key;
    for _ in 0..3 {
        settings_frame(&ctx, &mut app, vec![]);
    }
    assert!(settings_text(&settings_frame(&ctx, &mut app, vec![]), "保存").is_none());
}

#[test]
fn provider_models_panel_opens_without_repeating_connection_forms() {
    let ctx = egui::Context::default();
    let mut app = settings_fixture(&ctx);
    let mut second = app.settings.ai.providers[0].clone();
    second.id = "second-fixture".into();
    second.name = "备用服务".into();
    app.settings.ai.providers.push(second);
    app.preferences_draft = app.settings.clone();
    app.preferences_tab = crate::app::PrefTab::Providers;
    for _ in 0..3 {
        settings_frame(&ctx, &mut app, vec![]);
    }
    let output = settings_frame(&ctx, &mut app, vec![]);
    assert_eq!(
        output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, Shape::Text(t) if t.galley.text() == "服务地址"))
            .count(),
        1
    );
    let point = settings_text(&output, "2 个模型").expect("model manager button missing");
    for pressed in [true, false] {
        settings_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    for _ in 0..3 {
        settings_frame(&ctx, &mut app, vec![]);
    }
    let output = settings_frame(&ctx, &mut app, vec![]);
    assert!(settings_text(&output, "qwen/omni-flash").is_some());
    settings_click(&ctx, &mut app, settings_text(&output, "手动添加").unwrap());
    let output = settings_frame(&ctx, &mut app, vec![]);
    settings_click(
        &ctx,
        &mut app,
        settings_text(&output, "模型 ID").expect("manual model input missing"),
    );
    settings_frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("manual/new-model".into())],
    );
    let output = settings_frame(&ctx, &mut app, vec![]);
    settings_click(&ctx, &mut app, settings_text(&output, "添加").unwrap());
    assert!(
        app.preferences_draft.ai.providers[0]
            .models
            .iter()
            .any(|m| m == "manual/new-model")
    );
    assert!(settings_text(&settings_frame(&ctx, &mut app, vec![]), "保存").is_some());
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
        ai: settings.ai.clone(),
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
                    Some(icons::refresh),
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
