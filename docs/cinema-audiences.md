# Cinema Kids and Cinema Parents

The first household deployment uses the existing independent app flavors:
`com.beb.plxnative.debug` for Parents and `com.beb.plxnative.nightly` for Kids.
The launcher titles are Cinema Parents and Cinema Kids, with distinct generated icons; the native executable, PMS,
transport and rendering stack are shared. Separate app IDs retain separate state
and local-server configuration. The ordinary stable/debug account flow is unchanged.

Kids requires both its private local-server configuration (see
[local server mode](local-server.md)) and a private audience snapshot at:
`/media/internal/.com.beb.plxnative.nightly-audience.json`.

```json
{"version":1,"audience":"kids","machineId":"PMS_MACHINE_IDENTIFIER","ratingKeys":["10","11"],"sectionKeys":["1"],"partIds":["20","21"],"streamIds":[]}
```

These IDs are examples, not household IDs. The snapshot's server identity must
match the local-server configuration. Each item/section/part/stream ID is a numeric
string. A missing, oversized, invalid or mismatched snapshot fails closed. Kids
cannot fall back to Plex account login. Parents remains unrestricted.

The private source is an agent-authored Markdown media catalog on the Mini, using
YAML frontmatter for audience tags and exact PMS identities plus prose describing
the editorial choice. Only explicitly curated Kids entries enter the snapshot;
new/unknown files do not. This initial curation permits Bambi (1942) and Toy Story 5,
and excludes Obsession and The Mentalist. The exporter resolves current PMS IDs
and part/stream IDs from actual metadata, then uses existing authenticated SSH to
install the snapshot. No title guessing, additional server or VPN is involved.

A shared policy in the Plex Client layer filters every parsed container, including
browse, search, hubs, detail and nested extras. Only approved metadata survives;
unsupported directories and empty search/hub groups are removed. The URL boundary
also rejects unknown metadata, sections, parts, streams and transcode targets,
including encoded targets. Direct-play URLs and already-built artwork requests
share the same policy. This keeps a stale detail/play request from bypassing the
visible list filter. The snapshot is loaded once per process; restart Kids after
an administrative catalog change.

This is a media audience inside Cinema, not a PIN lock on the television's other
apps or the Parents launcher. Launcher appearance and actual playback require TV
validation; host tests establish filtering and request policy only.
