# Flint development history

## 2026-09-26 — Linux native port and runtime extraction fix

- Ported Flint to build and run natively on Linux via a custom AppImage pipeline (Rust/Tauri backend, Fabric client mod unchanged). Added Linux system-Java detection (`JAVA_HOME`, `$PATH`, `/usr/lib/jvm`), Linux native-library selection (`natives-linux`) in the install/metadata pipeline, `:`-separated classpaths, and forced-off WebKitGTK compositing to avoid a GPU rendering issue on affected drivers.
- Fixed a critical bug in the managed Java runtime extractor: every symlink inside a downloaded archive was unconditionally rejected as unsafe. Real Eclipse Temurin Linux `.tar.gz` builds legitimately contain symlinks (mainly under `legal/`, where shared license text is symlinked across modules instead of duplicated), so every genuine first-time managed runtime download failed with `runtime_archive_symlink`. This was previously masked in local testing because an existing system Java satisfied the version requirement and the managed download path was never actually exercised. The extractor now resolves each symlink's target lexically relative to its own location in the archive and rejects only links that would escape the destination directory; hard links remain rejected outright. Verified against a real downloaded archive with the system Java runtime temporarily hidden to force the managed download path to run.
- [PLACEHOLDER — need the two Codex-identified fixes here]
- Confirmed AutoAuth is non-functional in this build; the feature was left incomplete during original development and needs to be revisited in a future version rather than shipped as working.
- Packaging: fixed the AppImage build script's WebKit helper-path patch, which asserted the compiled absolute path appeared exactly once in `libwebkit2gtk-4.1.so.0`. On this build environment's WebKitGTK package it appears twice (the standalone helper directory, and as a prefix of the longer injected-bundle path), which silently aborted the build under `set -eu` and left a stale, unpatched AppImage. The patch now handles every occurrence safely regardless of count.

- Fixed the managed Java runtime extractor rejecting every symlink in downloaded archives outright. Real Eclipse Temurin Linux `.tar.gz` builds legitimately contain symlinks (mainly under `legal/`, shared license text across modules), so every genuine first-time runtime download failed with `runtime_archive_symlink`. This was previously masked in testing because an existing system Java satisfied version checks and the managed download path was never exercised. The extractor now resolves each symlink's target relative to its own location in the archive and rejects only links that would escape the destination directory; hard links remain rejected.
- **Caveat on the keyring backend above:** storage now works on Linux, but AutoAuth end-to-end is confirmed non-functional in this build — the original developer confirmed the feature was left incomplete during development. This is a feature gap, not a Linux-specific limitation, and needs to be revisited in a future version rather than shipped as working.
- Fixed `packaging/build-appimage.sh`'s WebKit helper-path patch, which asserted the compiled absolute path appeared exactly once in `libwebkit2gtk-4.1.so.0`. On this build environment's WebKitGTK package it appears twice (the standalone helper directory, and as a prefix of the longer injected-bundle path), silently aborting the build under `set -eu` and leaving a stale, unpatched AppImage. Now handles every occurrence safely regardless of count.
- Update to bullet 5 above: the AppImage build *was* subsequently completed and verified via the GitHub Codespaces devcontainer — Java runtime download/extraction, native library loading, and normal launch all confirmed working end to end.

## 2026-09-21 — Deferred Flint Client module implementation

- Added draggable, normalized, per-instance HUD positioning through Edit HUD plus CPS, combo, and active-effects HUD modules. Reworked the in-game interface and HUD accents from lime to Flint orange/amber and added scrolling for larger module categories.
- Added opt-in Zoom (hold C), Freelook (hold Left Alt without rotating the player), local-only fixed daytime/clear-weather views, compact first-person hand transforms, bounded local projectile trails/player particles, a game-sound-driven audio visualizer, Toggle Sprint, and conservative Auto GG. Auto GG recognizes only three exact victory messages, waits one second, sends once per connection, and enforces a ten-second process-level rate limit.
- Rebuilt the remapped Fabric client and replaced the launcher-embedded JAR. Gradle clean build/check and the three focused launcher integration tests passed; built and embedded SHA-256 values matched. No Minecraft or module behavior was manually observed in this pass.
- Final native verification passed Rust formatting/check and 72/72 runnable tests with one interactive Credential Manager test ignored. The former handwritten HTTP fixture was replaced with a test-only HTTP server after repeated runs exposed intermittent Windows connection resets; its ten download tests then passed in three consecutive focused runs.

## 2026-09-21 — Flint Client foundation and AutoAuth reconnect correction

- Added a non-pausing in-game Flint Client screen opened by Right Shift, with HUD/Render/Player/Flint/Settings navigation, an atomic per-instance settings file, a rebindable menu key, and vanilla-key conflict warnings. Added working FPS, coordinates, ping, speed, memory, keystrokes, reach, armor, and client-only Fullbright modules; the broader requested module catalog remains deferred and is not represented as complete.
- Corrected AutoAuth replay scope. The launcher bridge previously remembered a rule for the lifetime of the Minecraft process, preventing a legitimate reconnect to the same server. Requests now carry a random validated connection UUID; repeats within that connection remain blocked while a later connection can make one new attempt. Safe lifecycle logging records dispatch/completion without commands, servers, usernames, or secrets.
- Closed local cosmetic image streams deterministically and rebuilt the remapped Fabric artifact embedded by the launcher. Gradle clean build/check and focused launcher integration tests passed. No Minecraft window, in-game menu, AutoAuth server exchange, skin, or cape was visually observed in this remote pass.
- Final automated verification passed: frontend production build, lint, and 12/12 tests; Rust formatting/check and 72/72 runnable tests with one interactive Credential Manager test ignored; and the Fabric Loom clean build including both client state/persistence checks. The embedded JAR SHA-256 matched the remapped Gradle output.
- The repository still declares product/client version `0.3.0`. Because no authoritative next version was supplied, no installer or release artifact was produced under an invented v0.4 version.

## 2026-09-19 — External-tester download hardening

- Investigated the external Minecraft 1.21.11 failure path. No failing tester log or artifact was present, so the exact remote-machine cause could not be proven; the developer cache could bypass fresh-download defects that a clean tester installation exercises.
- Replaced whole-response writes and a shared temporary filename with streamed SHA-1/size validation, unique sibling temporary files, durable flushes, and atomic Windows replacement. Invalid cached files are replaced, valid files are reused, failed temporary files are cleaned, and simultaneous requests for one destination are serialized.
- Limited retries to transient HTTP/network/filesystem conditions with bounded backoff, added connection/operation timeouts, stopped retrying permanent HTTP failures, and made malformed fresh Mojang manifest caches refresh instead of being trusted.
- Download errors now identify the artifact in the primary message. Safe log/UI detail includes the query-free URL, HTTP status, destination, checksum/size discrepancy, attempt count, and underlying network/filesystem operation; the frontend no longer discards structured technical detail.
- Added deterministic local-server regression coverage for clean/cache/corruption/interruption/checksum/HTTP/retry/concurrency/path/promotion behavior. Frontend tests passed 12/12, production build and lint passed, Rust format/check passed, and Rust tests passed 72/72 runnable with one pre-existing interactive Credential Manager test ignored. The optimized executable and x64 NSIS installer rebuilt from a clean isolated target; MSI bundling still fails at WiX `light.exe` on this host. An external clean-machine launch remains required to identify or close the original report.

## 2026-09-15 — Post-v0.3 client integration and Home milestone

- Repaired the AutoAuth workflow end to end in code. The previous client waited for a manually typed `/flintauth` pseudo-command, so ordinary gameplay could never trigger authentication automatically. Rules now expose Disabled/Login/Register modes, legacy enabled entries migrate to Login, every game-join resets state, and a 40-tick client-readiness gate makes exactly one privacy-safe bridge request. The bridge selects the stored mode, normalizes default-port server matching, blocks replay, fails closed on missing credentials, and never logs secrets, server addresses, or rendered commands.
- Identified the cosmetics failure from a real Minecraft log: the one-argument `TextureAssetInfo` constructor rewrote the registered dynamic texture identifier into `flint:textures/local_skin.png`, which Minecraft then tried and failed to load as a resource-pack asset. Flint Client now supplies the dynamic identifier as both the logical ID and render path, retains the original model when no local skin is active, and emits secret-free debug lifecycle messages.
- Added a data-driven image/optional-local-video Home hero. The current release intentionally keeps its existing static Flint artwork; future bundled WebM/MP4 entries receive poster/error fallback, muted playback, metadata-only preload, reduced-motion image fallback, and hidden/unfocused pause behavior. Home now gives real Flint update/compatibility context and direct Mods, Profiles, Import, and Settings actions while launch status appears only when relevant.
- Verification: frontend production build and lint passed; 9/9 frontend tests passed. Rust formatting/check passed; 61/61 runnable Rust tests passed with one interactive Credential Manager test ignored. The focused AutoAuth suite passed 9 runnable tests with one ignored, cosmetics passed 3/3, Flint Client launcher integration passed 3/3, and all 5 opt-in live metadata tests passed. Fabric Loom/Gradle 9.2.1 clean build, Java state test, checks, and `remapJar` passed; the remapped JAR contains all three required mixins.
- The optimized Tauri executable and x64 NSIS installer rebuilt successfully. The exact executable opened a responsive Windows `Flint` main window and accepted a normal close request; its tracked PID exited and no Flint/Java/Gradle/installer process remained. Native UI capture returned no controllable application surface, so minimum-window visual acceptance and all Minecraft/AutoAuth/skin/cape gameplay observations remain manual tests. Implementation complete; manual in-game verification required.

## 2026-09-11 — Flint v0.3 release-readiness milestone

- Made cross-version setup imports fail closed: source-version detection is explicit, unknown or different-version JARs are never copied directly, and SHA-1-identified Modrinth projects are re-resolved for the target profile.
- Added automatic 64-bit Temurin management based on Mojang's declared Java major. Provider SHA-256, archive size/path bounds, extracted runtime architecture/major, staging cleanup, atomic promotion, coexistence, and reuse are tested; system Java installations are not modified.
- Added an optional, profile-isolated Flint Client 0.3.0 Fabric mod with an explicit Minecraft 1.21.11 compatibility boundary and protocol-v1 configuration. The remapped artifact implements local-player-only Classic/Slim skins and capes without Mojang uploads or server-visible entitlement claims.
- Added opt-in, exact-server AutoAuth with configurable login/register templates. Passwords use Windows Credential Manager; instance configuration contains only an opaque reference. An ephemeral token-authenticated loopback bridge responds only to explicit `/flintauth login` or `/flintauth register` triggers and limits attempts without keyboard automation or plugin-detection claims.
- Refined Home around release artwork and Minecraft version context while preserving the dominant Play action, integrated profile selection, reduced-motion/focus behavior, and narrow-window scrolling.
- Final verification: frontend production build and lint passed; 9/9 frontend tests passed. Rust format/check passed; 57/57 runnable Rust tests passed with the interactive Credential Manager test ignored. Five opted-in live Mojang/Fabric/Modrinth/Adoptium metadata tests passed. Fabric Loom/Gradle compiled, ran the focused AutoAuth state test, and remapped Flint Client successfully. The final fix resets its completed state on every game-join event, including reconnecting to the same server. The optimized Tauri release and x64 NSIS package completed successfully after that fix.
- A controlled process-level smoke test kept the new `flint.exe` responsive through startup and then stopped its exact PID; no Flint, Java, Gradle, or installer process remained. A narrow browser-rendered Home pass found no horizontal clipping, but it could not exercise native Tauri IPC. The remote shell could not open Windows Credential Manager (error 1312), so its dummy live round-trip and all actual in-game v0.3 behavior remain manual checks rather than claimed successes.

## 2026-09-10 — Importer and cosmetics regression fixes

- Fixed same-version Fabric mods being left unresolved because the first importer accepted only literal Minecraft-version strings. The parser now evaluates common exact, comparator, bounded, tilde, caret, wildcard, hyphen, OR, and array predicates; checks environment/Fabric Loader requirements; and verifies required mods are present before direct copying.
- Added four explicit importer outcomes: Compatible, Can Reinstall, Needs Review, and Incompatible. Unknown files remain unselected. SHA-1-identifiable Modrinth files use the existing compatible-version/dependency installer, and Fabric API module JARs are consolidated to prevent duplicates.
- Fixed broken local skin/cape previews caused by CSP rejecting generated `blob:` sources. A tested byte-to-Blob adapter now avoids raw Windows paths, missing files fall back independently, and reset clears persisted selection. Unicode and spaces are covered by Rust persistence tests.
- Project-owner manual testing confirmed live Discord Rich Presence states and the `flint` artwork. The two-button payload remains covered by tests; cross-account button visibility remains a manual Discord test.
- Verification: frontend build/lint passed with 9 tests; Rust format/check passed with 34 tests; the focused live Modrinth SHA-1 resolution test passed for Sodium on Minecraft 1.21.11. An optimized Windows executable was rebuilt for physical retesting. No new Minecraft launch or native cosmetic/importer observation is claimed here.

## 2026-09-10 — Post-v0.2 beta product milestone

### Implemented

- Adopted the provided pixel-art flint image as the canonical brand source; generated optimized launcher/hero assets and Tauri Windows/platform icon variants.
- Reworked the launcher palette and Home hero around near-black/charcoal surfaces and a restrained warm Flint accent, with responsive 780px-class layout behavior and a data-driven artwork fallback.
- Added a read-only existing-setup scanner with explicit preview/selection for settings, servers, resource packs, shaders, configs, conservative Fabric mods, and opt-in worlds. Sources are never modified and credentials/logs/caches are outside the import surface.
- Added profile-local PNG skin/cape validation, preview, reset, Classic/Slim model choice, and cape enable state. These are honestly labelled local Flint Client cosmetics and do not alter official accounts.
- Added backward-compatible Flint Client lifecycle state persistence and UI status. The Fabric client itself, installation/update service, in-game badge/cosmetics, and module registry remain architecture-only.
- Added Discord buttons for the official GitHub latest-release destination and Flint Discord invite while preserving private, failure-isolated activity states.

### Verification and limitations

- Baseline before this milestone: frontend build/lint passed, 4 frontend tests passed, Rust format/check passed, and 21 Rust tests passed.
- Current automated verification: frontend build/lint passed, 6 frontend tests passed, Rust format/check passed, and 28 Rust tests passed, including live Fabric/Modrinth metadata checks.
- The optimized `flint.exe` and x64 NSIS installer rebuilt successfully. MSI regeneration reached WiX but failed because this remote Windows session could not access the Windows Installer service (`LGHT0217` / `0x643`); the older MSI was not treated as a new result.
- The new release executable was started once; it remained alive and created its WebView2 child tree, then Flint and all six descendants were closed by their recorded PID chain. Native UI capture was unavailable, so this is process-level smoke evidence only—not a visual workflow or Minecraft main-menu claim.
- The new interface was visually inspected in a narrow local browser viewport. Native importer/cosmetics dialogs, live Discord display, and Minecraft main-menu regression require later manual Windows verification; local cosmetics intentionally have no in-game effect yet.

Entries are append-only and record what was true when work was performed.

## 2026-09-07 — Milestone 1 foundation (0.1.0)

### Implemented

- Created the Tauri 2, React 19, TypeScript, Vite, and Rust project from an empty repository.
- Added local/offline profile validation and JSON persistence with stable IDs and timestamps.
- Added isolated per-profile game directories plus shared version, library, and asset caches.
- Implemented Java discovery through `JAVA_HOME`, `PATH`, and common Windows vendor directories, with executable/version validation.
- Implemented Mojang manifest/version metadata parsing, rule-aware libraries and arguments, SHA-1/size verification, bounded asset downloads, Windows native extraction, logging configuration, and child Java process monitoring.
- Added typed IPC calls and structured launcher status events for Ready, Preparing, Downloading, Launching, Running, Failed, and Finished states.
- Added project, architecture, security, contribution, roadmap, and repository-map documentation.

### Technical decisions

- Limited Milestone 1 to vanilla Minecraft Java Edition 26.2 and Java 25 after verifying Mojang's live manifest and version metadata on 2026-09-07. This keeps the foundation current without pretending to support every historical argument format and platform combination.
- Kept mutable instance game data separate while sharing immutable Mojang artifacts to balance isolation and disk usage.
- Derived offline UUIDs with the established `OfflinePlayer:<username>` UUID v3 convention. No session is forged and no online authentication is bypassed.
- Kept networking and process control in Rust; the frontend only sends typed user intent and renders status.

### Bugs and fixes

- Vite/esbuild could not load the local configuration inside the restricted verification sandbox. The same production build passed in the normal Windows filesystem context.
- The first Vitest run reported no tests. Added focused profile-validation tests rather than allowing empty test suites.
- Rust was absent, so Rust 1.98.1 and rustfmt were installed. Cargo then identified the missing Microsoft C++ linker prerequisite; installing Visual Studio 2022 Build Tools with the C++ workload resolved it.
- Tauri's Windows resource build required an application icon. Added a source Flint SVG and generated the standard desktop/mobile icon assets.
- The current React Hooks lint rules flagged synchronous form-state resets in an effect. Removing the unnecessary effect fixed the issue.
- A Tauri build retry initially could not find newly installed Cargo from npm's process environment. Adding Cargo's bin directory to that build process resolved it.
- The first runtime-data ignore rule for `minecraft/` also matched the Rust `src/minecraft` module. Anchoring runtime directories to the repository root kept launcher source trackable while retaining runtime protection.
- A registry check found newer maintained frontend majors. Flint moved to Vite 8.2.2, ESLint 10.10.0, and Vitest 5.0.0; TypeScript remains on 5.9.3 because the maintained TypeScript ESLint release does not yet support TypeScript 7.

### Verification and limitations

- `npm run build` passed using Vite 8.2.2 with 24 modules transformed.
- `npm run lint` passed using ESLint 10.10.0 with no findings.
- Vitest 5.0.0 passed 3 frontend profile-validation tests.
- `cargo fmt --check` passed, and `cargo check` passed without warnings.
- `cargo test` passed 5 Rust unit tests covering profile persistence/validation, Java parsing, metadata rules, and placeholder replacement.
- `tauri build --debug --no-bundle` succeeded and produced `src-tauri/target/debug/flint.exe`.
- Mojang's live manifest reported 26.2 as the current release, with Java major 25, 131 libraries, asset index 32, modern arguments, and a logging configuration.
- The native application compiles, but an actual Minecraft window/process launch is not verified. This machine's installed Java 21 cannot run the selected Java 25 game.
- No claim is made that Minecraft has launched successfully.

## 2026-09-08 — Milestone 2 foundation (0.2.0)

### Verified Milestone 1 baseline

- The project owner reported manual verification that the existing 26.2 offline profile reached the main menu, reused its cache, and connected to multiplayer before Milestone 2 began.
- Existing logs corroborate a Java 25 Minecraft 26.2 process start on 2026-09-07, a running state, and a normal exit after 3 minutes 20 seconds. This historical evidence is not presented as a new Milestone 2 GUI test.

### Implemented

- Replaced the 26.2-only profile restriction with Mojang's dynamic release catalog, a one-hour manifest cache with stale-cache fallback, and optional snapshot visibility.
- Made Java selection metadata-driven, added 64-bit runtime validation, preserved automatic selection, and added a manual Java executable setting.
- Expanded isolated profiles with Vanilla/Fabric loader selection, Fabric Loader version, preset, per-profile RAM, last-played time, edit/delete/duplicate operations, and destructive deletion confirmation in the UI.
- Added Fabric Meta loader discovery and launcher-profile merging, including checksum-verified Fabric Maven libraries and the Knot client entry point.
- Added Modrinth Fabric search, compatibility-filtered installation, required dependency resolution, instance-local managed-mod manifests, listing, replacement, and removal.
- Added live-resolved Vanilla, Performance (Sodium, Lithium, Entity Culling), Visuals (Iris plus declared required dependencies), and Custom preset flows with a pre-install preview.
- Added launcher settings for RAM, resolution, snapshot visibility, Java selection, Discord Rich Presence, and keep/minimize/hide behavior.
- Added privacy-safe Discord activity states and elapsed time. RPC failures are warnings and never abort launch. A real Discord client ID and registered Flint asset remain a release-time configuration requirement.
- Added bounded three-attempt download retries and real library/asset task counts.
- Configured release builds as Windows GUI applications, aligned version metadata at 0.2.0, and produced x64 MSI and NSIS installers.

### Bugs and fixes

- The old launcher selected Java 25 before reading the chosen version. Metadata resolution now occurs first and the declared Java major drives selection.
- Fabric launcher libraries do not all include hashes in the profile JSON. Flint retrieves the corresponding Maven SHA-1 sidecar before accepting those artifacts.
- Mod installation paths now reject traversal and non-JAR filenames before writing or removing files.
- Final review found snapshot visibility also admitted legacy alpha/beta entries and preset previews omitted required dependencies. The catalog now exposes only releases plus opt-in snapshots, and previews expand the same dependency graph used for installation.
- A sandboxed Java 25 execution reported access denied; the required outside-sandbox rerun and Flint detector test both passed, identifying the failure as sandbox policy rather than a runtime defect.

### Verification and limitations

- Frontend build and lint passed; Vitest passed 4 tests.
- Cargo formatting and check passed; Rust passed 15 tests. Two network-backed live tests passed against Fabric and Modrinth; the separately filtered Mojang fixture test was a conditional no-op because no fixture path was supplied.
- Live API validation found Fabric Loader 0.19.5 stable for 26.2 and compatible current Modrinth builds for all four preset projects.
- Java 21.0.12.1 and Java 25.0.4.1 both executed as 64-bit Temurin runtimes; Flint's focused detector test passed for each major.
- The final optimized Tauri rebuild passed from the clean Milestone 2 tree and regenerated the application, x64 MSI, and x64 NSIS installer.
- The first direct executable invocation ended before the follow-up process inspection and produced no Flint crash log or Windows application error. A diagnostic invocation of the same rebuilt executable then presented a responsive `Flint` main window for 46 seconds and exited normally with code 0. Native computer control was unavailable in this session, so no new vanilla or Fabric main-menu launch is claimed.

## 2026-09-09 — Day 3 release readiness (0.2.0)

### Implemented

- Routed every launcher-owned child command through one platform policy. Windows release builds now apply `CREATE_NO_WINDOW` to Java discovery, Java validation, and the Minecraft/Fabric Java child while preserving captured and file-redirected output.
- Deduplicated Java aliases using their resolved `java.home` identity, eliminating repeated entries without merging genuinely separate installations.
- Completed Discord activity lifecycle handling for browsing, preparing, downloading, launching, playing, disabling, and returning to Flint. Connection/configuration failures are logged once per enabled session and never propagate into launch.
- Replaced the scrolling developer-dashboard layout with consumer Home, Profiles, Mods, and Settings views. Play and player identity lead the Home screen; installed/discover mods are separated; settings are grouped into Minecraft, Launcher, and Advanced sections.
- Added responsive minimum-window behavior, visible keyboard focus, reduced-motion handling, explicit loading/disabled/error states, and clearer destructive actions.

### Verification and limitations

- The process policy, Java identity, privacy-safe activity labels, and non-blocking unconfigured-presence behavior have focused Rust tests.
- Frontend production build, lint, and all 4 frontend tests passed after the redesign. Visual browser QA passed at a standard desktop viewport and 780×620 without horizontal clipping.
- Rust formatting/check passed and all 20 Rust tests passed. Live verification passed for the cached official Mojang 26.2 metadata plus current Fabric and Modrinth compatibility; focused Java 21 and Java 25 detector runs also passed.
- The optimized Tauri release build passed and regenerated the x64 MSI and NSIS bundles. The exact release `flint.exe` opened a responsive Flint window and accepted a normal close request; startup left no helper or Java child running.
- No Discord Application ID exists in the repository or environment, so live Discord display is not verified. A public numeric Application ID and registered `flint` image asset are still required.
- The project owner supplied the current manual Minecraft/Fabric/Modrinth baseline. Native Windows UI capture was unavailable, so no new main-menu or visible terminal-suppression observation is claimed; both remain manual release-candidate checks.

### Discord application configuration

- Configured public Discord Application ID `1547183366091051019` directly in the Rich Presence adapter, removing the `FLINT_DISCORD_CLIENT_ID` requirement for normal builds and users.
- Retained the `flint` art asset name, Join Discord button, privacy-safe lifecycle states, Settings toggle, and non-blocking connection behavior. No bot token, client secret, public key, or credential was added.
- Rust formatting/check and all 21 Rust tests passed, including the exact-ID regression test. The optimized Windows release and both x64 installer formats rebuilt successfully; live Discord display still requires observation with Discord running and the Developer Portal asset available.

