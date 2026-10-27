# Native household launcher

The `family` package is an optional launcher alongside the Cinema and book applications. It starts its own native screen without entering the Plex account, profile or first-run flows. It uses the existing media engine for an independent, silent background video; launching an application uses the platform application-manager adapter.

The launcher uses the live video plane without a static replacement image or cover fade. The legacy `cover` field is accepted only for configuration compatibility and is never loaded or rendered. Icons retain their separate 1 MiB/512×512 limits.

The background can also declare `localCache: {"sha256": "<64 lowercase hex characters>", "bytes": 83963525}`. This names only `/media/internal/pinpin-family/cache/<sha256>.mp4`; it does not accept an arbitrary file path or relax HTTP(S) URL validation. The existing demux worker opens a regular, non-symlink file, checks its declared size (at most 1 GiB), retains that same open file for AVIO reads and seeks. An administrator can verify an installed clip once with `node tools/write-local-cache-receipt.cjs SHA256 BYTES`, after making its cache directory root-owned and protected, and its media file root-owned and read-only. The writer hashes the actual opened file, checks its complete identity before and after, and atomically publishes an app-readable receipt. On subsequent opens the demux worker reuses that proof only when root ownership, permissions, configured hash/size, device, inode and exact nanosecond modification/change timestamps still match the opened file. A missing, malformed, stale or unprotected receipt falls back to the original cancellable SHA-256 check; it never becomes a verification override. Missing or corrupt cache files retain the configured HTTP(S) background as fallback. Verification and file reads run off the UI thread. Private media is provisioned separately from the public application package.

A private, versioned configuration lives at `/media/internal/.com.beb.plxnative.family-home.json`. It accepts one to five distinct application IDs. The household dock uses three entries: Cinema Kids, Cinema Parents and the book. There are no mandatory Apps or Edit buttons, and external-service shortcuts do not belong in this row. An example using documentation-only addresses and identifiers is:

```json
{
  "version": 1,
  "background": {
    "url": "http://192.0.2.1:32400/library/parts/1/example/file.mov",
    "videoCodec": "hevc",
    "fps": 30,
    "width": 3840,
    "height": 2160,
    "loop": true
  },
  "apps": [
    {
      "id": "org.example.children",
      "title": "Children",
      "icon": "/media/internal/pinpin-family/children.png"
    },
    {"id": "org.example.parents", "title": "Parents"},
    {"id": "org.example.book", "title": "Book"}
  ]
}
```

The optional application icons must be local PNGs under `/media/internal/`, at most one MiB and 512 by 512 pixels. Missing artwork retains a focusable fallback tile. Titles remain application metadata, but no title labels are drawn above or inside dock controls. The application reads and decodes artwork during configuration bootstrap; the shared UI resource uploads textures during its existing prepare budget. Icons grow with the shared focus spring. The dock material uses half its previous opacity while icons remain opaque. Its translucent theme tint does not blur the separate hardware video plane.

The top band displays local system time and the existing localized calendar date. A presentation adapter samples wall time separately from the application's frame/replay clock and delivers status to the screen. The dock and top band remain visible at all times; inactivity never turns the launcher into a video-only view. Resume and ordinary Home relaunch show the dock immediately, independently of video buffering; a parked frame gap does not consume the new idle interval. Root Back stays in the launcher and returns an active scene to its default.

The existing resident may mirror its `home.status` observation to `/media/internal/pinpin-family/home-status.json`. A bounded asynchronous worker reads at most 4096 bytes every 25 seconds; no status network request or file read runs in the frame path. The consumer requires schema 1 and integer `observedAtUnix` seconds, rejects observations more than 120 seconds old or more than 60 seconds in the future, and displays unknown for unavailable/disconnected/invalid data. A valid connected Wi-Fi observation displays its measured signal when available; Ethernet is identified separately. The SSID is not displayed. Weather remains absent until an actual location and observation are configured.

Background declarations must describe the actual video: supported H.264/HEVC codec, source dimensions and source frame rate. The initial H.264 bound is 30 fps; HEVC is accepted up to 60 fps. No higher-rate source is relabelled to satisfy these bounds. This is video-only playback, with no invented audio stream, Plex watch identity, queue or timeline reporting. A verified local looping VOD seeks to the beginning four seconds before the end, retaining its existing decoder and video plane instead of stopping and loading again. This avoids the normal EOS unbind/rebind transition. Pending seeks and an active pointer gesture retain priority; nonlocal streams keep their existing EOS restart. This does not spend the trailer preview seek budget.

The separate default-off `pointerDirectionVideo: true` selects a continuous-play
experiment for a specifically prepared local asset: 120 seconds forward, the
same 3600 frames in reverse order, then four seconds of priming padding (244
seconds at 30 fps). It takes priority over the older shuttle and requires the
actual verified-local decoder; managed scenes retain their own controls. Normal
native Play runs uninterrupted between direction changes. Accumulated horizontal
cursor displacement of 0.005 normalized units selects left/reverse or right/forward,
rather than selecting by cursor position on the screen. Small alternating jitter
is filtered; vertical movement keeps the last direction. A real sign change
requests one mirrored seek using the current landed PTS. Changes received while
loading or seeking coalesce until ready. Each direction loops within its own
120-second segment. One second without actual XY motion releases input ownership
but does not pause the video or change its last direction. This uses pre-encoded
frames and ordinary seek/Play; it does not introduce a negative-rate decoder API.
The media contract and visible transition quality require device acceptance.

The top-level `pointerShuttle: true` optionally steers this passive looping
background through existing native pause, absolute-seek and resume operations.
It is off by default. Fresh horizontal pointer displacement from the center
sets desired signed timeline velocity, proportional up to eight seconds of source time
per second at the edges. The centre uses continuous native Play at normal +1×
while the gesture retains logical ownership. It never pauses an already playing
centre or approximates normal playback with repeated seeks. Returning from
paused seek steering resumes once the in-flight seek settles and Play is accepted;
moving outside the centre pauses once before seek steering begins. The same
local loop mechanism may seek to zero near the end while centre Play owns it,
retaining the decoder rather than waiting for EOS.
Left/right absolute seeks are coalesced to at most four per second and wait for loading,
pending seeks and seek preroll to settle. Frame-clock gaps are bounded and
targets wrap within the measured duration. This approximates reverse motion
through seeks; it does not claim a hardware negative playback-rate API.
Actual X or Y coordinate changes retain control. Duplicate stationary packets
do not renew ownership or reclaim it after release. After one complete second
without movement, ownership returns to ambient playback. An already playing
centre continues without another Play call; paused seek steering resumes after
the in-flight seek settles and Resume is accepted. Refusals retain ownership
for a bounded retry. Dock,
modal and D-pad handoff, window backgrounding and leaving the scene retain their
existing input priority. A managed Mini scene keeps its
own input authority and is never controlled by this local shuttle. Actual
smoothness and pointer behavior still require TV verification.

Private `pointerDiagnostics: true` enables sampled observations in the existing
Family event log. Ingress is sampled before eligibility and D-pad release checks:
raw SDL pixels, logical/normalized coordinates, dock exclusion and native plane
ownership remain distinguishable. Action/state samples include the desired
timeline, actual PTS, readiness, pending/preroll state and pause/resume return
values. Input/state sampling is bounded to 250 ms and action sampling to 125 ms.
The option is read only at bootstrap, defaults off, supplies no behavior override
and does not log service URLs or capabilities. Disable it after device diagnosis.

Configuration, service URLs and any capabilities remain private. Installing this package does not replace Cinema or book package identities, change the factory launcher, or redirect the remote's Home button. Those integrations require their own reversible deployment steps.


## Optional native scene return channel

Passive PMS backgrounds remain unchanged when `controls` is absent. To opt into an interactive Mini scene, set `background.url` to that scene's native video stream and add `"controls": {"inputURL": "http://192.0.2.1:4188/__render/native/input?cap=example"}`. The input endpoint and video URL must have the same HTTP(S) scheme, host and effective port. Keep the actual capability private; the example is not a usable credential.

One bounded worker sends the shared `kind: input` envelope using the existing HTTP transport. It keeps the latest pointer/held-key state, coalesces activation and zoom edges, sends at most once per 50 milliseconds, caps requests at 250 milliseconds and responses at 4096 bytes, and follows no redirects. No network operation runs in the frame handler. A failed request releases steering; the peer also expires the input lease after 750 milliseconds. Activation is not blindly replayed after an ambiguous HTTP failure.

Native pointer positions are normalized to the video canvas. Steering requires an actual bound background video plane; stopped or ended playback releases the return channel. Dock controls and their magnification margin are excluded, as are modal surfaces; the existing D-pad/pointer handoff and remote OK synthesis stay authoritative. Scene clicks and wheel zoom apply only outside those controls. Keyboard WASD, E, Space and Shift can be relayed while the scene pointer is engaged; D-pad navigation, Back and Exit remain ordinary launcher input.

`engaged: true` means intentional scene steering, including an explicit centered pointer that holds the scene. `engaged: false` releases steering so the shared producer returns to its ambient forward policy. Idle expiry, backgrounding, failed transport and shutdown release steering. The same return channel serves forest, Atlas and other scenes; none adds its own transport. Native live-stream compatibility and the actual TV return path still require device verification before changing a household's working passive configuration.

## Shared Mini scenes

A private optional `rendering` object supplies `queryURL` and a registry `scene`
(default `forest`). An app entry can additionally declare `scene: "atlas"` or
`"alambique"`; its ordinary installed app id remains available when rendering is
not configured. The controller uses the shared HTTP transport on bounded workers,
keeps passive media until a ready grant arrives, validates grant origins, and
heartbeats only while foreground. Root Back and an ordinary Home relaunch return
to the configured default. Backgrounding retires the grant; late replies are
stopped rather than displayed. The Mini's expiry remains authoritative if the TV
process disappears before cleanup can run. No scene supplies commands or URLs.

A host test establishes controller ordering, not native receipt of a live scene.
Verify the actual TV's decoded frames and returned pointer controls before
claiming an interactive scene has been delivered.
