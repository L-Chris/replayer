# Historical notes

These are dated observations, not current compatibility guarantees. Use the [user guide](guide.md) and [development instructions](development.md) for current behavior.

## Earlier playback and subtitle trials

An earlier local test used a roughly 53-minute HEVC/E-AC-3 episode with embedded subtitles. Sampled playback, seeking, pause and replay passed with Windows hardware decoding; the entire episode was not watched or transcribed as part of that check.

Short AI subtitle trials demonstrated concurrent generation and SRT formatting, but also missed or misheard dialogue. Timing/layout warnings did not measure semantic accuracy. Local media paths, generated sample links and old test-count/performance claims have been removed from current instructions because they are not reproducible repository fixtures.

## Thunder / Xunlei research · September 2026

An earlier investigation of Thunder 25.1.10.1612 found a session-specific local HTTP playback stream that FFmpeg could read. It did not establish a supported third-party launch contract, persistent stream lifetime or reliable takeover of the application's playback button. Preallocated temporary files were not safe substitutes for verified downloaded ranges.

Older [XMP forwarding adapters](https://github.com/fondoger/xmp-2-potplayer) and [download-engine research](https://github.com/tomjiu/smart-downloader/blob/main/docs/research/xunlei/xunlei_engine_research.md) were investigation references, not verified integrations for that installation. Their ports, paths and ABI assumptions must not be copied into current setup instructions.

No Thunder button integration is presented as a shipped feature. Pordo's implemented magnet playback uses its embedded torrent engine and is documented separately in the user guide.
