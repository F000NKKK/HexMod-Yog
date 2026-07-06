# HexMod-Yog

An unofficial, work-in-progress **Rust port of [Hex Casting](https://github.com/FallingColors/HexMod)**
— running on [Yog Mod Loader](https://github.com/F000NKKK/Yog-Mod-Loader) instead
of the JVM host API. It reimplements the hex-pattern casting grid, the
casting VM, and a growing subset of the original's actions from scratch in
Rust.

**This is not the official Hex Casting mod and is not affiliated with, endorsed
by, or supported by its original authors or the FallingColors organization.**
It's a fan port built to demonstrate and stress-test the Yog Mod Loader. If
you want the real, feature-complete, actively maintained mod, get it from
[Modrinth](https://modrinth.com/mod/hex-casting) or
[CurseForge](https://www.curseforge.com/minecraft/mc-mods/hexcasting).

## Credits

Hex Casting was created by **gamma-delta** and is maintained by the
**[FallingColors](https://github.com/FallingColors)** organization and its many
contributors — see the
[original repository](https://github.com/FallingColors/HexMod) and its
[contributors page](https://github.com/FallingColors/HexMod/graphs/contributors)
for the full list. All hex pattern names, spell design, and item/texture
assets bundled here originate from that project and are used under its MIT
license (see [LICENSE](LICENSE) — the original copyright notice is preserved
in full, as required).

Yog port: F000NKKK, Yog Team.

## Status

Early and incomplete. Currently implemented:

- The Oaken Staff — right-click opens the casting grid; sneak + right-click
  resets your stack (server-side, persists across reopening the grid)
- The casting grid: drag across the hex lattice to draw a pattern, released
  strokes are sent to the server and resolved by a small casting VM
- A handful of actions: Mind's Reflection, Compass' Purification, Alidade's
  Purification, Reveal, Jester's Gambit
- The Hexbook, with a couple of intro pages

Most of Hex Casting's action list, media/Amethyst economy, foci, and
particle/sound feedback are not ported yet.

## Requirements

Built against [Yog Mod Loader](https://github.com/F000NKKK/Yog-Mod-Loader).
Any loader/Minecraft combination Yog supports should work.

## Building

```bash
yog build
```

Produces `artifacts/HexMod-Yog.yog` — drop it into `<game dir>/yog-mods/`.
