A stability release for the seed finder.

### Download
- **Windows:** `minecraftrust-seedmap-windows-x64.exe`. If SmartScreen warns you, choose *More info → Run anyway*.
- **Linux:** `minecraftrust-seedmap-linux-x64`. Run `chmod +x` on it first.

### Fixed
- **Seed finder crashes.** An internal error on one seed used to close the whole app. Now that seed is skipped, the search carries on, and the finder shows how many seeds were skipped. The same protection covers the map's background workers.
- **"Not responding" during searches.** Search and map threads run at low priority, and the finder leaves one CPU core free by default, so the window stays responsive.
- **Stop now reacts within a second**, even in the middle of a heavy seed.
- **"Close stronghold" never found anything:** strongholds are never closer than about 1,400 blocks, and the preset asked for 900. It now uses 1,600.

### Faster
- **Stronghold searches are about 4,000× faster.** Only the first ring is checked (it always holds the nearest strongholds), and seeds without one in range are rejected before any world is generated.
- Very dense structures (buried treasure, mineshafts, nether fossils) are searched up to 1,500 blocks, so one seed can no longer take minutes.

### If something still goes wrong
Errors are written to `crash.log` next to the app's settings (`%APPDATA%\minecraftrust-seedmap\data\crash.log` on Windows), with function names in the backtrace. Please attach that file to an issue.
