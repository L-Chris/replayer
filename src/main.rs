#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod player;
mod renderer;
mod selftest;
mod settings;
mod subtitles;
mod updater;

use app::App;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--subtitles") {
        let result = match (args.get(1), args.get(2)) {
            (Some(input), Some(output)) => subtitle_cli(input, output, &args[3..]),
            _ => Err(anyhow::anyhow!(
                "Usage: replayer --subtitles <media> <output.srt>"
            )),
        };
        if let Err(error) = result {
            eprintln!("字幕生成失败: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }

    if args.first().map(String::as_str) == Some("--selftest") {
        let path = args.get(1).cloned().unwrap_or_default();
        if let Err(e) = selftest::run(path) {
            eprintln!("selftest error: {e:?}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let initial = args.into_iter().next();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("replayer")
            .with_inner_size([1100.0, 640.0])
            .with_min_inner_size([420.0, 280.0]),
        ..Default::default()
    };
    eframe::run_native(
        "replayer",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial)))),
    )
}

fn subtitle_cli(input: &str, output: &str, args: &[String]) -> anyhow::Result<()> {
    let settings = settings::Settings::load();
    let mut options = subtitles::Options {
        language: settings.language,
        concurrency: settings.subtitle_concurrency,
        ..Default::default()
    };
    let mut duration = None;
    anyhow::ensure!(args.len().is_multiple_of(2), "subtitle options need values");
    for pair in args.chunks_exact(2) {
        match pair[0].as_str() {
            "--language" => options.language = settings::Language::parse(&pair[1])?,
            "--concurrency" => options.concurrency = pair[1].parse()?,
            "--start" => options.start = pair[1].parse()?,
            "--duration" => duration = Some(pair[1].parse::<f64>()?),
            other => anyhow::bail!("unknown subtitle option: {other}"),
        }
    }
    options.end = duration.map(|duration| options.start + duration);
    subtitles::run_cli(
        std::path::Path::new(input),
        std::path::Path::new(output),
        options,
    )
}
