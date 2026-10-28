# Streaming regression corpus

These deterministic synthetic white-noise files have 70 seconds of mono audio.
MP3, FLAC and tail-indexed M4A each exceed three 512 KiB cache blocks, so tests
can withhold the sequential download and verify independent range seeks.
They are project-owned fixtures under the repository license. Tests read the
committed bytes and require no external encoder or FFmpeg executable.

`adapter.flac` is a four-second synthetic tone with the title "QQ adapter test",
used to verify encrypted local music metadata, playback and seeking.
