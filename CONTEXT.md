# Streem

A Stremio-like media centre for family and friends: it browses catalogs and plays streams supplied by addons.

## Language

**Account**:
The single login that owns the shared settings and a set of profiles. Everyone who signs in to it is trusted equally.
_Avoid_: User, parent account, main account

**Profile**:
One viewer under an account, with their own watch history, library and settings that differ from the account's.
_Avoid_: User, sub-account

**Override**:
A profile's own value for a setting, used for that profile in place of the account's value.
_Avoid_: Custom config, profile config

**Debrid Service**:
A paid third-party service that turns a torrent into a direct stream.
_Avoid_: Premium link service, cache service

**Download**:
A title saved to a device so it can be played without an internet connection.
_Avoid_: Offline copy, cached title

**Watch Party**:
A shared viewing of one title by two or more profiles of the same account, where a play, pause or seek by anyone applies to everyone.
_Avoid_: Room, session, watch together, sync play

**Addon**:
A remote service that speaks the Stremio addon protocol and supplies catalogs, metadata, streams or subtitles. It is the only way to extend the app.
_Avoid_: Plugin, extension, module
