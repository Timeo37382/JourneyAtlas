# JourneyAtlas

A map mod for [Horizon](https://playhorizon.dev) (Minecraft: Java Edition 26.3 in Rust): a minimap, a
fullscreen map, waypoints with beacons and an entity radar. Native mod, client side.

JourneyAtlas is inspired by one of the best-known minimap mods for Minecraft: Java Edition. It is an
independent project written from scratch in Rust for Horizon: its code, artwork and translations are its own,
and it is not affiliated with or endorsed by the authors of that mod.

## Features

- **Minimap** in a corner or at the top centre, square or round, two presets, zoom, opacity, rotation with
  the player, compass, coordinates, biome, time of day and weather.
- **Fullscreen map** (`J`): drag to move, wheel to zoom, floating toolbars:
  - map views: day, night, topographic (contour lines), caves;
  - filters: villagers, monsters, animals, pets, players; chunk / region grid; keyboard shortcut legend;
  - tools: waypoint manager, options, help, recalculate the map, delete the map of a dimension, save the
    map as a PNG image, pause / resume mapping.
- **Waypoints** (`B`, or right-click on the fullscreen map): rename, recolour, hide, delete, teleport
  there (operators), shown on the maps and in the world as a beacon with name and distance. The last
  death is marked automatically and removed once you get back to it.
- **Entity radar** with mob heads taken from each mob's own model and texture, or plain dots.
- Maps are saved per world and per dimension in `<game folder>/journeyatlas/`.
- **Tutorial** the first time you open each world (it can be replayed from the options).
- **Languages**: follows the game's language — English, French, German, Spanish, Italian, Portuguese,
  Russian, Ukrainian, Polish, Dutch, Turkish, Chinese, Japanese; English for any other.

## Keys

| Key | Action |
| --- | --- |
| `J` | Fullscreen map |
| `B` | Create a waypoint |
| `=` / `-` | Minimap zoom |
| `[` | Change map type |
| `\` | Switch minimap preset |
| `]` | Show / hide the minimap |

All keys can be changed in the game's controls.

## Building

```sh
cargo horizon-mod check
horizon-launcher native-build . --instance "<instance>" --trust-native-code
```

The pure parts (tiles, shading, layout, screens, translations) are tested with `cargo test`.
`src/bridge.rs` and `src/mixins/` are compiled inside the game by the launcher.

## Licence

All rights reserved, see [LICENSE](LICENSE). You may download and use JourneyAtlas for your own personal,
non-commercial play. Modifying it, redistributing it (modpacks and re-uploads included), selling it or reusing
its code, artwork or translations is not allowed without the author's written permission. The source is public
so it can be read and reviewed; the Horizon mod registry is allowed to review, host and distribute the
unmodified releases. All artwork in `assets/` is original to this project.
