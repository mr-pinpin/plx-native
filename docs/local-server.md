# PinPin account-free local server mode

This fork can open an explicitly configured household Plex Media Server without a
Plex account, QR code, invented token or cloud server discovery. The server must
already permit unauthenticated access from the TV's trusted LAN address. This
client setting does not change the server's access policy.

For the stable `com.beb.plxnative` app, place this non-secret configuration outside
Git at `/media/internal/.com.beb.plxnative-local-server.json`:

```json
{"version":1,"origin":"http://192.168.50.12:32400","machineId":"PMS_MACHINE_IDENTIFIER","name":"Home media"}
```

The example uses a numeric private LAN address. The endpoint may use
HTTP or HTTPS; ordinary certificate verification applies to HTTPS. Only private IP
addresses, loopback or `.local` names are accepted. The file must be readable by
the native app's user, for example owner-readable under that UID. App flavours use
their own app ID in the filename. No household address is compiled into the app.
The optional `machineId` is the PMS `machineIdentifier` reported by its local
`/identity` endpoint. It enables Plex GDM discovery on the LAN: a bounded shared
Session worker sends server multicast, broadcast and known-address unicast probes.
Only replies identifying that exact server, from a private LAN address, are
accepted. Address changes retarget the existing server instead of duplicating it.
This requires no mDNS support, account service, token or router configuration.

With `machineId`, `origin` may be omitted for discovery-only startup. Providing a
numeric private `origin` also gives an immediate first-play fallback while the
background scan runs. A `.local` origin is accepted but depends on the TV's
resolver; it is not the automatic-discovery mechanism. GDM is unauthenticated LAN
advertising, not cryptographic proof of the sender. Use it only on a trusted LAN.

Presence selects an explicit `LocalPms` session authority. It registers one
anonymous local PMS, renders Home, and uses the existing library, artwork and
playback APIs. Requests carry the install's client identifier, but no account
token, user token or empty `X-Plex-Token` parameter. The tokenless capability is
explicit per Client and is revoked along with that Client. Plex account discovery,
QR creation and Home-profile switching do not run for this authority.

An absent file leaves the upstream account flow unchanged. An unreadable or
invalid file reports a local configuration error instead of starting a cloud
login. Existing cached account credentials are not used or erased by local mode;
the running cache is revoked so older synchronous readers cannot recover them.
Removing the configuration and restarting returns to the normal account flow.

Local Sign out disconnects the running local server, without starting Plex login
or deleting a different cached account. The administrative file remains a boot
configuration: restarting reconnects to it. Remove the file to disable the mode.
Account-mode sign-out keeps its original behaviour.

The extension adds no new decoder, transport or rendering stack. It still needs
an ARM release build and an actual TV check of anonymous library access and
playback. Host tests cover configuration constraints, the tokenless request
boundary, GDM identity filtering and address changes, local boot and local disconnect; they do not prove the TV's resolver,
filesystem permissions, media plane or LAN service availability.


## Device evidence for this fork

On 2026-10-26, the television's native control handshake reported webOS 10.3.2 and system information identified OLED55A3RLA with a DVB receiver. The installed upstream 0.9.0 application opened its account-link screen, as observed by the owner. These observations establish the device and original native application; they do not establish playback of this fork. A direct request from the television reached the Mini's unclaimed server, and a local broadcast discovery probe identified that same server. Private device addresses and identifiers are kept outside this repository.
