# Native household launcher

The `family` package is an optional launcher alongside the Cinema and book applications. It starts its own native screen without entering the Plex account, profile or first-run flows. It uses the existing media engine for an independent, silent background video; launching an application uses the platform application-manager adapter.

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

The top band displays local system time and the existing localized calendar date. A presentation adapter samples wall time separately from the application's frame/replay clock and delivers status to the screen. The controls fade after eight seconds of inactivity and return on interaction.

The existing resident may mirror its `home.status` observation to `/media/internal/pinpin-family/home-status.json`. A bounded asynchronous worker reads at most 4096 bytes every 25 seconds; no status network request or file read runs in the frame path. The consumer requires schema 1 and integer `observedAtUnix` seconds, rejects observations more than 120 seconds old or more than 60 seconds in the future, and displays unknown for unavailable/disconnected/invalid data. A valid connected Wi-Fi observation displays its measured signal when available; Ethernet is identified separately. The SSID is not displayed. Weather remains absent until an actual location and observation are configured.

Background declarations must describe the actual video: supported H.264/HEVC codec, source dimensions and source frame rate. The initial H.264 bound is 30 fps; HEVC is accepted up to 60 fps. No higher-rate source is relabelled to satisfy these bounds. This is video-only playback, with no invented audio stream, Plex watch identity, queue or timeline reporting. A looping VOD restarts at its end through its own owner; it does not spend the trailer preview seek budget.

Configuration, service URLs and any capabilities remain private. Installing this package does not replace Cinema or book package identities, change the factory launcher, or redirect the remote's Home button. Those integrations require their own reversible deployment steps.
