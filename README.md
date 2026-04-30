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
- `M`: increase samples by ~10%
- `T`: toggle opaque / half-transparent
- `F`: toggle filled / wireframe
- `1`: toggle Re(f(x))
- `2`: toggle Im(f(x))
- `3`: toggle |f(x)|
- `P`: save the current view as PNG
- `F1`: hide/show help overlay
- `Esc`: quit

## Expression grammar

Supported operators: `+`, `-`, `*`, `/`, `^`, parentheses, comma-separated function calls, unary signs, and implicit multiplication such as `2x`, `2sin(x)`, `(x+1)(x-1)`.

Variable: `x` or `z`, both interpreted as the complex argument.

Constants: `i`, `j`, `pi`, `π`, `tau`, `e`, `inf`, `nan`.

Functions include: `abs`, `arg`, `re`, `im`, `conj`, `exp`, `ln`, `log`, `log2`, `log10`, `sqrt`, `cbrt`, `sin`, `cos`, `tan`, inverse trig, hyperbolic functions, `pow`, `root`, `complex`, `rect`, `polar`, and several component-wise rounding functions.
