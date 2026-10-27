# Original UI design references

These generated mockups are historical visual references, not screenshots or a current behavior specification. Example media, lyrics and books are illustrative. Follow the [user guide](../../guide.md) for current functionality.

## Mockups

| Image | View |
| --- | --- |
| [00-home.png](00-home.png) | Home |
| [01-music-library.png](01-music-library.png) | Music library and queue |
| [02-music-now-playing.png](02-music-now-playing.png) | Music playback and timed lyrics |
| [03-video-now-playing.png](03-video-now-playing.png) | Video and AI subtitles |
| [04-comic-search.png](04-comic-search.png) | Comic search and bookshelf |
| [05-comic-detail.png](05-comic-detail.png) | Comic details and chapters |
| [06-comic-reader.png](06-comic-reader.png) | Comic reader and translation comparison |
| [07-settings.png](07-settings.png) | Settings and AI services |

The shared direction is a dark background, mint accents, cover-led browsing and immersive playback/reading. Exact controls and layouts have evolved; old pixel dimensions and implementation checklists are intentionally omitted.

The original generation prompts are preserved in [PROMPTS.md](PROMPTS.md). Current visual checks use `cargo test --locked comic_catalog_visuals -- --ignored --nocapture` and write review images under `target/ui-review/`.
