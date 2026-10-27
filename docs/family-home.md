# Native household launcher

The `family` package is an optional launcher alongside the Cinema and book applications. It starts its own native screen without entering the Plex account, profile or first-run flows. It uses the existing media engine for an independent, silent background video; launching an application uses the platform application-manager adapter.

An optional private `cover` path names a local PNG under `/media/internal/`, bounded to 1 MiB and 512×512 pixels. A landscape image such as 512×288 provides a soft forest background while video starts. It is decoded at configuration load and uploaded once through the existing image budget, with no frame-path file or network reads. The image stays behind the usable dock until the video plane is bound and this playback session has actually presented a picture; a load-completed event or a timeout does not remove it. Stop/reload or an offline stream shows the local image again. The private image is not included in the public package.

A private, versioned configuration lives at `/media/internal/.com.beb.plxnative.family-home.json`. It accepts one to five distinct application IDs. A household can choose four dock entries; there are no mandatory Apps or Edit buttons. An example using documentation-only addresses and identifiers is:

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
    {"id": "org.example.parents", "title": "Parents"}
  ]
}
```

The optional artwork must be a local PNG under `/media/internal/`, at most one MiB and 512 by 512 pixels. Invalid artwork retains the title button. The application reads and decodes artwork during configuration bootstrap; the shared UI resource uploads textures during its existing prepare budget. Icons grow with the shared focus spring. The dock's translucent theme tint does not blur the separate hardware video plane.

The top band displays local system time and the existing localized calendar date. A presentation adapter samples wall time separately from the application's frame/replay clock and delivers status to the screen. The dock and top band remain visible at all times; inactivity never turns the launcher into a video-only view. Resume and ordinary Home relaunch show the dock immediately, independently of video buffering; a parked frame gap does not consume the new idle interval. Root Back stays in the launcher and returns an active scene to its default.

The existing resident may mirror its `home.status` observation to `/media/internal/pinpin-family/home-status.json`. A bounded asynchronous worker reads at most 4096 bytes every 25 seconds; no status network request or file read runs in the frame path. The consumer requires schema 1 and integer `observedAtUnix` seconds, rejects observations more than 120 seconds old or more than 60 seconds in the future, and displays unknown for unavailable/disconnected/invalid data. A valid connected Wi-Fi observation displays its measured signal when available; Ethernet is identified separately. The SSID is not displayed. Weather remains absent until an actual location and observation are configured.

Background declarations must describe the actual video: supported H.264/HEVC codec, source dimensions and source frame rate. The initial H.264 bound is 30 fps; HEVC is accepted up to 60 fps. No higher-rate source is relabelled to satisfy these bounds. This is video-only playback, with no invented audio stream, Plex watch identity, queue or timeline reporting. A looping VOD restarts at its end through its own owner; it does not spend the trailer preview seek budget.

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
