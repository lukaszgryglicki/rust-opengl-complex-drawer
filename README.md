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
- `C`: cycle color mode: `solid` / `phase` / `height` / `rings`
- `B`: toggle rainbow hue animation (in `phase`, `height` and `rings` modes)
- `V`: cycle derivative view: `f` / `f'` / `f''` (numeric, computed on the fly)
- `G`: cycle vertical scale: `linear` / `arsinh` / `log10`
- `I`: toggle iso-value lines
- `U/O`: decrease/increase iso-value line count
- `P`: save the current view as PNG
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
`re + im i`, modulus and argument.

## Color modes

`C` cycles four coloring styles; they apply to filled surfaces and wireframes:

- `solid`: one fixed color per surface (see table above).
- `phase`: hue encodes `arg(f(x))` as a rainbow over `(-pi, pi]` — the classic
  complex-phase portrait lifted onto the 3D surfaces. Overlapping surfaces are
  distinguished by brightness: `Re` brightest, then `Im`, `|f|`, `arg`.
- `height`: rainbow by surface height (blue = low, red = high), each surface
  normalized to its own value range.
- `rings`: domain coloring — hue encodes phase and brightness bands form one
  ring per doubling of `|f(x)|`, so zeros and poles show as ring bullseyes.

`B` animates the hue (a slowly rotating rainbow) in the three hue-based modes.
Colors are recomputed in place without re-evaluating the function, but with
very large sample grids the per-frame recolor can still be noticeable.

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

That is, the range is divided into `count + 1` equal intervals and the internal division points are drawn. The default iso-line count is `99`. In filled mode, iso-value lines are drawn as an overlay. In wireframe mode, when iso-value lines are enabled, they replace the regular rectangular grid wireframe.

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

## Tests

```bash
cargo test
```

covers the expression parser/evaluator, gamma, numeric derivatives,
vertical-scale transforms and plot building around poles.

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
