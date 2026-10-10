<p align="center"><img src="packaging/icons/256.png" width="128" alt="Photo Studio icon"></p>

<h1 align="center">Photo Studio</h1>

<p align="center">A fast, native, layer-based photo editor for macOS, Windows and Linux.<br>
One self-contained binary, no runtime, works offline.</p>

<p align="center"><a href="https://photo-studio.devops-monk.com">Website</a> ·
<a href="https://github.com/devops-monk/photo-studio/releases/latest">Download</a></p>

## Features

- **Layers**: pixel, type, shape, fill and adjustment layers, plus 27 blend modes, opacity, locking, clipping masks and layer masks
- **Layer styles**: drop shadow, inner shadow, outer and inner glow, stroke and color overlay
- **16 adjustments**, usable live as adjustment layers or applied directly: Levels, Curves, Hue/Saturation, Color Balance, Black & White, Gradient Map, Selective Color and more, plus Auto Tone, Auto Color and Shadows/Highlights
- **30 filters** with live preview inside the selection: blurs (Gaussian, motion, radial, surface, tilt-shift), sharpening, noise, distortions, pixelate, stylize and render
- **Selections**: marquees, lasso, polygonal lasso, magic wand, quick selection, Select Subject and Color Range; feather, expand, contract, border and smooth
- **Painting and retouching**: brush, pencil, eraser, clone stamp, healing and spot healing, blur, sharpen, smudge, dodge, burn, sponge, gradient and paint bucket
- **Type and shapes**: editable text with bundled fonts; rectangles, rounded rectangles, ellipses, polygons, stars and lines
- **Transform**: Free Transform, crop, image size, canvas size, rotate and flip
- **Files**: layered `.pstudio` projects, PSD import and export, and PNG, JPEG, WebP, TIFF, BMP, GIF, TGA, ICO and QOI
- **Undo history panel**, Navigator, Histogram, light and dark themes, and keyboard shortcuts familiar from other editors

## Install

Download the build for your platform from [Releases](https://github.com/devops-monk/photo-studio/releases/latest):

| Platform | File |
|---|---|
| macOS (Apple Silicon and Intel) | `photo-studio-<ver>-macos-universal.dmg` |
| Windows x64 / ARM64 | `photo-studio-<ver>-windows-<arch>.zip` |
| Ubuntu / Debian | `photo-studio_<ver>_<arch>.deb` |
| Any Linux | `.AppImage` or `.tar.gz` |

macOS builds aren't notarized yet. On first launch, right-click the app and choose **Open**.

## Command line

```sh
photo-studio-cli photo.jpg --adjust autotone --filter unsharp=80,1.5 -o photo-fixed.jpg
photo-studio-cli batch ./raw --out-dir ./web --format webp --resize 1600
photo-studio-cli list    # every adjustment and filter
```

## Build from source

```sh
cargo run --release -p photo-studio          # desktop app
cargo test --workspace                       # engine and UI tests
cargo run -p photo-studio --example gen_icons  # regenerate icons
```

On Linux, install `libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libgtk-3-dev` first.
If the GPU driver is old, set `PHOTO_STUDIO_RENDERER=glow`.

## Releasing

Push a tag such as `git tag v0.1.0 && git push origin v0.1.0`. GitHub Actions builds every platform and publishes the release.

## License

MIT © 2026 Abhay Pratap Singh. Bundled fonts are under the SIL Open Font License (see `assets/fonts`).
