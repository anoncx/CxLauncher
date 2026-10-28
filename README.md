# CxLauncher

A fast, native Minecraft launcher for Windows and Linux, built for the CxVisuals client. One button installs and
starts Minecraft with Fabric, Fabric API and the newest CxVisuals, and every profile can just as well run any other
Minecraft release, vanilla or with Fabric, or a Modrinth modpack. All in the same calm Nothing and Apple design
language as CxUML and the CxVisuals settings window.

- **Native Rust**: one binary rendered with [Slint](https://slint.dev) and Skia, no web view and no runtime to
  install. At rest it does not redraw at all, so it uses no CPU while it waits
- **Made for low end PCs**: Skia falls back to software rendering when the GPU driver is missing, downloads
  stream straight to disk while they are hashed, the console only keeps the rows you can see, and Effects can be
  turned off for older hardware. The memory and download defaults follow the PC it runs on
- **CxVisuals built in, never forced**: a switch per profile. When on, the client and Fabric API are kept up to
  date and loaded with `-Dfabric.addMods`, so your mods folder stays yours. A copy you drop in yourself wins
- **Profiles and modpacks**: any Minecraft release, Vanilla or Fabric, Modrinth modpacks from the built in browser
  or a `.mrpack` file
- **Everything themed**: light, dark or system, seven accents, reduce motion
- **Effects that match the client**: a dot matrix field that lights up around the cursor like a Glyph, a parallax
  hero in the spirit of Spatial UI, your skin as a pixel figure, a dot progress bar, rolling numbers, spring
  animations everywhere

## Playing

Press **Play**. The first start downloads Minecraft, the assets, Fabric, the Java Minecraft asks for, Fabric API
and CxVisuals, all in one parallel batch with one progress bar. After that a start only checks sizes, and works
offline from the saved metadata. **Settings → Repair** hashes every file and replaces anything damaged.

While the game runs the button shows the play time, the Console shows the game output (filter warnings or errors,
copy, open the logs folder), and Stop ends the game. If Minecraft closes with an error the launcher opens the
Console on its own. When the game window appears the launcher minimizes (or stays, see Settings) and comes back
when you quit.

## Accounts

Sign in with Microsoft. The sheet shows a code for microsoft.com/link; open the page on any device, type the code,
done. If code sign-in is not possible the launcher switches to browser sign-in: it opens the Microsoft page, and
when you land on the empty page afterwards you copy its address, which the launcher picks up from the clipboard
(or paste it into the field).

Sign-in uses the public client of the official Minecraft launcher with the Xbox Live and Minecraft services, the
same chain every launcher uses. Several accounts can be added and switched from the account menu. Tokens are
refreshed before each start and stored in `accounts.json` readable by your user only.

## Profiles and modpacks

A profile is its own game folder with its own Minecraft version, loader, mods, worlds, resource packs and options.
The **Profiles** page shows them as cards; press Play on any card, or switch from the chip above the title on the
Play page. Each profile picks:

- **Minecraft**: Automatic (the CxVisuals version when CxVisuals is on, otherwise the latest release) or any release
- **Loader**: Fabric or Vanilla. Old versions work too: legacy natives and the old asset layouts are handled
- **CxVisuals**: on or off. Builds exist for several Minecraft versions (26.3 and 26.2 today) and the launcher
  downloads the one that matches the profile; the editor says so when no build fits the version
- a server to join on launch and an optional pinned Fabric Loader version

**Modpacks** on the Profiles page searches Modrinth for Fabric modpacks; **Get** downloads one into a new profile with
its Minecraft version, Fabric Loader version, mods and config. **Import** does the same for a `.mrpack` file. Forge,
NeoForge and Quilt packs are recognised and politely declined.

**Import from other launchers** finds instances of the official launcher, Prism, CurseForge and Modrinth, shows what
each contains (Minecraft version, loader, mods, worlds) and copies the ones you tick into new profiles. The originals
are never touched, and an instance you already imported is not offered again. The launcher offers this once on first
start; the button on the Profiles page runs it again.

## Mods

Mods belong to the selected profile. Vanilla profiles have none and offer a one click switch to Fabric.
**Installed** lists the mods in the profile with their icons; switch them off without deleting them (the jar is
renamed to `.jar.disabled`) or remove them. **Discover** searches Modrinth for Fabric mods for the current
Minecraft version and installs them with their required dependencies. **Boost FPS** installs Sodium, Lithium,
FerriteCore, EntityCulling and ImmediatelyFast in one go and skips any that are not out for the version yet.

## Java and performance

The managed Java is the exact runtime Minecraft names in its version file, downloaded from Mojang. Any installed
Java 25 or newer can be picked instead. Memory defaults to what fits the PC (2 GB on 4 GB machines, up to 6 GB on
large ones). The garbage collector is **Balanced** (G1, tuned like the official launcher, plus compact object
headers on Java 25) or **Smooth** (generational ZGC). Extra Java arguments go under Settings.

## CxVisuals releases

The launcher reads the release manifest from `https://visuals.cxiy.dev/v2/client` and the jar next to it, checks
the size and SHA-1, and keeps the last good manifest for offline starts. Publish a new build with
`cloud/publish-client.sh` in the CxVisuals repository. To try a local build, pick **Settings → CxVisuals → Local
jar** and choose the jar from `build/libs`.

## Files

Everything lives in one folder: `%APPDATA%\CxLauncher` on Windows, `~/.local/share/CxLauncher` on Linux, or the
folder in `CXLAUNCHER_HOME`.

```
settings.json  accounts.json  profiles.json
profiles/<id>/     game folder of each profile (mods, saves, logs, options.txt)
versions/  libraries/  assets/  runtime/  natives/
managed/           CxVisuals and Fabric API, kept up to date by the launcher
meta/              saved metadata for offline starts
cache/             skins and mod icons
```

## Download

Every push to the default branch builds Windows and Linux and publishes a release automatically
(`v0.1.<build number>`), see [Releases](https://github.com/anoncx/CxLauncher/releases).

| Platform | Files |
| --- | --- |
| Windows | `CxLauncher_*_x64-setup.exe` installer, or `CxLauncher-*-windows-x64-portable.exe` |
| Linux | `.AppImage`, `.deb`, or `CxLauncher-*-linux-x64.tar.gz` |

## Build

```bash
cargo run --release
cargo test
cargo install cargo-packager
cargo packager --release
```

Linux needs `libfontconfig1-dev libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev` to build and
`libxkbcommon-x11-0` at runtime. Windows needs the MSVC toolchain.

`cargo test` includes two end to end tests against a local server laid out like the real services: one installs
and starts a fake game through the whole pipeline (managed Java, Fabric profile, assets, CxVisuals, Fabric API,
repair, a vanilla profile, a modpack, offline start), the other walks the full Microsoft, Xbox Live and Minecraft
sign-in. Debug builds can be
pointed at such a server with `CXLAUNCHER_LOCAL=http://127.0.0.1:<port>`.

## Layout

```
ui/                  Slint UI
  theme.slint        colors (light / dark / system), accents, effects, motion
  components.slint   icons, buttons, toggles, segmented control, inputs, menus, slider, dots, rolling numbers, sheets
  play.slint         dot field, hero, Play button, profile menu, cards
  profiles.slint     profile library and modpack browser
  rows.slint         Modrinth result rows and search pane shared by mods and modpacks
  mods.slint  console.slint  settings.slint  sheets.slint
  app.slint          window, navigation, account menu, toasts
src/
  net.rs  download.rs     HTTP and the parallel, hash checked download queue
  mc/                     version files, rules, arguments, libraries, assets, Java runtimes, Fabric, Modrinth,
                          modpacks, mod jars
  client.rs               CxVisuals release manifest or local jar
  install.rs  game.rs     install plan, launch arguments, the game process and its output
  auth.rs  skin.rs        Microsoft sign-in, skin head and figure
  store.rs  sys.rs        settings, profiles, machine info
  app/                    UI binding: play, accounts, profiles and modpacks, Modrinth search, mods, console, settings
```

Fonts: Geist, Geist Mono and Doto (SIL OFL).
