# Windows opaque-window text patch

Base: published `i-slint-renderer-skia` 1.18.1, upstream revision
`372cf0ee5577c3dfec309a45e7b778ba4e81b734`.
Official crate SHA256: `dc9c1cf02385f1d26fc1f423544e52390a8e5c2d89420a8042aa94b4fe109d1f`.
Upstream licenses are retained in `LICENSES/`.

Changes are limited to `windows_text.rs`, `lib.rs`, `opengl_surface.rs`,
`itemrenderer.rs`, and the existing Windows dependency's feature list in
`Cargo.toml`. The package-local lockfile is omitted; the application workspace
lockfile pins dependencies.
The upstream revision's root `rustfmt.toml` is included to preserve its formatting.

The Windows OpenGL surface uses RGB/BGR pixel geometry only when the Slint
window background is a fully opaque solid color and Windows ClearType is on.
Each rendered frame checks these conditions, rebuilding surface properties when
needed. Renderer state reads base geometry through the output-pointer `getProps`
API and disables LCD inside opacity layers, restoring it on exit. Direct C++
aggregate-return `getTopProps` is not used: its Windows x64 calling convention
caused the delivered .5 EXE to crash at RVA `0x9dbb80` when opening settings.
Transparent cached layers explicitly use unknown geometry; ordinary opacity
save-layers retain Skia's grayscale behavior. There is no PreserveLCDText flag,
forced scale factor, font-family override, or change to the native subtitle window.

Settings/API content backgrounds are opaque. Hover/press and page animations
are preserved. Compile/tests and real OpenGL visual acceptance are distinct:
the user must confirm text at 150% scale, after animations, in both themes and
after resizing/reopening. This patch does not claim pixel equivalence to GDI menus.

CI explicitly runs the renderer's Windows native raster-canvas regression test
in addition to the virtual UI tests. It exercises the actual native property
query, canvas integrity and transparent-layer eligibility without opening a HWND.

Reassess/remove this patch when upgrading Slint; do not apply it to a new version
without checking the renderer's surface and layer behavior.
