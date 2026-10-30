# CxLauncher

A fast, native Minecraft launcher for Windows and Linux. One button installs and starts Minecraft, every profile
can run any Minecraft release with Fabric, Quilt, Forge, NeoForge or vanilla, and Modrinth mods, modpacks, resource
packs and shaders are one click away. CxVisuals, our own client, is available as an optional suggestion and is never
forced on you. All in the same calm Nothing and Apple design language as CxUML and the CxVisuals settings window.

- **Native Rust**: one binary rendered with [Slint](https://slint.dev) and Skia, no web view and no runtime to
  install. At rest it does not redraw at all, so it uses no CPU while it waits
- **Made for low end PCs**: Skia falls back to software rendering when the GPU driver is missing, downloads
  stream straight to disk while they are hashed, the console only keeps the rows you can see, and Effects can be
  turned off for older hardware. The memory and download defaults follow the PC it runs on
- **No account, no analytics**: you sign in with Microsoft or play offline, nothing is tracked, and everything that
  leaves your PC is listed under [Privacy](#privacy-and-data)
- **CxVisuals is optional**: new profiles start without it. A profile can add it with one click, which keeps the
  client and Fabric API up to date and loads them with `-Dfabric.addMods`, so your mods folder stays yours. The hint
  can be dismissed or switched off in Settings
- **Profiles and modpacks**: any Minecraft release, five loaders, Modrinth modpacks from the built in browser or a
  `.mrpack` file, groups, per profile Java, memory and launch settings
- **Make it yours**: light, dark or system, seven accents, your own background colour (surfaces and text adapt to
  it), four corner styles from sharp to round, reduce motion
- **Effects that match the client**: a dot matrix field that lights up around the cursor like a Glyph, a parallax
  hero in the spirit of Spatial UI, your skin as a pixel figure, a dot progress bar, rolling numbers, spring
  animations everywhere

## Playing

Press **Play**. The first start downloads Minecraft, the assets, the loader and the Java Minecraft asks for, all in
one parallel batch with one progress bar. After that a start only checks sizes, and works offline from the saved
metadata. **Settings → Repair** hashes every file and replaces anything damaged. The accessibility screen Minecraft
shows on its first start is skipped for new profiles.

While the game runs the button shows the play time, the Console shows the game output (filter warnings or errors,
copy, share, open the logs folder), and Stop ends the game. If Minecraft closes with an error the launcher opens the
Console on its own and, when it recognises the cause (too little memory, a mod for another version, a missing
dependency, an old Java, a graphics driver problem), says so in plain words. When the game window appears the
launcher minimizes (or stays, see Settings) and comes back when you quit.

## Accounts

Sign in with Microsoft. The sheet shows a code for microsoft.com/link; open the page on any device, type the code,
done. If code sign-in is not possible the launcher switches to browser sign-in: it opens the Microsoft page, and
when you land on the empty page afterwards you copy its address, which the launcher picks up from the clipboard
(or paste it into the field). Offline accounts need only a name.

Sign-in uses the public client of the official Minecraft launcher with the Xbox Live and Minecraft services, the
same chain every launcher uses. Several accounts can be added and switched from the account menu. Tokens are
refreshed before each start, and when you are offline a saved sign-in still starts the game. They are stored in
`accounts.json` readable by your user only.

## Profiles and modpacks

A profile is its own game folder with its own Minecraft version, loader, mods, worlds, resource packs and options.
The **Profiles** page shows them as cards; press Play on any card, or switch from the chip above the title on the
Play page. The editor has two tabs.

**General**: name, icon (one of six symbols or your own image, and a modpack from Modrinth keeps its own icon),
Minecraft version (Automatic or any release or snapshot), loader (Fabric, Quilt, Forge,
NeoForge or Vanilla), a pinned loader version, Java runtime, a server to join on launch, and the optional CxVisuals
switch. Builds of CxVisuals exist for several Minecraft versions and the launcher downloads the one that matches the
profile.

**Advanced**: all of these are per profile and blank means the Settings default.

- **Group** to sort profiles; a group filter appears on the Profiles page
- **Memory** and **extra Java arguments** that are added after the ones from Settings
- **Environment variables** (`KEY=VALUE` pairs) for the game process
- **Wrapper command** to start Java through another program, for example `gamemoderun`
- **Before launch** and **After exit** commands. Before launch has to finish, and a failure stops the start. Both
  get `INST_ID`, `INST_NAME`, `INST_DIR`, `INST_MC_DIR` and `INST_MC_VER`, and After exit also `INST_EXIT_CODE`.
  These run with your permissions, so the editor says to only use commands you trust. Imported profiles and
  modpacks never carry commands
- **Notes** that only you see

The editor also lists the profile's worlds. **Play** next to a world starts the game straight in that world, and
**Shortcut** puts a desktop shortcut for the profile on your desktop.

**Modpacks** on the Profiles page searches Modrinth for modpacks; **Get** downloads one into a new profile with
its Minecraft version, loader version, mods and config. **Import** does the same for a `.mrpack` file. A pack may
only download from the hosts the Modrinth pack format allows (Modrinth, GitHub, GitLab), otherwise nothing is
installed.

**Import from other launchers** finds instances of the official launcher, Prism, CurseForge and Modrinth, shows what
each contains (Minecraft version, loader, mods, worlds) and copies the ones you tick into new profiles. The originals
are never touched, and an instance you already imported is not offered again. The launcher offers this once on first
start; the button on the Profiles page runs it again.

## Mods

Mods belong to the selected profile. Vanilla profiles have none and offer a one click switch to Fabric.
**Installed** lists the mods in the profile with their icons; switch them off without deleting them (the jar is
renamed to `.jar.disabled`) or remove them. Removing a mod that another installed mod needs asks you to press remove
again. **Discover** searches Modrinth for mods, resource packs and shaders for the current Minecraft version and
loader, loads more results as you scroll, and installs mods with their required dependencies. **Boost FPS**
installs Sodium, Lithium, FerriteCore, EntityCulling and ImmediatelyFast in one go and skips any that are not out
for the version yet.

**Update** checks every mod you installed through the launcher and, when you press it, also the jars you added by
hand: their file hashes are sent to Modrinth to find out what they are, and the ones it knows are tracked from then
on. Updates stay on full releases unless you turn on beta and alpha updates in Settings, a mod is never replaced by
an older version, and disabled mods stay disabled.

## Java and performance

The managed Java is the exact runtime Minecraft names in its version file, downloaded from Mojang. Any installed
Java 25 or newer can be picked instead, per profile or in Settings. Memory defaults to what fits the PC (2 GB on 4 GB
machines, up to 6 GB on large ones). The garbage collector is **Balanced** (G1, tuned like the official launcher,
plus compact object headers on Java 25) or **Smooth** (generational ZGC). Extra Java arguments go under Settings.

## Command line

```
cxlauncher --launch "<profile name or id>" [--world "<world folder>"]
cxlauncher --import path/to/pack.mrpack
```

`--launch` opens the launcher and starts that profile, which is what the desktop shortcuts use. `--world` joins a
world in it directly.

## Console and sharing logs

The Console keeps the output of the last session. **Copy** puts it on your clipboard. **Share** uploads it to
[mclo.gs](https://mclo.gs), a public paste service, and copies the link, but only after you read the notice and press
Upload. Your sign-in token and your home folder path are removed from the text first.

## Updates

The launcher checks GitHub for a newer release when it starts and shows an Update button in Settings. Windows
installs run the installer, portable builds and AppImages replace themselves, and other installs open the release
page. Downloads are verified against the size and SHA-256 in the manifest that every release publishes
(`launcher.json`) and only come from github.com or visuals.cxiy.dev.

## CxVisuals releases

The launcher reads the release manifest from `https://visuals.cxiy.dev/v2/client` and the jar next to it, checks
the size and SHA-1, and keeps the last good manifest for offline starts. Publish a new build with
`cloud/publish-client.sh` in the CxVisuals repository. To try a local build, pick **Settings → CxVisuals → Local
jar** and choose the jar from `build/libs`.

## Privacy and data

There are no accounts and no analytics. This is everything the launcher contacts, and why:

| Who | What for | When |
| --- | --- | --- |
| Microsoft, Xbox Live, Mojang | sign-in, versions, game files, Java | when you sign in, play or repair |
| Modrinth | searching and installing content | when you browse or install |
| Modrinth (file hashes) | recognising mods you added by hand | only when you press Update |
| GitHub | checking for and downloading launcher updates | at start and when you press Update |
| `visuals.cxiy.dev` | CxVisuals release info and builds, and the players online count | at start, and builds only when a profile uses CxVisuals; the count can be turned off in Settings |
| mclo.gs | a log you chose to share | only after you press Upload |

Everything else stays on your PC. Anything that sends data from your PC is optional, behind a button, and says so
before it does.

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
  theme.slint        colors (light / dark / system / custom background), accents, corner styles, effects, motion
  components.slint   icons, buttons, toggles, segmented control, inputs, menus, slider, scrolling, sheets
  play.slint         dot field, hero, Play button, profile menu, cards
  profiles.slint     profile library and modpack browser
  rows.slint         Modrinth result rows and search pane shared by mods and modpacks
  mods.slint  console.slint  settings.slint  sheets.slint
  app.slint          window, navigation, account menu, toasts
src/
  net.rs  download.rs     HTTP and the parallel, hash checked download queue
  mc/                     version files, rules, arguments, libraries, assets, Java runtimes, loaders, Modrinth,
                          modpacks, mod jars
  client.rs               CxVisuals release manifest or local jar
  install.rs  game.rs     install plan, launch arguments, the game process and its output
  hooks.rs  cli.rs        profile commands and environment, command line arguments
  crash.rs  share.rs      crash cause hints, log redaction and upload
  shortcut.rs  update.rs  desktop shortcuts, launcher updates
  auth.rs  skin.rs        Microsoft sign-in, skin head and figure
  store.rs  sys.rs        settings, profiles, machine info
  app/                    UI binding: play, accounts, profiles and modpacks, Modrinth search, mods and their
                          updates, console, settings
```

Fonts: Geist, Geist Mono and Doto (SIL OFL).
