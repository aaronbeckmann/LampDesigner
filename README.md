# Lampdesigner

Parametric, 3D-printable lampshades for the **IKEA GENNAKER** table lamp base (or any E27 socket),
with a physically based light/heat simulation for Bambu Lab filaments and **3MF export for Bambu Studio**.

## Install (Windows)

Download `Lampdesigner-<version>-setup.exe` from the [releases](https://github.com/aaronbeckmann/LampDesigner/releases) and run it. It installs for your user only, without admin rights, into `%LOCALAPPDATA%\Programs\Lampdesigner`, with Start menu and desktop shortcuts. You can uninstall it under *Settings → Apps*.

The installer isn't code-signed, so Windows SmartScreen may say "Windows protected your PC". Click **More info → Run anyway**.

## Build from source

```
cargo run --release            # GUI
.\start.ps1                    # builds if needed and starts the GUI
.\installer\build.ps1          # builds dist\Lampdesigner-<version>-setup.exe (needs WiX 5)
```

The toolchain is pinned to Rust 1.99.1 in `rust-toolchain.toml`, and rustup installs it on the first build. The UI uses [Rune](https://github.com/anomalyco/rune) (GPU-rendered, retained mode), pinned to a git revision in `Cargo.toml`. Start the GUI with `LAMPDESIGNER_SCENE=section|thickness|crown` to open it in a preset state, which is handy for screenshots.

## Features

- **Random generator:** themes are Minimal, Organic, Floral, Geometric, Crystal, Waves, Fluted/Art Deco, Twisted and Surprise. You can set a **max overhang** (20–60°, default 50°) an optional **fixed height**, and an optional **fixed width** (largest outer diameter). Every design is checked for bulb clearance, overhang (measured at export resolution), wall thickness (≥ 0.8 mm), bulb insertion, mount and bed size. A design that is too steep is softened step by step (gentler bulges, folds, waves and ribs) before a new one is rolled. Seeds are reproducible, and ◀ Back walks the history. Your mount, bulb and material are kept. CLI: `lampdesigner-cli random organic 42 out.png --max-overhang 35 --height 140 --width 150 --save design.json`.
- **Shape:** height (up to 800 mm; split tall shades for printing), and a silhouette that is either a bulge curve (bottom/top radius plus a bump or waist) or a free curve through 6 radii. Polygon cross-section with twist.
- **Folds & flow:** lobes/folds with depth, growth toward the top, sharpness, twist and meander. Use 2–3 folds for a swaying organic column, 8–12 for a flowing crown. Optional wavy (scalloped) top rim.
- **Texture:** ridges or narrow grooves. They can follow the fold flow or have their own twist. Also cross-rib lattice, horizontal or spiral waves, smooth rim bands.
  - *Relief*: the inside stays smooth, so ribs become thicker/thinner wall and glow as light/dark bands.
  - *Corrugated*: the whole wall follows the texture (vase look).
- **Mount** (printed as part of the shade, lying on the build plate):
  - *Flat ring Ø28.5*: a washer clamped between the bulb cap and the socket. This is for the GENNAKER, which has no shade ring.
  - *Flat ring Ø40.5*: for sockets with a standard E27 shade-ring thread.
  - *Socket sleeve* (**default for the GENNAKER**: 42.75 mm hole, 2 mm sleeve wall, ledge with 33 mm hole and 2 mm height). It's a tube over the socket body (3–150 mm tall); the hole should be the socket Ø + ~0.35 mm. To save weight on tall sleeves, add **sleeve windows**: 2–12 struts between a solid band at the bottom and one at the top (around the ledge). The window tops are 45° points, so they print without supports. The checks show how many grams this saves. The vase-mode insert gets them too.
  - **Vase mode** (spiral vase print): the shade keeps only the ring and spokes, as its bottom layers. With the socket sleeve, the sleeve becomes a separate **glue-in insert** (exported as `<name>-mount-insert.3mf`, printed normally). It slides up into the shade's ring from below until its glue flange meets the ring, and you glue it there. Glue gap, flange width and flange height are adjustable. Set Bottom shell layers so the ring and spokes print solid; the app shows the number.
  - The spokes are built in layers. At every angle and height their ends sit in the middle of the shade wall, so they always merge fully with the shade and never poke out, even on ribbed, folded or polygon shades. It has an optional **ledge**: an inward ring at the top of the sleeve that rests on the socket's top rim. You set its hole Ø (default 33 mm, which must clear the bulb's E27 cap and neck) and its height. With the 45° chamfer underneath it prints without supports. The checks cover rim overlap, bulb neck/glass collision and the overhang of a flat ledge.
- **Bulbs:** A60 / G95 / G125 / decorative ellipsoid LEDs, plus a 60 W incandescent as a heat reference. All values can be edited.
- **Materials:** Bambu PLA Basic, PLA Matte, PETG HF and PETG Translucent, each with its colour palette or a custom colour.
- **Simulation** (runs in the background while you edit):
  - Photon tracing through the wall: Fresnel reflection, Kubelka–Munk diffuse transmission and reflection, absorption, and unscattered see-through for translucent filament, using the local wall thickness.
  - Results: light output (up / down / through the wall), shade glow in cd/m², bulb hotspot visibility, light pool on the table and wall, and a polar light distribution.
  - Estimated wall and mount temperatures, checked against each filament's softening point.
- **GPU 3D view:** the view is rasterised on the GPU with wgpu (DirectX 12 / Vulkan / Metal), including the translucent shell, light maps from the simulation and section caps. While you drag it renders a fast frame, and when you stop it renders a supersampled one. If no GPU is available it falls back to the CPU ray tracer. Saved images and the material comparison always use the CPU ray tracer.
- **Views:** *Shape* (fast clay render of the geometry), lamp on in a dark room, daylight, and false-colour maps for wall thickness, overhang and temperature.
- **Section / cut view:** ✂ Section cuts the 3D view at any X, Y or Z position. Cut faces are hatched: orange for the shade, blue for the mount, grey for the lamp. *📐 Analysis* opens the 2D section with a mm grid, bulb outline and bulb gap, wall thickness (min / mean / max, thinnest spot marked), diameters for Z cuts, a thickness graph along the cut, and a hover readout of coordinates and local wall. CLI: `lampdesigner-cli section <project> z 80 cut.png`.
- **Light simulation switch:** untick *light* in the Simulation panel to skip photon tracing for quick shape work. The Shape, Daylight, Thickness and Overhang views still work.
- **Quality:** Draft to Ultra refines the preview mesh (up to 3×) and anti-aliasing. The photon count stays the same. *Save image* exports PNGs up to 5120 px wide.
- **Compare materials:** simulates all four filaments with the same exposure, side by side.
- **Printers:** presets for Bambu Lab (A1 mini, A1, P1S/P1P, X1C/X1E, H2D, H2S), Prusa (MINI+, MK4, CORE One, XL), Creality (Ender-3 V3, K1/K1C, K1 Max, K2 Plus), Voron 2.4 350 and Elegoo Neptune 4 Max, or a custom build volume. If a shade is too wide for the bed, the check lists the presets that fit.
- **Split along the height:** shades taller than the printer can be printed in 2–8 parts with even cuts. *Fit printer height* picks the fewest parts. Each cut gets an overlapping (rabbet) joint: the lower part carries the inner half of the wall up as a lip, and the upper part's outer-half skirt slides over it, with an adjustable overlap and glue gap and a 45° chamfer so it prints without supports. A flat butt joint is also available, and vase mode always uses one. Export writes part 1, with the mount, to the chosen file and the rest as `<name>-part2.3mf`, `-part3.3mf`, and so on. Print each part upright and glue them together.
- **Checks:** bulb clearance, whether the bulb fits through the top opening, mount fit, overhangs, minimum wall thickness, bed size and heat.
- **3MF export:** a watertight shell plus mount parts as one object, in mm, centred on the selected printer's bed.

## CLI

The command-line tool is `lampdesigner-cli.exe`. It's installed next to the app, or use `.\start.ps1 <command>` when building from source.

```
lampdesigner-cli export   <project> <out.3mf> [--coarse] [--split n|auto]
lampdesigner-cli simulate [project] [--quality q]   # stats for all materials
lampdesigner-cli render   <project> <out.png> [shape|night|day|thickness|overhang|temperature] [--quality q] [--size WxH]
lampdesigner-cli gpu-render <project> <out.png> [mode] [--size WxH] [--ssaa n] [--clip y:0[:-]]   # same picture via the GPU view
lampdesigner-cli section  <project> <x|y|z> <pos mm> [out.png] [--keep-below]
lampdesigner-cli random   <theme> <seed> [out.png] [--save project.json]
```

`<project>` is a saved `.json`, `default`, or a preset such as `"preset:Flowing crown"`.

Projects are saved as JSON from the GUI (Save/Open).

## Printing in Bambu Studio

1. Import the `.3mf`. Click **Yes** if it asks whether to load it as a single object with multiple parts.
2. Print upright with no supports. The spider is the first layers.
3. Set *Wall loops* so the loops fill the wall: about 3 for 1.2 mm with a 0.4 mm nozzle. For a vase-like finish use a thinner wall.
4. **Use LED bulbs only.** PLA softens at about 55–57 °C. If the washer mount touches the bulb cap, PETG is safer.

## License

[MIT](LICENSE) © 2026 Aaron Beckmann

## Model notes / limitations

- Optical coefficients are estimates calibrated to typical behaviour, e.g. about 18 % diffuse transmission for 1 mm white PLA. Real filament colours and batches vary. Use the results to compare designs and materials, not as lab values.
- The temperature model is a rough steady-state estimate. It covers bulb IR, absorbed light and the warm air plume.
- GENNAKER dimensions: IKEA lists Ø120 mm foot and 100 mm height. The socket diameter (default 40 mm) is a guess, so **measure yours** under *Lamp base*.
