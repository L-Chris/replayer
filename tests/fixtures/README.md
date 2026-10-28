# Streaming regression corpus

These deterministic synthetic white-noise files have 70 seconds of mono audio.
MP3, FLAC and tail-indexed M4A each exceed three 512 KiB cache blocks, so tests
can withhold the sequential download and verify independent range seeks.
They are project-owned fixtures under the repository license. Tests read the
committed bytes and require no external encoder or FFmpeg executable.

`adapter.flac` is a four-second synthetic tone with the title "QQ adapter test",
used to verify encrypted local music metadata, playback and seeking.

`av-audio-preroll.mkv` combines remdia's MIT-licensed synthetic H.264 video and
PCM tone, with the video delayed by 1.25 seconds. It verifies that initial open
and paused seek keep demuxing until video is ready, even when paused audio has
already filled the normal half-second read-ahead window. No third-party media
is included.
