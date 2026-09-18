# Complex Surface Viewer

OpenGL-backed Rust viewer for complex functions `f(x)` over a rectangular complex domain.

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release -- "exp(x)-ln(x)" -2.0 2.0 -2.0 2.0
```

The four optional numeric arguments are:

```text
re_min re_max im_min im_max
```

If omitted, both domains default to `[-2, 2]`.

Options (all optional, may appear anywhere among the positional arguments):

| Option              | Meaning                                                                                                  |
|---------------------|----------------------------------------------------------------------------------------------------------|
| `--color=MODE`      | initial color mode: `solid` (default), `phase`, `height`, `rings` or `4d`                                |
| `--show=LIST`       | initially visible surfaces, comma-separated subset of `re,im,abs,arg` (default `re,im,abs`)              |
| `--colormap=SPEC`   | color map used by the `4d` mode, see [Color map](#color-map); `@FILE` reads the spec from a file          |
| `--screenshot=FILE` | render one frame, save it as PNG to `FILE` and exit (handy for scripting / headless checks under Xvfb)   |
| `--csv=FILE`        | save the grid values as CSV to `FILE` plus the im=0 / re=0 lines to `FILE_im0` / `FILE_re0` and exit, see [CSV export](#csv-export) |
| `--iter=T`          | plot the `T`-th iterate `f^T` for a complex `T` (`2`, `0.5`, `i`, `-.125-.02i`), see [Fractional iteration](#fractional-iteration) |
| `-h`, `--help`      | print usage, including the default color map spec                                                        |
| `--`                | end of options; only needed for an expression that itself starts with `--`, e.g. `-- "--x"`              |

```bash
cargo run --release -- --color=4d --show=re "x^2"
cargo run --release -- --color=4d --show=abs --colormap="3,(0,0,0),(1,0,0),(1,1,1)" "1/x"
cargo run --release -- --iter=0.5 --color=4d --show=re "exp(x)"
```

## Controls

- `R`: stop/start automatic rotation
- `A/D`: yaw left/right
- `W/S`: pitch up/down
- `Q/E`: roll left/right
- `Z`: expand both real and imaginary domains by `1.1x`
- `X`: shrink both domains by `1.1x`
- `H/L`: move real domain by `-/+ 10%` of current width
- `J/K`: move imaginary domain by `-/+ 10%` of current width
- `0`: reset to initial domain
- `N`: decrease samples by ~10%
- `M`: increase samples by ~10%, up to `1024 x 1024` samples
- `T`: toggle opaque / half-transparent
- `F`: toggle filled / wireframe
- `1`: toggle Re(f(x))
- `2`: toggle Im(f(x))
- `3`: toggle |f(x)|
- `4`: toggle arg(f(x))
- `5`: swap visibility of `Re(f) <-> Im(f)` and `|f| <-> arg(f)` (in `4d` mode this
  flips "surface `Re` colored by `Im`" into "surface `Im` colored by `Re`" and back)
- `C`: cycle color mode: `solid` / `phase` / `height` / `rings` / `4d`
- `B`: toggle rainbow hue animation (in `phase`, `height`, `rings` and `4d` modes)
- `V`: cycle derivative view: `f` / `f'` / `f''` (numeric, computed on the fly)
- `G`: cycle vertical scale: `linear` / `arsinh` / `log10`
- `I`: toggle iso-value lines
- `U/O`: decrease/increase iso-value line count
- `P`: save the current view as PNG
- `Y`: save the grid values as CSV, see [CSV export](#csv-export)
- `F1`: hide/show help overlay
- `F11`: toggle fullscreen
- `Esc`: quit

Hold adjustment keys (`Z`, `X`, `H`, `L`, `J`, `K`, `N`, `M`, `U`, `O`) to repeat. The first repeated action starts after about one second, then repeats at a normal keyboard-repeat rate.

## Surfaces

The function value is complex, so the 4D graph is split into up to four
real-valued surfaces over the complex domain plane, toggled with `1`-`4`:

| Key | Surface    | Solid-mode color |
|-----|------------|------------------|
| `1` | `Re(f(x))` | red              |
| `2` | `Im(f(x))` | blue             |
| `3` | `\|f(x)\|` | green            |
| `4` | `arg(f(x))`| orange (off by default) |

All visible surfaces share one vertical axis range. The HUD shows the value
of the currently displayed function (or derivative) at the domain center:
`re + im i`, modulus and argument. To see both components of `f(x)` on one
surface, use the [4D mode](#4d-mode).

## Color modes

`C` cycles five coloring styles; they apply to filled surfaces and wireframes:

- `solid`: one fixed color per surface (see table above).
- `phase`: hue encodes `arg(f(x))` as a rainbow over `(-pi, pi]` — the classic
  complex-phase portrait lifted onto the 3D surfaces. Overlapping surfaces are
  distinguished by brightness: `Re` brightest, then `Im`, `|f|`, `arg`.
- `height`: rainbow by surface height (blue = low, red = high), each surface
  normalized to its own value range.
- `rings`: domain coloring — hue encodes phase and brightness bands form one
  ring per doubling of `|f(x)|`, so zeros and poles show as ring bullseyes.
- `4d`: the surface height stays one component of `f(x)` and its color encodes
  the *other* one, so a single surface shows all four dimensions. See
  [4D mode](#4d-mode).

`B` animates the hue (a slowly rotating rainbow) in the four hue-based modes.
Colors are recomputed in place without re-evaluating the function, but with
very large sample grids the per-frame recolor can still be noticeable.

## 4D mode

A complex function is a map `R^2 -> R^2`, i.e. a 4D graph. The `4d` color mode
(`C` until the HUD says `color=4d`, or start with `--color=4d`) folds the 4th
dimension into color:

- the domain plane is `Re(x)`, `Im(x)` as usual,
- the height of each visible surface is its own component, as in every other mode,
- the color of a surface point is that surface's *paired* component at the same
  `x`: `Re(f)` is colored by `Im(f)`, `Im(f)` by `Re(f)`, `|f|` by `arg(f)`
  and `arg(f)` by `|f|`.

The colored component is normalized linearly from its minimum to its maximum
over the sampled grid (under the current vertical scale, so `G` also affects
the coloring near poles) and looked up in the [color map](#color-map). The HUD
shows one color bar per visible surface with the true `min` and `max` values of
the colored component at its ends.

To view a single colored surface start with `--color=4d --show=re` (or use
`1`-`4` to leave one surface on). `5` swaps `Re <-> Im` (and `|f| <-> arg`)
visibility, flipping between "`Re(f)` surface colored by `Im(f)`" and "`Im(f)`
surface colored by `Re(f)`" with one key. Everything else keeps working in this
mode: `N`/`M` grid density, `F` wireframe (grid lines take the map colors, so a
sparse grid shows the colored structure), `I`/`U`/`O` iso-value lines (drawn
neutral dark gray on top of a filled surface, colored by the map in wireframe
mode where there is no surface underneath), `T` transparency, `G` vertical
scale, `V` derivatives and `B` hue animation, which slides the cyclic color map
along the value range.

```bash
cargo run --release -- --color=4d --show=re "x^2"         # saddle x^2-y^2 colored by 2xy
cargo run --release -- --color=4d --show=im "sin(x)"
cargo run --release -- --color=4d --show=abs "gamma(x)" -4.5 4.5 -2.5 2.5   # |gamma| colored by arg
```

## Color map

The `4d` mode maps the normalized colored component `t in [0, 1]` through a
piecewise-linear color map. The default has 13 evenly spaced stops:

```text
gray -> black -> violet -> indigo -> blue -> teal -> green -> yellow -> orange -> red -> pink -> white -> gray
```

so `min` is black (after a short gray lead-in) and `max` is white. Both ends are
gray, which makes the map cyclic: the `B` hue animation and the wrap from `max`
back to `min` have no visible seam.

A custom map is given with `--colormap=SPEC` (or `--colormap=@FILE` to read
`SPEC` from a file), where

```text
SPEC = "N,(r,g,b),(r,g,b),...,(r,g,b)"
```

- `N` is the number of stops (`>= 2`) and exactly `N` tuples must follow,
- each tuple is `(red, green, blue)` with components in `0..1`,
- the stops are spread evenly from `min` (first stop) to `max` (last stop),
- separators between tuples are lenient: commas and/or whitespace, or none.

Repeat the first stop as the last one if you want the map to stay seamless
under `B` animation. The default map, printed by `--help`, is:

```text
13,(0.5,0.5,0.5),(0,0,0),(0.36,0,0.55),(0.3,0.05,0.8),(0.05,0.35,1),(0,0.7,0.7),(0,0.85,0.1),(1,1,0),(1,0.55,0),(1,0.05,0.05),(1,0.45,0.75),(1,1,1),(0.5,0.5,0.5)
```

Example: a simple black -> red -> white ramp

```bash
cargo run --release -- --color=4d --show=abs --colormap="3,(0,0,0),(1,0,0),(1,1,1)" "1/x"
```

## CSV export

`Y` (or `--csv=FILE`) writes three comma-separated files with a header row,
ready for Google Sheets / LibreOffice charts (first column as X axis, the
others as series):

- `complex_values_<timestamp>.csv` — exactly the displayed grid, one row per
  sample in grid order: `arg-re,arg-im,val-re,val-im,val-abs,val-arg`
- `..._im0.csv` — values along the real axis (`im = 0`): `arg-re,val-re,val-im,val-abs,val-arg`
- `..._re0.csv` — values along the imaginary axis (`re = 0`): `arg-im,val-re,val-im,val-abs,val-arg`

The axis lines are computed when saving at the grid's sample count over the
current `re` / `im` range with the same derivative order and `--iter` setting,
so they are exact even when the grid does not land on `im = 0` / `re = 0`
(and identical to the grid rows when it does). Values are the raw function
values (the `G` vertical scale does not apply); `val-arg` is in radians, and
poles / invalid samples leave the value cells empty. Numbers use `.` as the
decimal separator - pick an English locale in the import dialog if yours uses `,`.

## Derivative view

`V` cycles between plotting `f`, `f'` and `f''`. Derivatives are computed
numerically on the fly with central differences along the real direction
(step scaled to the domain size). For holomorphic functions this equals the
complex derivative; for non-holomorphic expressions such as `conj(x)`, `re(x)`
or `abs(x)` it is only the directional derivative along the real axis. The HUD
indicates which derivative is displayed.

## Vertical scale

`G` cycles the vertical (value) axis mapping, useful near poles where `|f|`
explodes and flattens everything else:

- `linear`: identity.
- `arsinh`: `asinh(y)` — linear near zero, logarithmic for large `|y|`,
  sign-preserving.
- `log10`: `sign(y) * log10(1 + |y|)` — stronger compression.

Axis tick labels always show the true (untransformed) values. Iso-value lines
are spaced evenly in the transformed scale, which keeps them visually evenly
distributed on screen.

## Iso-value lines

Iso-value lines are contour lines drawn on the currently visible surfaces.

For each visible surface, the program uses that surface's own sampled value range. For example, if `Re(f(x))` ranges from `[0, 10]` and the iso-line count is `4`, the lines are drawn at:

```text
2, 4, 6, 8
```

That is, the range is divided into `count + 1` equal intervals and the internal division points are drawn. The default iso-line count is `99`. In filled mode, iso-value lines are drawn as an overlay. In wireframe mode, when iso-value lines are enabled, they replace the regular rectangular grid wireframe. In the `4d` color mode the overlay lines are neutral dark gray, while the wireframe-mode contours are colored by the [color map](#color-map) like the surface they trace.

The iso-line count can be adjusted from `1` to `255`.

## Expression grammar

Supported operators: `+`, `-`, `*`, `/`, `^`, parentheses, comma-separated function calls, unary signs, and implicit multiplication such as `2x`, `2sin(x)`, `(x+1)(x-1)`.

Variable: `x` or `z`, both interpreted as the complex argument.

Constants: `i`, `j`, `pi`, `π`, `tau`, `e`, `inf`, `nan`.

Functions include: `abs`, `mag`, `mod`, `norm`, `abs2`, `mag2`, `norm_sqr`, `normsq`, `l1_norm`, `l1`, `manhattan`, `taxicab`, `arg`, `phase`, `to_polar`, `polar_r`, `radius`, `polar_theta`, `theta`, `re`, `real`, `im`, `imag`, `conj`, `conjugate`, `recip`, `inverse`, `inv`, `finv`, predicates such as `is_nan`, `is_infinite`, `is_finite`, `is_normal`, `sgn`, `sign`, `signum`, `cis`, `exp`, `exp2`, `expf`, `ln`, `log`, `log2`, `log10`, `sqrt`, `cbrt`, `sin`, `cos`, `tan`, inverse trig, hyperbolic functions, `sec`, `csc`, `cot`, `sech`, `csch`, `coth`, `gamma` (`tgamma`), `factorial` (`fact`), `pow`, `powc`, `powf`, `powi`, `powu`, `root`, `scale`, `unscale`, `fdiv`, `complex`, `rect`, `new`, `polar`, `from_polar`, and several component-wise rounding functions. Predicates return `1+0i` for true and `0+0i` for false.

`gamma(x)` is the complex gamma function (Lanczos approximation with reflection
for `Re(x) < 0.5`), and `factorial(x) = gamma(x+1)`. Try:

```bash
cargo run --release -- "gamma(x)" -4.5 4.5 -2.5 2.5
```

with the `rings` color mode and `arsinh` vertical scale.

## Fractional iteration

`--iter=T` plots `f^T`, the `T`-th iterate of `f`, for any complex constant `T`
(same syntax as constants in expressions: `2`, `0.5`, `i`, `-.125-.02i`).
Without `--iter` (or with `--iter=1`) the program behaves exactly as before.

- `T=0` is the identity, positive integers compose `f` directly (`--iter=2` is `f(f(x))`).
- Any other `T` uses numerical *regular iteration*: `f^T = Φ(λ^T Φ⁻¹(x))` where
  `Φ` is the Schröder/Koenigs conjugacy `Φ(λu) = f(Φ(u))` at a hyperbolic fixed
  point `p = f(p)`, `λ = f'(p)`, `0 < |λ| ≠ 1`. `Φ` comes from a Taylor series at
  `p` (Cauchy-integral coefficients) extended by iterating `f`; its inverse is
  followed by Newton continuation along the straight segment from `p` to `x`.
  `λ^T` uses the principal branch. The fixed point is searched once at startup
  near the initial domain (repelling points are preferred, then the one nearest
  to the domain center) and kept while you pan/zoom, so all samples use one
  consistent branch. The HUD shows `p`, `λ` and the series radii.
- `f^T` is single-valued only up to that branch choice: expect cuts, holes
  (points that cannot be reached by the continuation, e.g. `f^0.5(exp)` at
  `0` and `1`, the asymptotic values of `Φ`) and tall spikes along them; `G`
  (arsinh/log10 scale) helps. If `f` has no hyperbolic fixed point near the
  domain (`x+1` has none) the program prints an error and exits with status 2 -
  move the domain or change `f`.
- Cost: grids are built in parallel threads; a 191x191 grid of `exp` takes well
  under a second, resampling happens on every domain change (`Z`/`X`/`H`/`J`/`K`/`L`/`N`/`M`).

```bash
cargo run --release -- --iter=0.5 --color=4d --show=re "exp(x)"   # half-iterate of exp
cargo run --release -- --iter=i "exp(x)"                           # imaginary-order iterate
cargo run --release -- --iter=2 "x^2"                              # x^4, direct composition
cargo run --release -- --iter=0.5 "2x/(1+x)"                        # matches the closed form
```

## Tests

```bash
cargo test
```

covers the expression parser/evaluator, gamma, numeric derivatives,
vertical-scale transforms, plot building around poles, the `4d` coloring and
its color-map parser, the command-line options, fractional iteration
(closed forms for affine/Möbius/`x^2`, `f^0.5∘f^0.5 = exp`) and the CSV export.

## FreeBSD support

Upstream `miniquad` (the platform backend used by `macroquad`) has no FreeBSD
code: on FreeBSD its `start()` compiles to an empty function, so the program
used to print its startup lines and exit silently without ever opening a
window. This is not a GPU/driver problem.

This repository vendors miniquad 0.4.8 in `third_party/miniquad` with the
FreeBSD patch from [not-fl3/miniquad#600](https://github.com/not-fl3/miniquad/pull/600)
applied (it extends the Linux X11/EGL backend to FreeBSD), wired in via
`[patch.crates-io]` in `Cargo.toml`. See also
[not-fl3/macroquad#1012](https://github.com/not-fl3/macroquad/issues/1012),
which confirms this approach working on FreeBSD 14.3.

Requirements on the FreeBSD host:

```bash
pkg install libX11 libXi libxkbcommon mesa-libs
```

(all already present on any desktop FreeBSD install with working OpenGL)

and a running X11 session (`DISPLAY` set). The backend loads `libX11.so`,
`libXi.so`, `libxkbcommon.so` and `libGL.so` (GLX) or `libEGL.so` at runtime
via `dlopen`. Wayland is used as a fallback if X11 initialization fails.

If the backend still fails to create a window, the program now reports
`Error: the graphics backend exited without creating a window.` instead of
exiting silently.

When the upstream PR is merged and released, the vendored copy and the
`[patch.crates-io]` section can be removed.
