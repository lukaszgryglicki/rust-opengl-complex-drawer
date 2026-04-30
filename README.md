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
- `I`: toggle iso-value lines
- `U/O`: decrease/increase iso-value line count
- `P`: save the current view as PNG
- `F1`: hide/show help overlay
- `F11`: toggle fullscreen
- `Esc`: quit

Hold adjustment keys (`Z`, `X`, `H`, `L`, `J`, `K`, `N`, `M`, `U`, `O`) to repeat. The first repeated action starts after about one second, then repeats at a normal keyboard-repeat rate.

## Iso-value lines

Iso-value lines are contour lines drawn on the currently visible surfaces.

For each visible surface, the program uses that surface's own sampled value range. For example, if `Re(f(x))` ranges from `[0, 10]` and the iso-line count is `4`, the lines are drawn at:

```text
2, 4, 6, 8
```

That is, the range is divided into `count + 1` equal intervals and the internal division points are drawn. The default iso-line count is `4`. In filled mode, iso-value lines are drawn as an overlay. In wireframe mode, when iso-value lines are enabled, they replace the regular rectangular grid wireframe.

The iso-line count can be adjusted from `1` to `255`.

## Expression grammar

Supported operators: `+`, `-`, `*`, `/`, `^`, parentheses, comma-separated function calls, unary signs, and implicit multiplication such as `2x`, `2sin(x)`, `(x+1)(x-1)`.

Variable: `x` or `z`, both interpreted as the complex argument.

Constants: `i`, `j`, `pi`, `π`, `tau`, `e`, `inf`, `nan`.

Functions include: `abs`, `mag`, `mod`, `norm`, `abs2`, `mag2`, `norm_sqr`, `normsq`, `l1_norm`, `l1`, `manhattan`, `taxicab`, `arg`, `phase`, `to_polar`, `polar_r`, `radius`, `polar_theta`, `theta`, `re`, `real`, `im`, `imag`, `conj`, `conjugate`, `recip`, `inverse`, `inv`, `finv`, predicates such as `is_nan`, `is_infinite`, `is_finite`, `is_normal`, `sgn`, `sign`, `signum`, `cis`, `exp`, `exp2`, `expf`, `ln`, `log`, `log2`, `log10`, `sqrt`, `cbrt`, `sin`, `cos`, `tan`, inverse trig, hyperbolic functions, `sec`, `csc`, `cot`, `sech`, `csch`, `coth`, `pow`, `powc`, `powf`, `powi`, `powu`, `root`, `scale`, `unscale`, `fdiv`, `complex`, `rect`, `new`, `polar`, `from_polar`, and several component-wise rounding functions. Predicates return `1+0i` for true and `0+0i` for false.
