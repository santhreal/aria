# `com.example.data-pack` (`examples/data-plugin`)

Data-tier plugin package (`Tier::Data`) containing no guest code (`plugin.wasm`
or `lib/`). Mounting it through `DataPlugin` registers:

- Effect `com.example.data-pack.warm-tint` (`shaders/warm-tint.wgsl`, parameter
  `gain`).
- Generator `com.example.data-pack.uv-gradient` (`shaders/uv-gradient.wgsl`).
- Transition `com.example.data-pack.cross-dissolve`
  (`shaders/cross-dissolve.wgsl`).
- Preset `Soft Vignette` (`presets/soft-vignette.ariapreset`).

## Validate, pack, and install

```text
aria plugin check examples/data-plugin
aria plugin pack examples/data-plugin --no-build
```

Install the generated `com.example.data-pack-0.1.0.ariaplugin` archive from
**File > Install Plugin...**, **Settings > Plugins > Install from File...**, or
by dropping the archive onto the editor window.
