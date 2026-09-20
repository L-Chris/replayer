# Magnet playback (Windows)

Choose **Open magnet link** on the home screen, paste a `magnet:` URL and resolve
it. After metadata arrives, choose a video from the file list. Only the selected
video is downloaded, plus any unavoidable shared boundary pieces. A magnet can
also be passed as the player's command-line argument.

The engine is librqbit 9.0.1, embedded in the player with TCP/uTP and IPv4 DHT.
Pure v2 magnets without a v1 `btih` hash are currently rejected. The read-only
HTTP bridge binds to an ephemeral loopback port and requires a random per-task
token. No torrent-management API is exposed to the network.

The player waits for verified pieces rather than reading zeros from a sparse
file. When data runs out it enters Buffering and freezes the clock. Seeking
interrupts a superseded FFmpeg read and prioritizes the newly requested location.
Network open/read/seek operations have a five-minute ceiling and remain
cancellable. Local-file timeouts are unchanged.

AI subtitles use a separate cache-only endpoint. It checks the torrent's verified
piece bitmap before serving data, without registering a playback-priority stream.
Missing audio is retried after a delay, not counted as an LLM failure. Once the
whole selected video is complete, new subtitle jobs use its local file directly.
The usual model configuration, concurrency, language and seek scheduling apply.

**Magnet** in the top toolbar opens task controls: video selection, downloaded
bytes, download speed, pause/resume and stop. Closing/stopping a task stops the
engine and retains downloaded files. Pausing download also pauses playback;
resume playback with the player control. BT uploads are capped at 256 KiB/s.

Default cache: `%LOCALAPPDATA%/replayer/torrents/<infohash>/data`.
`REPLAYER_TORRENT_CACHE` overrides the cache root. Cached metadata and downloaded
files can be reused when reopening the same magnet; data is hash-checked again.
There is no automatic cache deletion. Close the task before manually deleting its
cache folder. No background torrent session survives application exit.

## TUN / proxy networks

TUN can capture UDP trackers, DHT and TCP/uTP peer connections. A working browser
proxy does not establish that the same outbound supports BT. If metadata stalls,
inspect the proxy's Connections view for the player process and its matched rule.
The application does not edit proxy configuration or disable TUN automatically.
The magnet dialog's **Copy Clash routing rules** button generates an example using
the current executable name and configured LLM host. Review it in your proxy
client and replace `PROXY` with the appropriate policy group before applying it.

For Mihomo/Clash in **rule** mode, a direct player rule may help when the selected
proxy rejects BT. Since LLM requests run in the same process, put the model
endpoint's proxy rule before the process rule, and preserve update-service proxy
rules if needed. Example (replace `PROXY` with your actual policy group):

```yaml
prepend:
  - DOMAIN,chat.rethinkos.com,PROXY
  - DOMAIN,api.github.com,PROXY
  - DOMAIN,github.com,PROXY
  - DOMAIN-SUFFIX,githubusercontent.com,PROXY
  - PROCESS-NAME,replayer.exe,DIRECT
```

This is a merge-profile example, not a full Mihomo configuration. The equivalent
entries can be placed first under `rules:` in a full configuration. If the LLM
endpoint or executable name changes, update its rule. BT direct routing uses the
local internet connection; only configure it if that is the intended route.

## Verification

Offline tests cover multi-file magnet metadata exchange over loopback, selected
file streaming, HTTP Range behavior, unverified-cache rejection, cache wait and
recovery, buffering-clock freeze, and cancellation of an old blocked seek.

For an explicitly authorized live test, save a magnet in an ignored local file:

```powershell
$env:REPLAYER_TORRENT_CACHE = "$PWD/target/torrent-test/cache"
./target/debug/replayer.exe --torrent-selftest target/torrent-test/magnet.txt
```

The test selects the first video, waits for playback, then seeks to 300 seconds and
rapidly to 600/120/60 seconds. It requires a video longer than 600 seconds. Test
logs/cache are local and must not be committed. Real-peer availability and network
routing determine startup/seek delays.
