# Music and video playback

Replayer automatically chooses the interface from the media's actual tracks.
Normal video tracks use video mode; audio-only media and audio with attached cover
art use music mode. File extensions are used for picker/torrent filtering, not for
the final playback-mode decision.

## Music

- Common supported selections include MP3, FLAC, WAV, M4A/AAC, Ogg/Opus and AIFF.
- Embedded title, artist and album are displayed when present. Missing tags fall
  back to the filename without showing artificial artist/album placeholders.
- Embedded JPEG/PNG/WebP artwork is decoded once, with image-size/allocation
  limits. Missing or unsupported artwork uses the music-note placeholder.
- Playback, seeking, volume, previous/next controls remain visible. Clicking the
  background or artwork does not toggle playback or full screen.
- Previous restarts the current song after three seconds; otherwise it selects
  the preceding queue item.
- Space pauses/resumes; arrows seek; M toggles mute. Music mode does not use F or
  double-click to enter full screen. Switching from full-screen video to music
  exits full screen.
- The queue uses a collapsible right sidebar in both music and video modes.
  No video decoder/frame queue drives audio-only playback.

## QQ Music local files

Open Files and drag/drop accept `.qmc0`, `.qmc2`, `.qmc3`, `.qmcflac`,
`.qmcogg`, `.mflac`, `.mflac0`, `.mflac1`, `.mgg`, `.mgg0`, `.mgg1`,
`.mggl`, `.mflach` and `.mmp4`. Extensions select the adapter, not the output codec.
Plain audio with these extensions is passed through. Supported encrypted variants:

- Legacy QMC1 fixed mask.
- QMC2 Map and segmented RC4, with V1 ekey envelopes embedded in a legacy
  length footer or QTag.
- STag and musicex v1 (192-byte or larger metadata footer) with a user-provided
  V1 ekey. These formats do not supply an embedded key.

For a missing/wrong key, use **Import song key and retry** on the error panel and
select a text file containing this song's Base64 ekey. It is used in memory for
that playback; replaying the queue item requires importing it again. Alternatively,
place the text beside the audio as `song.mflac.ekey` (retain the original extension).
This is a per-song encrypted key, not a QQ login token or an LLM API key.
The player does not retrieve keys from accounts or other processes.

EncV2 key envelopes and unrecognized footer versions are explicitly unsupported.
Actual modern-client downloads still need sample-based verification; a filename
extension alone does not guarantee compatibility. Encrypted torrent streaming is
not included: finish downloading and open the local file.

Preparation runs on the demux worker in bounded chunks and can be cancelled by
switching/stopping playback. It preserves the original compressed audio bytes,
tags and artwork, and does not overwrite the source or transcode to MP3. Temporary
playback files in `%LOCALAPPDATA%/replayer/music-cache` are removed when the media
closes, including handled failures. A process crash can leave temporary files.
There is no persistent decoded-cache reuse. FFmpeg validates the resulting media
on open and reports decoding failures during playback.

Tests compare QMC2 against independent libtakiyasha vectors, exercise footer/key
errors and cache cleanup, and play/seek generated encrypted FLAC through FFmpeg.

## Queue behavior

Open Files supports multiple selection and replaces the queue. Add Files and
Shift-drop append without replacing the current item. Ordinary multi-file drop
replaces the queue and begins the first item. Directories are not scanned.

The queue accepts mixed local music/video files. It advances after actual playback
completion and changes the interface accordingly. Items can be selected, moved up
or down, or removed; the currently playing item cannot be removed. Removing a
queue entry never deletes the media file. Queue order is kept for the current run.

The sidebar opens when a nonempty queue is available. Hide collapses it; the Queue
button toggles it again. Its visibility is retained when switching between music
and video. The playback area shrinks to leave room for the sidebar.

Magnet file selection includes music and video. The selected item and the other
supported files form a queue for that torrent. Moving to local files or stopping
the torrent removes that torrent's entries and retains its disk cache.

## Video

Video keeps its immersive canvas, edge-triggered toolbars, click-to-pause,
double-click/F full screen, Esc exit, and AI subtitle controls. The queue uses the
same right sidebar. Artwork and music details do not remain behind after switching.

This first stage does not add lyric generation, podcast workflows, gapless
playback, random/repeat modes, persistent libraries or a mini-player. AI subtitle
generation stays in video mode until dedicated music/lyrics behavior is added.

## Verification

Tests generate WAV/FLAC/MP3/M4A fixtures, including covers and tags, and exercise
audio EOF, resampling, pause, rapid seek, replay and device failure. Unit tests use
a simulated audio device so CI does not require a sound card. A real-device smoke
test (muted) is available for local verification:

```powershell
./target/debug/replayer.exe --selftest-music path/to/song.wav
```

Use a track longer than two seconds. Video self-tests continue to use `--selftest`.
