use macroquad::camera::Camera;
use macroquad::prelude::*;
use num_complex::{Complex64, ComplexFloat};
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SAMPLES_PER_AXIS: usize = 191;
const MIN_SAMPLES_PER_AXIS: usize = 5;
const MAX_SAMPLES_PER_AXIS: usize = 1024;
const MAX_MESH_SAMPLES_PER_AXIS: usize = 255;
const DEFAULT_ISO_LINE_COUNT: usize = 99;
const MIN_ISO_LINE_COUNT: usize = 1;
const MAX_ISO_LINE_COUNT: usize = 255;
const DOMAIN_EXTENT: f32 = 1.55;
const Y_EXTENT: f32 = 1.25;
const ISO_LINE_LIFT: f32 = 0.006;
const KEY_REPEAT_INITIAL_DELAY_SECONDS: f64 = 1.0;
const KEY_REPEAT_INTERVAL_SECONDS: f64 = 1.0 / 30.0;
const AUTO_ROTATE_RADIANS_PER_SEC: f32 = 0.15;
const MANUAL_ROTATE_RADIANS_PER_SEC: f32 = 0.95;
const TRANSPARENT_ALPHA: f32 = 0.50;
const HUE_ANIM_CYCLES_PER_SEC: f32 = 0.10;
// Relative step sizes for numeric differentiation, scaled by the domain span.
const DERIV_H1_REL: f64 = 1e-6;
const DERIV_H2_REL: f64 = 5e-5;
// Frames rendered before `--screenshot` captures the window (lets the window settle).
const SCREENSHOT_FRAME: u32 = 3;

// Default color map of the `4d` color mode: evenly spaced stops over [0, 1]
// (min..max of the color component). Both ends are the same gray so that the
// map is cyclic: shifting it (hue animation) has no seam. Override with
// `--colormap="N,(r,g,b),...(r,g,b)"`.
const DEFAULT_COLORMAP_STOPS: [(f32, f32, f32); 13] = [
    (0.50, 0.50, 0.50), // gray
    (0.00, 0.00, 0.00), // black
    (0.36, 0.00, 0.55), // violet
    (0.30, 0.05, 0.80), // indigo
    (0.05, 0.35, 1.00), // blue
    (0.00, 0.70, 0.70), // teal
    (0.00, 0.85, 0.10), // green
    (1.00, 1.00, 0.00), // yellow
    (1.00, 0.55, 0.00), // orange
    (1.00, 0.05, 0.05), // red
    (1.00, 0.45, 0.75), // pink
    (1.00, 1.00, 1.00), // white
    (0.50, 0.50, 0.50), // gray
];

type C = Complex64;

/// Color map of the `4d` color mode: N evenly spaced RGB stops over [0, 1],
/// linearly interpolated in between.
#[derive(Clone, Debug, PartialEq)]
struct ColorMap {
    stops: Vec<(f32, f32, f32)>,
}

impl Default for ColorMap {
    fn default() -> Self {
        Self {
            stops: DEFAULT_COLORMAP_STOPS.to_vec(),
        }
    }
}

impl ColorMap {
    /// Parses `"N,(r,g,b),(r,g,b),..."`: N (at least 2) stops with components in
    /// `0..=1`, evenly spaced from the minimum to the maximum of the color
    /// component. Separators between stops may be commas and/or whitespace.
    fn parse(spec: &str) -> Result<Self, String> {
        let Some(first_paren) = spec.find('(') else {
            return Err("color map must look like \"N,(r,g,b),(r,g,b),...\"".to_owned());
        };
        let count_text = spec[..first_paren].trim().trim_end_matches(',').trim();
        let count: usize = count_text.parse().map_err(|_| {
            format!("color map must start with the number of stops, got '{count_text}'")
        })?;
        if count < 2 {
            return Err(format!("color map needs at least 2 stops, got {count}"));
        }

        // `count` is unvalidated user input, so do not size an allocation from it.
        let mut stops = Vec::new();
        let mut rest = spec[first_paren..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        while !rest.is_empty() {
            if stops.len() == count {
                return Err(format!(
                    "color map declares {count} stops but lists more (at '{}')",
                    excerpt(rest)
                ));
            }
            let Some(inner) = rest.strip_prefix('(') else {
                return Err(format!("expected '(' at '{}' in color map", excerpt(rest)));
            };
            let Some(close) = inner.find(')') else {
                return Err(format!("missing ')' at '{}' in color map", excerpt(rest)));
            };
            let tuple = &inner[..close];
            let parts: Vec<&str> = tuple.split(',').map(str::trim).collect();
            if parts.len() != 3 {
                return Err(format!("color map stop '({tuple})' must have 3 components (r,g,b)"));
            }
            let mut rgb = [0.0f32; 3];
            for (component, part) in rgb.iter_mut().zip(parts.iter()) {
                let value: f32 = part.parse().map_err(|_| {
                    format!("invalid number '{part}' in color map stop '({tuple})'")
                })?;
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(format!(
                        "component {value} in color map stop '({tuple})' is outside 0..=1"
                    ));
                }
                *component = value;
            }
            stops.push((rgb[0], rgb[1], rgb[2]));
            rest = inner[close + 1..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        }

        if stops.len() != count {
            return Err(format!(
                "color map declares {count} stops but lists {}",
                stops.len()
            ));
        }
        Ok(Self { stops })
    }

    /// Reads the spec from a file when `arg` starts with `@`, otherwise parses `arg` itself.
    fn from_cli_arg(arg: &str) -> Result<Self, String> {
        match arg.strip_prefix('@') {
            Some(path) => {
                let spec = std::fs::read_to_string(path)
                    .map_err(|err| format!("cannot read color map file '{path}': {err}"))?;
                Self::parse(&spec).map_err(|err| format!("{err} (in '{path}')"))
            }
            None => Self::parse(arg),
        }
    }

    /// The spec string that reproduces this map (the format accepted by `parse`).
    fn to_spec(&self) -> String {
        let stops: Vec<String> = self
            .stops
            .iter()
            .map(|(r, g, b)| format!("({r},{g},{b})"))
            .collect();
        format!("{},{}", self.stops.len(), stops.join(","))
    }

    /// Piecewise-linear interpolation between the stops: `0` is the first stop and
    /// `1` the last. Values outside `[0, 1]` (hue-animation offsets) wrap modulo 1,
    /// which is seamless whenever the first and last stops are equal (as in the default).
    fn color(&self, t: f32) -> (f32, f32, f32) {
        let t = if !t.is_finite() {
            0.0
        } else if (0.0..=1.0).contains(&t) {
            t
        } else {
            t.rem_euclid(1.0)
        };
        let segments = self.stops.len() - 1;
        let x = t * segments as f32;
        let i = (x.floor() as usize).min(segments - 1);
        let f = x - i as f32;
        let (r0, g0, b0) = self.stops[i];
        let (r1, g1, b1) = self.stops[i + 1];
        (
            r0 + (r1 - r0) * f,
            g0 + (g1 - g0) * f,
            b0 + (b1 - b0) * f,
        )
    }
}

fn excerpt(text: &str) -> String {
    let short: String = text.chars().take(24).collect();
    if short.len() < text.len() {
        format!("{short}...")
    } else {
        short
    }
}

/// Per-vertex color inputs shared by mesh building, in-place recoloring and
/// wireframe drawing.
#[derive(Clone, Copy)]
struct Shading<'a> {
    mode: ColorMode,
    hue_offset: f32,
    transparent: bool,
    colormap: &'a ColorMap,
}

fn window_conf() -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: macroquad::miniquad::conf::Conf {
            window_title: "Complex function surface viewer".to_owned(),
            window_width: 1280,
            window_height: 900,
            high_dpi: true,
            sample_count: 4,
            ..Default::default()
        },
        // draw_mesh() uses u16 indices internally. Larger sample grids are split into
        // multiple tiles, each no larger than MAX_MESH_SAMPLES_PER_AXIS per axis.
        draw_call_vertex_capacity: MAX_MESH_SAMPLES_PER_AXIS * MAX_MESH_SAMPLES_PER_AXIS + 1024,
        draw_call_index_capacity: (MAX_MESH_SAMPLES_PER_AXIS - 1) * (MAX_MESH_SAMPLES_PER_AXIS - 1) * 6 + 1024,
        ..Default::default()
    }
}

fn main() {
    // Parse CLI and the expression before creating the window, so `--help`,
    // usage errors and parse errors work without a GPU/display.
    let cli = parse_cli_or_exit();
    let expr = match Parser::parse(&cli.function) {
        Ok(expr) => expr,
        Err(err) => {
            eprintln!("Parse error: {err}");
            std::process::exit(2);
        }
    };

    println!("Function: {}", cli.function);
    println!("Real domain: [{}, {}]", cli.re.min, cli.re.max);
    println!("Imag domain: [{}, {}]", cli.im.min, cli.im.max);
    println!("Samples: {} x {}", DEFAULT_SAMPLES_PER_AXIS, DEFAULT_SAMPLES_PER_AXIS);
    println!("Press F1 in the window for controls.");

    macroquad::Window::from_config(window_conf(), run_viewer(cli, expr));

    // On a working platform `Window::from_config` blocks until the user quits,
    // and at least one frame is always rendered first. If we get here without
    // ever rendering a frame, the platform backend failed to create a window
    // (it can fail silently, e.g. unsupported OS or no usable display).
    if !FRAME_RENDERED.load(Ordering::Relaxed) {
        eprintln!("Error: the graphics backend exited without creating a window.");
        eprintln!("No frame was ever rendered. Likely causes:");
        eprintln!("  - no graphical session (DISPLAY/WAYLAND_DISPLAY unset or unreachable)");
        eprintln!("  - missing X11/EGL runtime libraries (libX11, libXi, libEGL, libGL)");
        eprintln!("  - an OS without windowing support in the miniquad backend");
        std::process::exit(1);
    }
}

/// Set to `true` on the first rendered frame; used to detect a silent
/// platform-backend failure (see the check at the end of `main`).
static FRAME_RENDERED: AtomicBool = AtomicBool::new(false);

async fn run_viewer(cli: Cli, expr: Expr) {
    let mut screenshot_path = cli.screenshot.clone();
    let mut frame_index: u32 = 0;
    let mut state = AppState {
        function_text: cli.function.clone(),
        expr,
        domain: Domain {
            re: cli.re,
            im: cli.im,
        },
        initial_domain: Domain {
            re: cli.re,
            im: cli.im,
        },
        plot: None,
        samples_per_axis: DEFAULT_SAMPLES_PER_AXIS,
        show_real: cli.visibility.show_real,
        show_imag: cli.visibility.show_imag,
        show_abs: cli.visibility.show_abs,
        show_arg: cli.visibility.show_arg,
        color_mode: cli.color_mode,
        colormap: cli.colormap.clone(),
        hue_anim: false,
        hue_offset: 0.0,
        deriv_order: 0,
        y_scale: YScale::Linear,
        transparent_surfaces: false,
        wireframe_mode: false,
        iso_lines_enabled: false,
        iso_line_count: DEFAULT_ISO_LINE_COUNT,
        fullscreen: false,
        yaw: 0.75,
        pitch: 0.52,
        roll: 0.0,
        auto_rotate: true,
        show_help: true,
        status: String::new(),
    };

    state.rebuild_plot();

    let mut repeat_keys = KeyRepeater::new(&[
        KeyCode::Z,
        KeyCode::X,
        KeyCode::H,
        KeyCode::L,
        KeyCode::J,
        KeyCode::K,
        KeyCode::N,
        KeyCode::M,
        KeyCode::U,
        KeyCode::O,
    ]);


    loop {
        FRAME_RENDERED.store(true, Ordering::Relaxed);

        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        let mut rebuild_plot = false;
        let now = get_time();

        if is_key_pressed(KeyCode::F1) {
            state.show_help = !state.show_help;
        }
        if is_key_pressed(KeyCode::F11) {
            state.fullscreen = !state.fullscreen;
            set_fullscreen(state.fullscreen);
        }
        if is_key_pressed(KeyCode::R) {
            state.auto_rotate = !state.auto_rotate;
        }
        if is_key_pressed(KeyCode::Key0) {
            state.domain = state.initial_domain;
            rebuild_plot = true;
        }

        // Domain controls: one discrete update per key press.
        if repeat_keys.should_fire(KeyCode::Z, now) {
            state.domain.scale_about_center(1.1);
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::X, now) {
            state.domain.scale_about_center(1.0 / 1.1);
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::H, now) {
            state.domain.shift_re(-0.10);
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::L, now) {
            state.domain.shift_re(0.10);
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::J, now) {
            state.domain.shift_im(-0.10);
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::K, now) {
            state.domain.shift_im(0.10);
            rebuild_plot = true;
        }

        // Resolution controls.
        if repeat_keys.should_fire(KeyCode::N, now) {
            rebuild_plot |= state.scale_samples(1.0 / 1.1);
        }
        if repeat_keys.should_fire(KeyCode::M, now) {
            rebuild_plot |= state.scale_samples(1.1);
        }

        // Surface visibility and rendering mode controls.
        if is_key_pressed(KeyCode::Key1) {
            state.show_real = !state.show_real;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::Key2) {
            state.show_imag = !state.show_imag;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::Key3) {
            state.show_abs = !state.show_abs;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::Key4) {
            state.show_arg = !state.show_arg;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::Key5) {
            // Swap visibility Re<->Im and |f|<->arg: in the `4d` color mode this
            // flips "Re surface colored by Im" into "Im surface colored by Re".
            std::mem::swap(&mut state.show_real, &mut state.show_imag);
            std::mem::swap(&mut state.show_abs, &mut state.show_arg);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::T) {
            state.transparent_surfaces = !state.transparent_surfaces;
            state.recolor();
        }
        if is_key_pressed(KeyCode::C) {
            state.color_mode = state.color_mode.next();
            state.recolor();
        }
        if is_key_pressed(KeyCode::B) {
            state.hue_anim = !state.hue_anim;
        }
        if is_key_pressed(KeyCode::V) {
            state.deriv_order = (state.deriv_order + 1) % 3;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::G) {
            state.y_scale = state.y_scale.next();
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::F) {
            state.wireframe_mode = !state.wireframe_mode;
        }
        if is_key_pressed(KeyCode::I) {
            state.iso_lines_enabled = !state.iso_lines_enabled;
            rebuild_plot = true;
        }
        if repeat_keys.should_fire(KeyCode::U, now) {
            rebuild_plot |= state.change_iso_line_count(-1);
        }
        if repeat_keys.should_fire(KeyCode::O, now) {
            rebuild_plot |= state.change_iso_line_count(1);
        }

        if rebuild_plot {
            state.rebuild_plot();
        }

        let dt = get_frame_time();
        if state.auto_rotate {
            state.yaw += AUTO_ROTATE_RADIANS_PER_SEC * dt;
        }
        if state.hue_anim {
            state.hue_offset = (state.hue_offset + HUE_ANIM_CYCLES_PER_SEC * dt).rem_euclid(1.0);
            if state.color_mode.uses_hue() && !state.wireframe_mode {
                state.recolor();
            }
        }

        // View rotation controls: continuous while held.
        if is_key_down(KeyCode::A) {
            state.yaw -= MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        if is_key_down(KeyCode::D) {
            state.yaw += MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        if is_key_down(KeyCode::W) {
            state.pitch += MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        if is_key_down(KeyCode::S) {
            state.pitch -= MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        if is_key_down(KeyCode::Q) {
            state.roll -= MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        if is_key_down(KeyCode::E) {
            state.roll += MANUAL_ROTATE_RADIANS_PER_SEC * dt;
        }
        state.pitch = state.pitch.clamp(-1.35, 1.35);

        clear_background(Color::new(0.97, 0.97, 0.95, 1.0));

        let camera = state.camera();
        set_camera(&camera);

        let mut labels = Vec::<(Vec3, String, Color)>::new();
        if let Some(plot) = &state.plot {
            let shading = state.shading();
            // Back-to-front-ish fixed order so transparency looks reasonable.
            let draw_order = [
                SurfaceKind::Arg,
                SurfaceKind::Abs,
                SurfaceKind::Imag,
                SurfaceKind::Real,
            ];
            if state.wireframe_mode {
                for kind in draw_order {
                    if !state.is_visible(kind) {
                        continue;
                    }
                    if state.iso_lines_enabled {
                        draw_iso_line_segments(&plot.surface(kind).iso_lines, kind, shading, false);
                    } else {
                        draw_wireframe_surface(
                            &plot.samples,
                            plot.samples_per_axis,
                            state.domain,
                            plot.y,
                            kind,
                            plot.surface(kind).value_range,
                            plot.surface(kind.partner()).value_range,
                            state.y_scale,
                            shading,
                        );
                    }
                }
            } else {
                for kind in draw_order {
                    if state.is_visible(kind) {
                        draw_meshes(&plot.surface(kind).meshes);
                    }
                }

                if state.iso_lines_enabled {
                    for kind in draw_order {
                        if state.is_visible(kind) {
                            draw_iso_line_segments(&plot.surface(kind).iso_lines, kind, shading, true);
                        }
                    }
                }
            }
            labels = draw_axes_and_ticks(&state.domain, plot, state.y_scale);
        }

        set_default_camera();
        for (pos, text, color) in labels {
            draw_label_3d(&camera, pos, &text, color, 18.0);
        }
        draw_hud(&state);

        if is_key_pressed(KeyCode::P) {
            let filename = format!("complex_view_{}.png", unix_timestamp_seconds());
            save_screenshot(&filename);
            state.status = format!("saved {filename}");
        }

        // `--screenshot=FILE`: capture once the window has settled, then quit.
        if frame_index >= SCREENSHOT_FRAME {
            if let Some(path) = screenshot_path.take() {
                save_screenshot(&path);
                break;
            }
        }
        frame_index = frame_index.saturating_add(1);

        next_frame().await;
    }
}

fn save_screenshot(path: &str) {
    get_screen_data().export_png(path);
    println!("Saved screenshot to {path}");
}

fn parse_cli_or_exit() -> Cli {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") || args.is_empty() {
        print_usage();
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }

    parse_cli(&args).unwrap_or_else(|err| {
        eprintln!("{err}");
        eprintln!();
        print_usage();
        std::process::exit(2);
    })
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  complex_surface_viewer [options] \"exp(x)-ln(x)\" [re_min re_max im_min im_max]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --color=MODE       initial color mode: solid, phase, height, rings, 4d (default: solid)");
    eprintln!("  --show=LIST        initially visible surfaces, comma-separated subset of re,im,abs,arg");
    eprintln!("                     (default: re,im,abs)");
    eprintln!("  --colormap=SPEC    color map of the 4d mode: \"N,(r,g,b),...,(r,g,b)\" with N stops");
    eprintln!("                     (components 0..1) spread evenly from min to max of the colored");
    eprintln!("                     component; use @FILE to read SPEC from a file. Default:");
    eprintln!("                     \"{}\"", ColorMap::default().to_spec());
    eprintln!("  --screenshot=FILE  render one frame, save it as PNG to FILE and exit");
    eprintln!("  -h, --help         show this help");
    eprintln!("  --                 end of options (needed only for expressions starting with \"--\")");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  complex_surface_viewer \"exp(x)-ln(x)\" -2 2 -2 2");
    eprintln!("  complex_surface_viewer \"sin(x)/x\"");
    eprintln!("  complex_surface_viewer \"gamma(x)\" -4.5 4.5 -2.5 2.5");
    eprintln!("  complex_surface_viewer --color=4d --show=re \"x^2\"");
    eprintln!("  complex_surface_viewer --color=4d --show=abs --colormap=\"3,(0,0,0),(1,0,0),(1,1,1)\" \"1/x\"");
}

fn parse_cli(args: &[String]) -> Result<Cli, String> {
    let mut color_mode = ColorMode::Solid;
    let mut visibility = SurfaceVisibility::default();
    let mut colormap = ColorMap::default();
    let mut screenshot = None;
    let mut positional = Vec::<&str>::new();
    let mut options_done = false;

    for arg in args {
        if options_done {
            positional.push(arg.as_str());
        } else if arg == "--" {
            // Conventional end of options; lets expressions such as "--x" through.
            options_done = true;
        } else if let Some(value) = arg.strip_prefix("--color=") {
            color_mode = ColorMode::from_cli_name(value)
                .ok_or_else(|| format!("Unknown color mode '{value}' (expected solid, phase, height, rings or 4d)"))?;
        } else if let Some(value) = arg.strip_prefix("--show=") {
            visibility = SurfaceVisibility::from_cli_list(value)?;
        } else if let Some(value) = arg.strip_prefix("--colormap=") {
            colormap = ColorMap::from_cli_arg(value).map_err(|err| format!("Invalid --colormap: {err}"))?;
        } else if let Some(value) = arg.strip_prefix("--screenshot=") {
            if value.is_empty() {
                return Err("--screenshot requires a file name".to_owned());
            }
            screenshot = Some(value.to_owned());
        } else if arg.starts_with("--") {
            return Err(format!("Unknown option '{arg}'"));
        } else {
            positional.push(arg.as_str());
        }
    }

    if positional.len() != 1 && positional.len() != 5 {
        return Err(format!(
            "Expected either 1 or 5 positional arguments, got {}.",
            positional.len()
        ));
    }

    let (re, im) = if positional.len() == 5 {
        (
            Range::new(parse_f64_arg(positional[1], "re_min")?, parse_f64_arg(positional[2], "re_max")?)?,
            Range::new(parse_f64_arg(positional[3], "im_min")?, parse_f64_arg(positional[4], "im_max")?)?,
        )
    } else {
        (Range::new(-2.0, 2.0)?, Range::new(-2.0, 2.0)?)
    };

    Ok(Cli {
        function: positional[0].to_owned(),
        re,
        im,
        color_mode,
        visibility,
        colormap,
        screenshot,
    })
}

fn parse_f64_arg(s: &str, name: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{name} must be a finite floating-point number, got '{s}'"))
}

#[derive(Clone)]
struct Cli {
    function: String,
    re: Range,
    im: Range,
    color_mode: ColorMode,
    visibility: SurfaceVisibility,
    colormap: ColorMap,
    screenshot: Option<String>,
}

#[derive(Clone, Copy)]
struct Range {
    min: f64,
    max: f64,
}

impl Range {
    fn new(min: f64, max: f64) -> Result<Self, String> {
        if !min.is_finite() || !max.is_finite() {
            return Err("Range endpoints must be finite".to_owned());
        }
        if min >= max {
            return Err(format!("Invalid range [{min}, {max}]: min must be smaller than max"));
        }
        Ok(Self { min, max })
    }

    fn len(self) -> f64 {
        self.max - self.min
    }

    fn mid(self) -> f64 {
        0.5 * (self.min + self.max)
    }

    fn contains(self, value: f64) -> bool {
        self.min <= value && value <= self.max
    }

    fn scale_about_center(&mut self, factor: f64) {
        let center = self.mid();
        let half = 0.5 * self.len() * factor;
        self.min = center - half;
        self.max = center + half;
    }

    fn shift_by_fraction(&mut self, fraction: f64) {
        let d = self.len() * fraction;
        self.min += d;
        self.max += d;
    }

    fn ticks_5(self) -> [f64; 5] {
        let span = self.len();
        [
            self.min,
            self.min + 0.25 * span,
            self.min + 0.50 * span,
            self.min + 0.75 * span,
            self.max,
        ]
    }
}

#[derive(Clone, Copy)]
struct Domain {
    re: Range,
    im: Range,
}

impl Domain {
    fn scale_about_center(&mut self, factor: f64) {
        self.re.scale_about_center(factor);
        self.im.scale_about_center(factor);
    }

    fn shift_re(&mut self, fraction: f64) {
        self.re.shift_by_fraction(fraction);
    }

    fn shift_im(&mut self, fraction: f64) {
        self.im.shift_by_fraction(fraction);
    }
}

struct KeyRepeater {
    states: Vec<KeyRepeatState>,
}

struct KeyRepeatState {
    key: KeyCode,
    was_down: bool,
    next_repeat_time: f64,
}

impl KeyRepeater {
    fn new(keys: &[KeyCode]) -> Self {
        Self {
            states: keys
                .iter()
                .copied()
                .map(|key| KeyRepeatState {
                    key,
                    was_down: false,
                    next_repeat_time: 0.0,
                })
                .collect(),
        }
    }

    fn should_fire(&mut self, key: KeyCode, now: f64) -> bool {
        let Some(state) = self.states.iter_mut().find(|state| state.key == key) else {
            return is_key_pressed(key);
        };

        if !is_key_down(key) {
            state.was_down = false;
            state.next_repeat_time = 0.0;
            return false;
        }

        if !state.was_down {
            state.was_down = true;
            state.next_repeat_time = now + KEY_REPEAT_INITIAL_DELAY_SECONDS;
            return true;
        }

        if now >= state.next_repeat_time {
            state.next_repeat_time = now + KEY_REPEAT_INTERVAL_SECONDS;
            return true;
        }

        false
    }
}

struct AppState {
    function_text: String,
    expr: Expr,
    domain: Domain,
    initial_domain: Domain,
    plot: Option<PlotData>,
    samples_per_axis: usize,
    show_real: bool,
    show_imag: bool,
    show_abs: bool,
    show_arg: bool,
    color_mode: ColorMode,
    colormap: ColorMap,
    hue_anim: bool,
    hue_offset: f32,
    deriv_order: u8,
    y_scale: YScale,
    transparent_surfaces: bool,
    wireframe_mode: bool,
    iso_lines_enabled: bool,
    iso_line_count: usize,
    fullscreen: bool,
    yaw: f32,
    pitch: f32,
    roll: f32,
    auto_rotate: bool,
    show_help: bool,
    status: String,
}

impl AppState {
    fn shading(&self) -> Shading<'_> {
        Shading {
            mode: self.color_mode,
            hue_offset: self.hue_offset,
            transparent: self.transparent_surfaces,
            colormap: &self.colormap,
        }
    }

    fn rebuild_plot(&mut self) {
        let plot = build_plot(
            &self.expr,
            self.domain,
            self.samples_per_axis,
            self.visibility(),
            self.shading(),
            self.iso_lines_enabled,
            self.iso_line_count,
            self.deriv_order,
            self.y_scale,
        );
        self.status = format!(
            "domain re=[{}, {}] im=[{}, {}]  y=[{}, {}]  samples: {}x{}  finite: {}/{}  iso: {}({})  visible: {}{}{}{}",
            fmt_axis(self.domain.re.min),
            fmt_axis(self.domain.re.max),
            fmt_axis(self.domain.im.min),
            fmt_axis(self.domain.im.max),
            fmt_axis(self.y_scale.invert(plot.y.min)),
            fmt_axis(self.y_scale.invert(plot.y.max)),
            self.samples_per_axis,
            self.samples_per_axis,
            plot.finite_sample_count,
            plot.total_sample_count,
            on_off(self.iso_lines_enabled),
            self.iso_line_count,
            if self.show_real { "Re " } else { "" },
            if self.show_imag { "Im " } else { "" },
            if self.show_abs { "|f| " } else { "" },
            if self.show_arg { "arg" } else { "" },
        );
        self.plot = Some(plot);
    }

    fn recolor(&mut self) {
        // Built inline (not via `shading()`) so `plot` can be borrowed mutably alongside.
        let shading = Shading {
            mode: self.color_mode,
            hue_offset: self.hue_offset,
            transparent: self.transparent_surfaces,
            colormap: &self.colormap,
        };
        if let Some(plot) = &mut self.plot {
            recolor_plot(plot, shading);
        }
    }

    fn is_visible(&self, kind: SurfaceKind) -> bool {
        match kind {
            SurfaceKind::Real => self.show_real,
            SurfaceKind::Imag => self.show_imag,
            SurfaceKind::Abs => self.show_abs,
            SurfaceKind::Arg => self.show_arg,
        }
    }

    fn visibility(&self) -> SurfaceVisibility {
        SurfaceVisibility {
            show_real: self.show_real,
            show_imag: self.show_imag,
            show_abs: self.show_abs,
            show_arg: self.show_arg,
        }
    }

    fn scale_samples(&mut self, factor: f64) -> bool {
        let mut next = ((self.samples_per_axis as f64) * factor).round() as usize;
        if next == self.samples_per_axis {
            next = if factor > 1.0 {
                self.samples_per_axis.saturating_add(1)
            } else {
                self.samples_per_axis.saturating_sub(1)
            };
        }
        next = next.clamp(MIN_SAMPLES_PER_AXIS, MAX_SAMPLES_PER_AXIS);
        if next < 2 || next == self.samples_per_axis {
            return false;
        }
        self.samples_per_axis = next;
        true
    }

    fn change_iso_line_count(&mut self, delta: isize) -> bool {
        let next = if delta < 0 {
            self.iso_line_count.saturating_sub(delta.unsigned_abs())
        } else {
            self.iso_line_count.saturating_add(delta as usize)
        }
        .clamp(MIN_ISO_LINE_COUNT, MAX_ISO_LINE_COUNT);

        if next == self.iso_line_count {
            return false;
        }
        self.iso_line_count = next;
        true
    }

    fn camera(&self) -> Camera3D {
        let radius = 5.25;
        let cp = self.pitch.cos();
        let eye = vec3(
            radius * cp * self.yaw.sin(),
            radius * self.pitch.sin(),
            radius * cp * self.yaw.cos(),
        );
        let target = Vec3::ZERO;
        let forward = (target - eye).normalize();
        let base_up = Vec3::Y;
        let mut right = forward.cross(base_up);
        if right.length_squared() < 1e-6 {
            right = Vec3::X;
        } else {
            right = right.normalize();
        }
        let up_without_roll = right.cross(forward).normalize();
        let up = Quat::from_axis_angle(forward, self.roll).mul_vec3(up_without_roll);

        Camera3D {
            position: eye,
            target,
            up,
            fovy: 45.0_f32.to_radians(),
            z_near: 0.01,
            z_far: 100.0,
            ..Default::default()
        }
    }
}

struct PlotData {
    surfaces: [SurfaceData; 4],
    y: Range,
    finite_sample_count: usize,
    total_sample_count: usize,
    samples_per_axis: usize,
    samples: Vec<Sample>,
    center_input: C,
    center_value: C,
}

impl PlotData {
    fn surface(&self, kind: SurfaceKind) -> &SurfaceData {
        &self.surfaces[kind.index()]
    }
}

#[derive(Default)]
struct SurfaceData {
    meshes: Vec<Mesh>,
    // Per mesh, per vertex: color inputs so meshes can be recolored in place
    // (color-mode change, transparency toggle, hue animation) without re-evaluating f.
    vertex_infos: Vec<Vec<VertexInfo>>,
    iso_lines: Vec<IsoSegment>,
    // Transformed (y-scaled) per-surface value range.
    value_range: Option<Range>,
}

/// One marching-squares contour piece on a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IsoSegment {
    a: Vec3,
    b: Vec3,
    /// Paired component (see `SurfaceKind::partner`) normalized to its range, averaged
    /// over the segment; lets the `4d` mode color contours when no surface is drawn.
    color01: f32,
}

#[derive(Clone, Copy)]
struct VertexInfo {
    sample: u32,
    /// This surface's own value, normalized to its sampled range.
    value01: f32,
    /// The paired component (Re<->Im, |f|<->arg), normalized to its sampled range;
    /// the color source in the `4d` color mode.
    color01: f32,
}

#[derive(Clone, Copy)]
struct Sample {
    real: f64,
    imag: f64,
    abs: f64,
    arg: f64,
    valid: bool,
}

impl Sample {
    fn invalid() -> Self {
        Self {
            real: f64::NAN,
            imag: f64::NAN,
            abs: f64::NAN,
            arg: f64::NAN,
            valid: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SurfaceVisibility {
    show_real: bool,
    show_imag: bool,
    show_abs: bool,
    show_arg: bool,
}

impl Default for SurfaceVisibility {
    fn default() -> Self {
        Self {
            show_real: true,
            show_imag: true,
            show_abs: true,
            show_arg: false,
        }
    }
}

impl SurfaceVisibility {
    fn on(self, kind: SurfaceKind) -> bool {
        match kind {
            SurfaceKind::Real => self.show_real,
            SurfaceKind::Imag => self.show_imag,
            SurfaceKind::Abs => self.show_abs,
            SurfaceKind::Arg => self.show_arg,
        }
    }

    /// Parses a `--show=` list such as `re,im` or `abs`.
    fn from_cli_list(list: &str) -> Result<Self, String> {
        let mut visibility = Self {
            show_real: false,
            show_imag: false,
            show_abs: false,
            show_arg: false,
        };
        for name in list.split(',').map(str::trim).filter(|name| !name.is_empty()) {
            match SurfaceKind::from_cli_name(name) {
                Some(SurfaceKind::Real) => visibility.show_real = true,
                Some(SurfaceKind::Imag) => visibility.show_imag = true,
                Some(SurfaceKind::Abs) => visibility.show_abs = true,
                Some(SurfaceKind::Arg) => visibility.show_arg = true,
                None => {
                    return Err(format!(
                        "Unknown surface '{name}' in --show (expected a comma-separated subset of re,im,abs,arg)"
                    ))
                }
            }
        }
        Ok(visibility)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColorMode {
    /// One fixed color per surface.
    Solid,
    /// Hue encodes arg(f(x)): the classic complex-phase rainbow.
    Phase,
    /// Rainbow by surface height, each surface normalized to its own range.
    Height,
    /// Domain coloring: phase hue plus brightness rings at each doubling of |f|.
    Rings,
    /// 4D view: color encodes the paired component (Re<->Im, |f|<->arg) along the
    /// cyclic spectrum in `SPECTRUM_STOPS`, normalized to that component's range.
    /// With a single surface visible this shows all four dimensions at once.
    FourD,
}

impl ColorMode {
    fn next(self) -> Self {
        match self {
            ColorMode::Solid => ColorMode::Phase,
            ColorMode::Phase => ColorMode::Height,
            ColorMode::Height => ColorMode::Rings,
            ColorMode::Rings => ColorMode::FourD,
            ColorMode::FourD => ColorMode::Solid,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ColorMode::Solid => "solid",
            ColorMode::Phase => "phase",
            ColorMode::Height => "height",
            ColorMode::Rings => "rings",
            ColorMode::FourD => "4d",
        }
    }

    fn from_cli_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "solid" => Some(ColorMode::Solid),
            "phase" => Some(ColorMode::Phase),
            "height" => Some(ColorMode::Height),
            "rings" => Some(ColorMode::Rings),
            "4d" => Some(ColorMode::FourD),
            _ => None,
        }
    }

    fn uses_hue(self) -> bool {
        !matches!(self, ColorMode::Solid)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YScale {
    Linear,
    /// asinh(y): linear near zero, logarithmic far away; sign-preserving.
    Arsinh,
    /// sign(y) * log10(1 + |y|): stronger compression for huge poles.
    Log10,
}

impl YScale {
    fn next(self) -> Self {
        match self {
            YScale::Linear => YScale::Arsinh,
            YScale::Arsinh => YScale::Log10,
            YScale::Log10 => YScale::Linear,
        }
    }

    fn label(self) -> &'static str {
        match self {
            YScale::Linear => "linear",
            YScale::Arsinh => "arsinh",
            YScale::Log10 => "log10",
        }
    }

    fn apply(self, v: f64) -> f64 {
        match self {
            YScale::Linear => v,
            YScale::Arsinh => v.asinh(),
            YScale::Log10 => {
                if v >= 0.0 {
                    (1.0 + v).log10()
                } else {
                    -(1.0 - v).log10()
                }
            }
        }
    }

    fn invert(self, t: f64) -> f64 {
        match self {
            YScale::Linear => t,
            YScale::Arsinh => t.sinh(),
            YScale::Log10 => {
                if t >= 0.0 {
                    10f64.powf(t) - 1.0
                } else {
                    -(10f64.powf(-t) - 1.0)
                }
            }
        }
    }
}


fn eval_target(expr: &Expr, x: C, deriv_order: u8, h: f64) -> C {
    match deriv_order {
        0 => expr.eval(x),
        1 => {
            // Central difference along the real direction. Exact (up to O(h^2))
            // for holomorphic f; a directional derivative otherwise.
            let hc = C::new(h, 0.0);
            (expr.eval(x + hc) - expr.eval(x - hc)) / C::new(2.0 * h, 0.0)
        }
        _ => {
            let hc = C::new(h, 0.0);
            (expr.eval(x + hc) - expr.eval(x) * C::new(2.0, 0.0) + expr.eval(x - hc))
                / C::new(h * h, 0.0)
        }
    }
}

fn deriv_step(domain: Domain, deriv_order: u8) -> f64 {
    let span = domain.re.len().abs().max(domain.im.len().abs()).max(1e-9);
    match deriv_order {
        1 => span * DERIV_H1_REL,
        2 => span * DERIV_H2_REL,
        _ => 0.0,
    }
}

fn deriv_label(order: u8) -> &'static str {
    match order {
        0 => "f",
        1 => "f'",
        _ => "f''",
    }
}

#[allow(clippy::too_many_arguments)]
fn build_plot(
    expr: &Expr,
    domain: Domain,
    samples_per_axis: usize,
    visibility: SurfaceVisibility,
    shading: Shading,
    iso_lines_enabled: bool,
    iso_line_count: usize,
    deriv_order: u8,
    y_scale: YScale,
) -> PlotData {
    let n = samples_per_axis;
    assert!(n >= 2);
    let steps = n - 1;
    let h = deriv_step(domain, deriv_order);

    let mut samples = vec![Sample::invalid(); n * n];
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut v_min = [f64::INFINITY; 4];
    let mut v_max = [f64::NEG_INFINITY; 4];
    let mut finite_sample_count = 0;
    let mut visible_value_count = 0usize;

    for iz in 0..n {
        let im = lerp_f64(domain.im.min, domain.im.max, iz as f64 / steps as f64);
        for ix in 0..n {
            let re = lerp_f64(domain.re.min, domain.re.max, ix as f64 / steps as f64);
            let x = C::new(re, im);
            let f = eval_target(expr, x, deriv_order, h);
            let abs = f.norm();
            let valid = f.re.is_finite() && f.im.is_finite() && abs.is_finite();
            let idx = iz * n + ix;
            if valid {
                finite_sample_count += 1;
                let sample = Sample {
                    real: f.re,
                    imag: f.im,
                    abs,
                    arg: f.arg(),
                    valid: true,
                };
                samples[idx] = sample;
                for kind in SurfaceKind::ALL {
                    let tv = y_scale.apply(kind.raw_value(sample));
                    let k = kind.index();
                    v_min[k] = v_min[k].min(tv);
                    v_max[k] = v_max[k].max(tv);
                    if visibility.on(kind) {
                        y_min = y_min.min(tv);
                        y_max = y_max.max(tv);
                        visible_value_count += 1;
                    }
                }
            }
        }
    }

    if visible_value_count == 0 || !y_min.is_finite() || !y_max.is_finite() {
        y_min = -1.0;
        y_max = 1.0;
    }
    if (y_max - y_min).abs() < 1e-12 {
        let pad = (y_max.abs() * 0.1).max(1.0);
        y_min -= pad;
        y_max += pad;
    }

    let y = Range { min: y_min, max: y_max };

    // Every surface's range is needed up front: in the `4d` color mode a surface
    // is colored by its partner component, normalized to the partner's range.
    let value_ranges: [Option<Range>; 4] =
        std::array::from_fn(|k| finite_range(v_min[k], v_max[k]));

    let mut surfaces: [SurfaceData; 4] = Default::default();
    for kind in SurfaceKind::ALL {
        let k = kind.index();
        let value_range = value_ranges[k];
        let color_range = value_ranges[kind.partner().index()];
        let mut surface = SurfaceData {
            value_range,
            ..Default::default()
        };
        if visibility.on(kind) {
            let (meshes, vertex_infos) = build_surface_meshes(
                &samples,
                n,
                domain,
                y,
                kind,
                value_range,
                color_range,
                y_scale,
                shading,
            );
            surface.meshes = meshes;
            surface.vertex_infos = vertex_infos;
            if iso_lines_enabled {
                surface.iso_lines = build_iso_value_lines(
                    &samples,
                    n,
                    domain,
                    y,
                    value_range,
                    color_range,
                    kind,
                    y_scale,
                    iso_line_count,
                );
            }
        }
        surfaces[k] = surface;
    }

    let center_input = C::new(domain.re.mid(), domain.im.mid());
    let center_value = eval_target(expr, center_input, deriv_order, h);

    PlotData {
        surfaces,
        y,
        finite_sample_count,
        total_sample_count: n * n,
        samples_per_axis: n,
        samples,
        center_input,
        center_value,
    }
}

fn finite_range(min: f64, max: f64) -> Option<Range> {
    if min.is_finite() && max.is_finite() && (max - min).abs() > 1e-12 {
        Some(Range { min, max })
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SurfaceKind {
    Real,
    Imag,
    Abs,
    Arg,
}

impl SurfaceKind {
    const ALL: [SurfaceKind; 4] = [
        SurfaceKind::Real,
        SurfaceKind::Imag,
        SurfaceKind::Abs,
        SurfaceKind::Arg,
    ];

    fn index(self) -> usize {
        match self {
            SurfaceKind::Real => 0,
            SurfaceKind::Imag => 1,
            SurfaceKind::Abs => 2,
            SurfaceKind::Arg => 3,
        }
    }

    /// The component shown as color on this surface in the `4d` color mode:
    /// Re<->Im (Cartesian pair) and |f|<->arg (polar pair).
    fn partner(self) -> SurfaceKind {
        match self {
            SurfaceKind::Real => SurfaceKind::Imag,
            SurfaceKind::Imag => SurfaceKind::Real,
            SurfaceKind::Abs => SurfaceKind::Arg,
            SurfaceKind::Arg => SurfaceKind::Abs,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SurfaceKind::Real => "Re(f)",
            SurfaceKind::Imag => "Im(f)",
            SurfaceKind::Abs => "|f|",
            SurfaceKind::Arg => "arg(f)",
        }
    }

    fn from_cli_name(name: &str) -> Option<SurfaceKind> {
        match name.trim().to_ascii_lowercase().as_str() {
            "re" | "real" => Some(SurfaceKind::Real),
            "im" | "imag" => Some(SurfaceKind::Imag),
            "abs" | "mod" | "modulus" => Some(SurfaceKind::Abs),
            "arg" | "phase" => Some(SurfaceKind::Arg),
            _ => None,
        }
    }

    fn color(self, transparent: bool) -> Color {
        let alpha = if transparent { TRANSPARENT_ALPHA } else { 1.0 };
        match self {
            SurfaceKind::Real => Color::new(1.0, 0.04, 0.02, alpha),
            SurfaceKind::Imag => Color::new(0.08, 0.20, 1.0, alpha),
            SurfaceKind::Abs => Color::new(0.05, 0.70, 0.10, alpha),
            SurfaceKind::Arg => Color::new(1.0, 0.55, 0.0, alpha),
        }
    }

    fn iso_color(self, transparent: bool) -> Color {
        let alpha = if transparent { 0.88 } else { 1.0 };
        match self {
            SurfaceKind::Real => Color::new(0.55, 0.00, 0.00, alpha),
            SurfaceKind::Imag => Color::new(0.00, 0.03, 0.65, alpha),
            SurfaceKind::Abs => Color::new(0.00, 0.38, 0.00, alpha),
            SurfaceKind::Arg => Color::new(0.60, 0.30, 0.00, alpha),
        }
    }

    // Per-surface brightness factor so overlapping surfaces stay distinguishable
    // in the hue-based color modes.
    fn brightness(self) -> f32 {
        match self {
            SurfaceKind::Real => 1.0,
            SurfaceKind::Imag => 0.75,
            SurfaceKind::Abs => 0.50,
            SurfaceKind::Arg => 0.30,
        }
    }

    fn raw_value(self, s: Sample) -> f64 {
        match self {
            SurfaceKind::Real => s.real,
            SurfaceKind::Imag => s.imag,
            SurfaceKind::Abs => s.abs,
            SurfaceKind::Arg => s.arg,
        }
    }

    fn value(self, s: Sample, y_scale: YScale) -> f64 {
        y_scale.apply(self.raw_value(s))
    }
}

fn phase01(s: Sample) -> f32 {
    (((s.arg + std::f64::consts::PI) / std::f64::consts::TAU) as f32).clamp(0.0, 1.0)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn vertex_color(shading: Shading, kind: SurfaceKind, s: Sample, value01: f32, color01: f32) -> Color {
    let Shading {
        mode,
        hue_offset,
        transparent,
        colormap,
    } = shading;
    let alpha = if transparent { TRANSPARENT_ALPHA } else { 1.0 };
    match mode {
        ColorMode::Solid => kind.color(transparent),
        ColorMode::Phase => {
            let hue = phase01(s) + hue_offset;
            let v = 0.55 + 0.45 * kind.brightness();
            let (r, g, b) = hsv_to_rgb(hue, 0.85, v);
            Color::new(r, g, b, alpha)
        }
        ColorMode::Height => {
            // Blue (low) to red (high) rainbow over each surface's own range.
            let hue = (1.0 - value01.clamp(0.0, 1.0)) * 0.70 + hue_offset;
            let v = 0.62 + 0.38 * kind.brightness();
            let (r, g, b) = hsv_to_rgb(hue, 0.88, v);
            Color::new(r, g, b, alpha)
        }
        ColorMode::Rings => {
            let hue = phase01(s) + hue_offset;
            let band = if s.abs.is_finite() && s.abs > 0.0 {
                let l = s.abs.log2();
                (l - l.floor()) as f32
            } else {
                0.0
            };
            let v = (0.45 + 0.50 * band) * (0.62 + 0.38 * kind.brightness());
            let (r, g, b) = hsv_to_rgb(hue, 0.90, v);
            Color::new(r, g, b, alpha)
        }
        ColorMode::FourD => {
            // The paired component drives the color; the hue offset shifts the
            // (cyclic) map, so the animation sweeps the spectrum over the surface.
            let (r, g, b) = colormap.color(color01.clamp(0.0, 1.0) + hue_offset);
            Color::new(r, g, b, alpha)
        }
    }
}

fn surface_value01(value_range: Option<Range>, transformed_value: f64) -> f32 {
    match value_range {
        Some(r) => (((transformed_value - r.min) / r.len()) as f32).clamp(0.0, 1.0),
        None => 0.5,
    }
}

/// Normalized paired-component value of a sample, the color source of the `4d` mode.
fn sample_color01(kind: SurfaceKind, s: Sample, color_range: Option<Range>, y_scale: YScale) -> f32 {
    if !s.valid {
        return 0.0;
    }
    surface_value01(color_range, kind.partner().value(s, y_scale))
}

fn recolor_plot(plot: &mut PlotData, shading: Shading) {
    for kind in SurfaceKind::ALL {
        let surface = &mut plot.surfaces[kind.index()];
        for (mesh, infos) in surface.meshes.iter_mut().zip(surface.vertex_infos.iter()) {
            for (vertex, info) in mesh.vertices.iter_mut().zip(infos.iter()) {
                let s = plot.samples[info.sample as usize];
                vertex.color = vertex_color(shading, kind, s, info.value01, info.color01).into();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_surface_meshes(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    value_range: Option<Range>,
    color_range: Option<Range>,
    y_scale: YScale,
    shading: Shading,
) -> (Vec<Mesh>, Vec<Vec<VertexInfo>>) {
    let mut meshes = Vec::new();
    let mut vertex_infos = Vec::new();
    let tile_stride = MAX_MESH_SAMPLES_PER_AXIS - 1;
    let mut z0 = 0usize;

    while z0 < samples_per_axis - 1 {
        let z1 = (z0 + tile_stride).min(samples_per_axis - 1);
        let mut x0 = 0usize;
        while x0 < samples_per_axis - 1 {
            let x1 = (x0 + tile_stride).min(samples_per_axis - 1);
            let (mesh, infos) = build_surface_mesh_tile(
                samples,
                samples_per_axis,
                domain,
                y_range,
                kind,
                value_range,
                color_range,
                y_scale,
                shading,
                x0,
                x1,
                z0,
                z1,
            );
            meshes.push(mesh);
            vertex_infos.push(infos);
            x0 = x1;
        }
        z0 = z1;
    }

    (meshes, vertex_infos)
}

#[allow(clippy::too_many_arguments)]
fn build_surface_mesh_tile(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    value_range: Option<Range>,
    color_range: Option<Range>,
    y_scale: YScale,
    shading: Shading,
    x0: usize,
    x1: usize,
    z0: usize,
    z1: usize,
) -> (Mesh, Vec<VertexInfo>) {
    let n = samples_per_axis;
    let steps = n - 1;
    let tile_w = x1 - x0 + 1;
    let tile_h = z1 - z0 + 1;
    assert!(tile_w <= MAX_MESH_SAMPLES_PER_AXIS);
    assert!(tile_h <= MAX_MESH_SAMPLES_PER_AXIS);
    assert!(tile_w * tile_h <= u16::MAX as usize);

    let mut vertices = Vec::with_capacity(tile_w * tile_h);
    let mut infos = Vec::with_capacity(tile_w * tile_h);

    for lz in 0..tile_h {
        let iz = z0 + lz;
        let im_t = iz as f64 / steps as f64;
        let im = lerp_f64(domain.im.min, domain.im.max, im_t);
        for lx in 0..tile_w {
            let ix = x0 + lx;
            let re_t = ix as f64 / steps as f64;
            let re = lerp_f64(domain.re.min, domain.re.max, re_t);
            let sample_idx = iz * n + ix;
            let sample = samples[sample_idx];
            let value = if sample.valid {
                kind.value(sample, y_scale)
            } else {
                y_range.min
            };
            let value01 = surface_value01(value_range, value);
            let color01 = sample_color01(kind, sample, color_range, y_scale);
            let info = VertexInfo {
                sample: sample_idx as u32,
                value01,
                color01,
            };
            let color = vertex_color(shading, kind, sample, value01, color01);
            let pos = vec3(
                map_re_to_world(domain.re, re),
                map_y_to_world(y_range, value),
                map_im_to_world(domain.im, im),
            );
            vertices.push(Vertex::new2(pos, vec2(re_t as f32, im_t as f32), color));
            infos.push(info);
        }
    }

    let mut indices: Vec<u16> = Vec::with_capacity((tile_w - 1) * (tile_h - 1) * 6);
    for lz in 0..(tile_h - 1) {
        for lx in 0..(tile_w - 1) {
            let ga = (z0 + lz) * n + (x0 + lx);
            let gb = (z0 + lz) * n + (x0 + lx + 1);
            let gc = (z0 + lz + 1) * n + (x0 + lx + 1);
            let gd = (z0 + lz + 1) * n + (x0 + lx);
            if samples[ga].valid && samples[gb].valid && samples[gc].valid && samples[gd].valid {
                let a = lz * tile_w + lx;
                let b = lz * tile_w + lx + 1;
                let c = (lz + 1) * tile_w + lx + 1;
                let d = (lz + 1) * tile_w + lx;
                indices.extend_from_slice(&[
                    a as u16, b as u16, c as u16, a as u16, c as u16, d as u16,
                ]);
            }
        }
    }

    (
        Mesh {
            vertices,
            indices,
            texture: None,
        },
        infos,
    )
}

fn draw_meshes(meshes: &[Mesh]) {
    for mesh in meshes {
        draw_mesh(mesh);
    }
}

fn map_re_to_world(range: Range, re: f64) -> f32 {
    map_to_world(range, re, -DOMAIN_EXTENT, DOMAIN_EXTENT)
}

fn map_im_to_world(range: Range, im: f64) -> f32 {
    map_to_world(range, im, -DOMAIN_EXTENT, DOMAIN_EXTENT)
}

fn map_y_to_world(range: Range, y: f64) -> f32 {
    map_to_world(range, y, -Y_EXTENT, Y_EXTENT)
}

fn map_to_world(range: Range, value: f64, out_min: f32, out_max: f32) -> f32 {
    let t = ((value - range.min) / range.len()) as f32;
    out_min + (out_max - out_min) * t
}

fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[allow(clippy::too_many_arguments)]
fn sample_world_pos(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    y_scale: YScale,
    ix: usize,
    iz: usize,
) -> Option<Vec3> {
    let n = samples_per_axis;
    let steps = n - 1;
    let sample = samples[iz * n + ix];
    if !sample.valid {
        return None;
    }
    let re_t = ix as f64 / steps as f64;
    let im_t = iz as f64 / steps as f64;
    let re = lerp_f64(domain.re.min, domain.re.max, re_t);
    let im = lerp_f64(domain.im.min, domain.im.max, im_t);
    Some(vec3(
        map_re_to_world(domain.re, re),
        map_y_to_world(y_range, kind.value(sample, y_scale)),
        map_im_to_world(domain.im, im),
    ))
}

#[allow(clippy::too_many_arguments)]
fn draw_wireframe_surface(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    value_range: Option<Range>,
    color_range: Option<Range>,
    y_scale: YScale,
    shading: Shading,
) {
    let n = samples_per_axis;
    let solid_color = kind.color(shading.transparent);
    let color_of = |ix: usize, iz: usize| -> Color {
        if shading.mode == ColorMode::Solid {
            return solid_color;
        }
        let s = samples[iz * n + ix];
        let value01 = surface_value01(value_range, kind.value(s, y_scale));
        let color01 = sample_color01(kind, s, color_range, y_scale);
        vertex_color(shading, kind, s, value01, color01)
    };

    for iz in 0..n {
        for ix in 0..(n - 1) {
            if let (Some(a), Some(b)) = (
                sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix, iz),
                sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix + 1, iz),
            ) {
                draw_line_3d(a, b, color_of(ix, iz));
            }
        }
    }

    for iz in 0..(n - 1) {
        for ix in 0..n {
            if let (Some(a), Some(b)) = (
                sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix, iz),
                sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix, iz + 1),
            ) {
                draw_line_3d(a, b, color_of(ix, iz));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_iso_value_lines(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    value_range: Option<Range>,
    color_range: Option<Range>,
    kind: SurfaceKind,
    y_scale: YScale,
    iso_line_count: usize,
) -> Vec<IsoSegment> {
    let Some(value_range) = value_range else {
        return Vec::new();
    };

    if iso_line_count == 0 {
        return Vec::new();
    }

    let n = samples_per_axis;
    let mut segments = Vec::<IsoSegment>::new();
    let eps = (value_range.len().abs() * 1e-12).max(1e-12);
    let segment = |p: (Vec3, f32), q: (Vec3, f32)| IsoSegment {
        a: p.0,
        b: q.0,
        color01: 0.5 * (p.1 + q.1),
    };

    for iso_index in 1..=iso_line_count {
        let iso_value =
            value_range.min + value_range.len() * (iso_index as f64) / ((iso_line_count + 1) as f64);

        for iz in 0..(n - 1) {
            for ix in 0..(n - 1) {
                let ia = iz * n + ix;
                let ib = iz * n + ix + 1;
                let ic = (iz + 1) * n + ix + 1;
                let id = (iz + 1) * n + ix;

                let sa = samples[ia];
                let sb = samples[ib];
                let sc = samples[ic];
                let sd = samples[id];

                if !(sa.valid && sb.valid && sc.valid && sd.valid) {
                    continue;
                }

                let va = kind.value(sa, y_scale);
                let vb = kind.value(sb, y_scale);
                let vc = kind.value(sc, y_scale);
                let vd = kind.value(sd, y_scale);

                let cell_min = va.min(vb).min(vc).min(vd);
                let cell_max = va.max(vb).max(vc).max(vd);
                if iso_value < cell_min - eps || iso_value > cell_max + eps {
                    continue;
                }

                let Some(pa) = sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix, iz)
                else {
                    continue;
                };
                let Some(pb) =
                    sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix + 1, iz)
                else {
                    continue;
                };
                let Some(pc) =
                    sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix + 1, iz + 1)
                else {
                    continue;
                };
                let Some(pd) =
                    sample_world_pos(samples, n, domain, y_range, kind, y_scale, ix, iz + 1)
                else {
                    continue;
                };

                let pa = lift_iso_point(pa);
                let pb = lift_iso_point(pb);
                let pc = lift_iso_point(pc);
                let pd = lift_iso_point(pd);

                let ca = sample_color01(kind, sa, color_range, y_scale);
                let cb = sample_color01(kind, sb, color_range, y_scale);
                let cc = sample_color01(kind, sc, color_range, y_scale);
                let cd = sample_color01(kind, sd, color_range, y_scale);

                let mut points = Vec::<(Vec3, f32)>::with_capacity(4);
                push_iso_intersection(&mut points, (pa, ca), va, (pb, cb), vb, iso_value, eps);
                push_iso_intersection(&mut points, (pb, cb), vb, (pc, cc), vc, iso_value, eps);
                push_iso_intersection(&mut points, (pc, cc), vc, (pd, cd), vd, iso_value, eps);
                push_iso_intersection(&mut points, (pd, cd), vd, (pa, ca), va, iso_value, eps);

                match points.len() {
                    0 | 1 => {}
                    2 => segments.push(segment(points[0], points[1])),
                    3 => segments.push(segment(points[0], points[1])),
                    _ => {
                        // Ambiguous marching-squares saddle case. Use a center-value
                        // decider so the contour connectivity is stable across cells.
                        let center = 0.25 * (va + vb + vc + vd);
                        let a_high = va >= iso_value;
                        let center_high = center >= iso_value;
                        if center_high == a_high {
                            segments.push(segment(points[0], points[1]));
                            segments.push(segment(points[2], points[3]));
                        } else {
                            segments.push(segment(points[0], points[3]));
                            segments.push(segment(points[1], points[2]));
                        }
                    }
                }
            }
        }
    }

    segments
}

fn push_iso_intersection(
    points: &mut Vec<(Vec3, f32)>,
    p0: (Vec3, f32),
    v0: f64,
    p1: (Vec3, f32),
    v1: f64,
    iso_value: f64,
    eps: f64,
) {
    if let Some(t) = iso_crossing(v0, v1, iso_value, eps) {
        let point = p0.0 + (p1.0 - p0.0) * t;
        let color01 = p0.1 + (p1.1 - p0.1) * t;
        push_unique_iso_point(points, (point, color01));
    }
}

/// Where along the edge `v0 -> v1` (0 = start, 1 = end) the contour `iso_value` crosses.
fn iso_crossing(v0: f64, v1: f64, iso_value: f64, eps: f64) -> Option<f32> {
    let d0 = v0 - iso_value;
    let d1 = v1 - iso_value;

    if d0.abs() <= eps && d1.abs() <= eps {
        return None;
    }
    if d0.abs() <= eps {
        return Some(0.0);
    }
    if d1.abs() <= eps {
        return Some(1.0);
    }
    if (d0 > 0.0 && d1 < 0.0) || (d0 < 0.0 && d1 > 0.0) {
        Some((-d0 / (d1 - d0)) as f32)
    } else {
        None
    }
}

fn push_unique_iso_point(points: &mut Vec<(Vec3, f32)>, point: (Vec3, f32)) {
    const EPS2: f32 = 1e-10;
    if !points.iter().any(|(p, _)| (*p - point.0).length_squared() <= EPS2) {
        points.push(point);
    }
}

fn lift_iso_point(mut point: Vec3) -> Vec3 {
    point.y += ISO_LINE_LIFT;
    point
}

fn draw_iso_line_segments(
    segments: &[IsoSegment],
    kind: SurfaceKind,
    shading: Shading,
    on_filled_surface: bool,
) {
    let alpha = if shading.transparent { 0.88 } else { 1.0 };
    if shading.mode == ColorMode::FourD {
        if on_filled_surface {
            // Neutral dark lines read well on the color-mapped surface.
            let color = Color::new(0.12, 0.12, 0.12, alpha);
            for seg in segments {
                draw_line_3d(seg.a, seg.b, color);
            }
        } else {
            // Nothing underneath (wireframe mode): the contours themselves carry the
            // paired component's color so the 4th dimension stays visible.
            for seg in segments {
                let (r, g, b) = shading.colormap.color(seg.color01 + shading.hue_offset);
                draw_line_3d(seg.a, seg.b, Color::new(r, g, b, alpha));
            }
        }
        return;
    }

    let color = kind.iso_color(shading.transparent);
    for seg in segments {
        draw_line_3d(seg.a, seg.b, color);
    }
}

fn draw_axes_and_ticks(domain: &Domain, plot: &PlotData, y_scale: YScale) -> Vec<(Vec3, String, Color)> {
    let zero_y = if plot.y.contains(0.0) {
        map_y_to_world(plot.y, 0.0)
    } else {
        -Y_EXTENT
    };

    let x_min = -DOMAIN_EXTENT;
    let x_max = DOMAIN_EXTENT;
    let z_min = -DOMAIN_EXTENT;
    let z_max = DOMAIN_EXTENT;
    let y_min = -Y_EXTENT;
    let y_max = Y_EXTENT;

    let axis_color = Color::new(0.10, 0.10, 0.10, 1.0);
    let grid_color = Color::new(0.55, 0.55, 0.55, 0.55);
    let zero_color = Color::new(0.0, 0.0, 0.0, 1.0);
    let f_zero_color = Color::new(0.35, 0.35, 0.35, 0.75);

    // Domain rectangle at the visible zero-output level if possible, otherwise at bottom.
    draw_line_3d(vec3(x_min, zero_y, z_min), vec3(x_max, zero_y, z_min), axis_color);
    draw_line_3d(vec3(x_max, zero_y, z_min), vec3(x_max, zero_y, z_max), axis_color);
    draw_line_3d(vec3(x_max, zero_y, z_max), vec3(x_min, zero_y, z_max), axis_color);
    draw_line_3d(vec3(x_min, zero_y, z_max), vec3(x_min, zero_y, z_min), axis_color);

    // f(x)=0 plane/level, when it is within the current y range.
    if plot.y.contains(0.0) {
        for t in [0.25_f32, 0.5, 0.75] {
            let x = -DOMAIN_EXTENT + 2.0 * DOMAIN_EXTENT * t;
            let z = -DOMAIN_EXTENT + 2.0 * DOMAIN_EXTENT * t;
            draw_line_3d(vec3(x, zero_y, z_min), vec3(x, zero_y, z_max), f_zero_color);
            draw_line_3d(vec3(x_min, zero_y, z), vec3(x_max, zero_y, z), f_zero_color);
        }
    }

    // re(x)=0 and im(x)=0 axes.
    let x_axis_z = if domain.im.contains(0.0) {
        map_im_to_world(domain.im, 0.0)
    } else {
        z_min
    };
    let z_axis_x = if domain.re.contains(0.0) {
        map_re_to_world(domain.re, 0.0)
    } else {
        x_min
    };

    if domain.im.contains(0.0) {
        draw_line_3d(
            vec3(x_min, zero_y, x_axis_z),
            vec3(x_max, zero_y, x_axis_z),
            zero_color,
        );
    }
    if domain.re.contains(0.0) {
        draw_line_3d(
            vec3(z_axis_x, zero_y, z_min),
            vec3(z_axis_x, zero_y, z_max),
            zero_color,
        );
    }

    // Value axis for f(x): at the mathematical origin when available, otherwise at the front-left corner.
    let y_axis_x = if domain.re.contains(0.0) { z_axis_x } else { x_min };
    let y_axis_z = if domain.im.contains(0.0) { x_axis_z } else { z_min };
    draw_line_3d(
        vec3(y_axis_x, y_min, y_axis_z),
        vec3(y_axis_x, y_max, y_axis_z),
        zero_color,
    );

    // Tick marks.
    let tick = 0.035;
    for value in domain.re.ticks_5() {
        let x = map_re_to_world(domain.re, value);
        draw_line_3d(
            vec3(x, zero_y, x_axis_z - tick),
            vec3(x, zero_y, x_axis_z + tick),
            grid_color,
        );
    }
    for value in domain.im.ticks_5() {
        let z = map_im_to_world(domain.im, value);
        draw_line_3d(
            vec3(z_axis_x - tick, zero_y, z),
            vec3(z_axis_x + tick, zero_y, z),
            grid_color,
        );
    }
    for value in plot.y.ticks_5() {
        let y = map_y_to_world(plot.y, value);
        draw_line_3d(
            vec3(y_axis_x - tick, y, y_axis_z),
            vec3(y_axis_x + tick, y, y_axis_z),
            grid_color,
        );
    }

    // 3D-to-2D labels are drawn while default camera is still not active, using the camera matrix.
    let mut labels = Vec::<(Vec3, String, Color)>::new();
    for value in domain.re.ticks_5() {
        let x = map_re_to_world(domain.re, value);
        labels.push((
            vec3(x, zero_y, x_axis_z - 0.12),
            fmt_axis(value),
            Color::new(0.0, 0.0, 0.0, 1.0),
        ));
    }
    for value in domain.im.ticks_5() {
        let z = map_im_to_world(domain.im, value);
        labels.push((
            vec3(z_axis_x - 0.18, zero_y, z),
            fmt_axis(value),
            Color::new(0.0, 0.0, 0.0, 1.0),
        ));
    }
    for value in plot.y.ticks_5() {
        let y = map_y_to_world(plot.y, value);
        labels.push((
            vec3(y_axis_x + 0.10, y, y_axis_z),
            fmt_axis(y_scale.invert(value)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        ));
    }

    labels.push((
        vec3(DOMAIN_EXTENT + 0.10, zero_y, x_axis_z),
        "Re(x)".to_owned(),
        Color::new(0.0, 0.0, 0.0, 1.0),
    ));
    labels.push((
        vec3(z_axis_x, zero_y, DOMAIN_EXTENT + 0.10),
        "Im(x)".to_owned(),
        Color::new(0.0, 0.0, 0.0, 1.0),
    ));
    labels.push((
        vec3(y_axis_x + 0.10, Y_EXTENT + 0.03, y_axis_z),
        match y_scale {
            YScale::Linear => "Re(f), Im(f), |f|, arg(f)".to_owned(),
            _ => format!("Re(f), Im(f), |f|, arg(f) [{}]", y_scale.label()),
        },
        Color::new(0.0, 0.0, 0.0, 1.0),
    ));

    labels
}

fn draw_label_3d(camera: &Camera3D, world: Vec3, text: &str, color: Color, font_size: f32) {
    if let Some(p) = world_to_screen(camera, world) {
        if p.x >= -100.0
            && p.y >= -100.0
            && p.x <= screen_width() + 100.0
            && p.y <= screen_height() + 100.0
        {
            let dims = measure_text(text, None, font_size as u16, 1.0);
            draw_rectangle(
                p.x - 3.0,
                p.y - dims.height - 2.0,
                dims.width + 6.0,
                dims.height + 5.0,
                Color::new(1.0, 1.0, 1.0, 0.72),
            );
            draw_text(text, p.x, p.y, font_size, color);
        }
    }
}

fn world_to_screen(camera: &Camera3D, point: Vec3) -> Option<Vec2> {
    let clip = camera.matrix() * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    Some(vec2(
        (ndc.x + 1.0) * 0.5 * screen_width(),
        (1.0 - ndc.y) * 0.5 * screen_height(),
    ))
}

fn draw_hud(state: &AppState) {
    let mut y = 22.0;
    let font_size = 19.0;
    let line = 22.0;

    draw_text(
        &format!(
            "f(x) = {}{}",
            state.function_text,
            match state.deriv_order {
                0 => String::new(),
                o => format!("   [showing {}(x), numeric]", deriv_label(o)),
            }
        ),
        14.0,
        y,
        font_size,
        BLACK,
    );
    y += line;

    if let Some(plot) = &state.plot {
        draw_text(
            &format!(
                "re=[{}, {}]  im=[{}, {}]  y=[{}, {}]",
                fmt_axis(state.domain.re.min),
                fmt_axis(state.domain.re.max),
                fmt_axis(state.domain.im.min),
                fmt_axis(state.domain.im.max),
                fmt_axis(state.y_scale.invert(plot.y.min)),
                fmt_axis(state.y_scale.invert(plot.y.max))
            ),
            14.0,
            y,
            font_size,
            BLACK,
        );
        y += line;
        draw_text(
            &format!(
                "samples={}x{}  mode={}  alpha={}  color={}  yscale={}  anim={}  iso={}({})  [1]Re={} [2]Im={} [3]|f|={} [4]arg={}",
                state.samples_per_axis,
                state.samples_per_axis,
                if state.wireframe_mode { "wireframe" } else { "filled" },
                if state.transparent_surfaces { "0.5" } else { "1.0" },
                state.color_mode.label(),
                state.y_scale.label(),
                on_off(state.hue_anim),
                on_off(state.iso_lines_enabled),
                state.iso_line_count,
                on_off(state.show_real),
                on_off(state.show_imag),
                on_off(state.show_abs),
                on_off(state.show_arg),
            ),
            14.0,
            y,
            font_size,
            BLACK,
        );
        y += line;

        let g = deriv_label(state.deriv_order);
        let cv = plot.center_value;
        let probe = if cv.re.is_finite() && cv.im.is_finite() {
            format!(
                "at center x = {} + {}i:  {} = {} + {}i   |{}| = {}   arg({}) = {}",
                fmt_axis(plot.center_input.re),
                fmt_axis(plot.center_input.im),
                g,
                fmt_axis(cv.re),
                fmt_axis(cv.im),
                g,
                fmt_axis(cv.norm()),
                g,
                fmt_axis(cv.arg()),
            )
        } else {
            format!(
                "at center x = {} + {}i:  {} is not finite",
                fmt_axis(plot.center_input.re),
                fmt_axis(plot.center_input.im),
                g,
            )
        };
        draw_text(&probe, 14.0, y, font_size, BLACK);
        y += line;
    }

    let legend = match state.color_mode {
        ColorMode::Solid => "red: Re(f)   blue: Im(f)   green: |f|   orange: arg(f)",
        ColorMode::Phase => "hue: arg(f(x)) rainbow (-pi..pi)   brightness: Re > Im > |f| > arg",
        ColorMode::Height => "rainbow by height, per surface range (blue: low, red: high)",
        ColorMode::Rings => "hue: arg(f(x))   brightness rings: one ring per doubling of |f|",
        ColorMode::FourD => {
            "4d: height = surface value, color = its paired component (Re<->Im, |f|<->arg) mapped over that component's min..max:"
        }
    };
    draw_text(legend, 14.0, y, font_size, BLACK);
    y += line;

    if state.color_mode == ColorMode::FourD {
        if let Some(plot) = &state.plot {
            for kind in SurfaceKind::ALL {
                if !state.is_visible(kind) {
                    continue;
                }
                y = draw_color_bar_line(plot, kind, state.y_scale, state.shading(), y, font_size, line);
            }
        }
    }

    if !state.status.is_empty() {
        draw_text(&state.status, 14.0, y, font_size, DARKGRAY);
        y += line;
    }

    if state.show_help {
        y += 8.0;
        let help = [
            "F1 hide/show help, F11 toggle fullscreen",
            "R toggle auto-rotation, A/D yaw, W/S pitch, Q/E roll",
            "Z expand domain by 1.1, X shrink domain by 1.1",
            "H/L move real domain -/+ 10%, J/K move imag domain -/+ 10%",
            "N decrease samples by 10%, M increase samples by 10%",
            "1/2/3/4 toggle Re(f)/Im(f)/|f|/arg(f) visibility, 5 swap Re<->Im and |f|<->arg",
            "T toggle transparency, F toggle filled vs wireframe",
            "C cycle color mode: solid / phase / height / rings / 4d",
            "B toggle rainbow hue animation (phase/height/rings/4d modes)",
            "V cycle derivative: f / f' / f'' (numeric, on the fly)",
            "G cycle vertical scale: linear / arsinh / log10",
            "I toggle iso-value lines, U/O decrease/increase iso-line count",
            "0 reset domain, P save PNG, Esc quit",
        ];
        for item in help {
            draw_text(item, 14.0, y, font_size, BLACK);
            y += line;
        }
    }
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One HUD line of the `4d` color mode: "<surface> colored by <partner>: min [bar] max",
/// with true (un-transformed) range labels. Returns the y position of the next line.
fn draw_color_bar_line(
    plot: &PlotData,
    kind: SurfaceKind,
    y_scale: YScale,
    shading: Shading,
    y: f32,
    font_size: f32,
    line: f32,
) -> f32 {
    let partner = kind.partner();
    let prefix = format!("{} colored by {}:", kind.label(), partner.label());
    draw_text(&prefix, 14.0, y, font_size, BLACK);
    let mut x = 14.0 + measure_text(&prefix, None, font_size as u16, 1.0).width + 10.0;

    match plot.surface(partner).value_range {
        Some(range) => {
            let min_label = fmt_axis(y_scale.invert(range.min));
            draw_text(&min_label, x, y, font_size, BLACK);
            x += measure_text(&min_label, None, font_size as u16, 1.0).width + 8.0;

            let bar_width = 300.0;
            let bar_height = line - 6.0;
            let bar_top = y - bar_height + 3.0;
            draw_colormap_bar(shading.colormap, shading.hue_offset, x, bar_top, bar_width, bar_height);
            x += bar_width + 8.0;

            let max_label = fmt_axis(y_scale.invert(range.max));
            draw_text(&max_label, x, y, font_size, BLACK);
            x += measure_text(&max_label, None, font_size as u16, 1.0).width + 12.0;
            if y_scale != YScale::Linear {
                draw_text(&format!("[{} scale]", y_scale.label()), x, y, font_size, DARKGRAY);
            }
        }
        None => {
            draw_text("constant (no color range)", x, y, font_size, DARKGRAY);
        }
    }
    y + line
}

fn draw_colormap_bar(colormap: &ColorMap, hue_offset: f32, x: f32, y: f32, width: f32, height: f32) {
    let steps = 120;
    let step_width = width / steps as f32;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let (r, g, b) = colormap.color(t + hue_offset);
        // Slight overlap hides seams between the strips.
        draw_rectangle(x + i as f32 * step_width, y, step_width + 0.75, height, Color::new(r, g, b, 1.0));
    }
    draw_rectangle_lines(x, y, width, height, 1.5, Color::new(0.1, 0.1, 0.1, 1.0));
}

fn fmt_axis(value: f64) -> String {
    let v = if value.abs() < 1e-12 { 0.0 } else { value };
    if v == 0.0 {
        return "0".to_owned();
    }
    let av = v.abs();
    let s = if av >= 10_000.0 || av < 0.001 {
        format!("{v:.3e}")
    } else if av >= 100.0 {
        format!("{v:.2}")
    } else if av >= 10.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.4}")
    };
    trim_float_string(s)
}

fn trim_float_string(mut s: String) -> String {
    if let Some(e_pos) = s.find('e') {
        let exp = s.split_off(e_pos);
        while s.contains('.') && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s.push_str(&exp);
        s
    } else {
        while s.contains('.') && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

// -----------------------------
// Expression parser and evaluator
// -----------------------------

#[derive(Clone, Debug)]
enum Expr {
    Const(C),
    Var,
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Func(String, Vec<Expr>),
}

impl Expr {
    fn eval(&self, x: C) -> C {
        match self {
            Expr::Const(c) => *c,
            Expr::Var => x,
            Expr::Unary(op, arg) => match op {
                UnaryOp::Plus => arg.eval(x),
                UnaryOp::Minus => -arg.eval(x),
            },
            Expr::Binary(op, left, right) => {
                let a = left.eval(x);
                let b = right.eval(x);
                match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => a / b,
                    BinaryOp::Pow => a.powc(b),
                }
            }
            Expr::Func(name, args) => eval_function(name, args, x),
        }
    }
}

fn eval_function(name: &str, args: &[Expr], x: C) -> C {
    let one = C::new(1.0, 0.0);
    let two = C::new(2.0, 0.0);
    match name {
        "abs" | "mag" | "mod" | "norm" => C::new(args[0].eval(x).norm(), 0.0),
        "abs2" | "mag2" | "norm_sqr" | "normsq" => C::new(args[0].eval(x).norm_sqr(), 0.0),
        "l1_norm" | "l1" | "manhattan" | "taxicab" => C::new(args[0].eval(x).l1_norm(), 0.0),
        "arg" | "phase" => C::new(args[0].eval(x).arg(), 0.0),
        "to_polar" => {
            let (r, theta) = args[0].eval(x).to_polar();
            C::new(r, theta)
        }
        "polar_r" | "radius" => C::new(args[0].eval(x).to_polar().0, 0.0),
        "polar_theta" | "theta" => C::new(args[0].eval(x).to_polar().1, 0.0),
        "re" | "real" => C::new(args[0].eval(x).re, 0.0),
        "im" | "imag" => C::new(args[0].eval(x).im, 0.0),
        "conj" | "conjugate" => args[0].eval(x).conj(),
        "inv" => args[0].eval(x).inv(),
        "recip" | "inverse" => args[0].eval(x).recip(),
        "finv" => args[0].eval(x).finv(),
        "is_nan" | "isnan" => bool_to_complex(args[0].eval(x).is_nan()),
        "is_infinite" | "isinf" | "isinfinite" => bool_to_complex(args[0].eval(x).is_infinite()),
        "is_finite" | "isfinite" => bool_to_complex(args[0].eval(x).is_finite()),
        "is_normal" | "isnormal" => bool_to_complex(args[0].eval(x).is_normal()),
        "sgn" | "sign" | "signum" => {
            let z = args[0].eval(x);
            let n = z.norm();
            if n == 0.0 { C::new(0.0, 0.0) } else { z / C::new(n, 0.0) }
        }
        "cis" => C::cis(args[0].eval(x).re),
        "exp" => args[0].eval(x).exp(),
        "exp2" => args[0].eval(x).exp2(),
        "expf" => args[0].eval(x).expf(args[1].eval(x).re),
        "ln" => args[0].eval(x).ln(),
        "log" => {
            if args.len() == 1 {
                args[0].eval(x).ln()
            } else {
                args[0].eval(x).ln() / args[1].eval(x).ln()
            }
        }
        "log2" => args[0].eval(x).log2(),
        "log10" => args[0].eval(x).log10(),
        "sqrt" => args[0].eval(x).sqrt(),
        "cbrt" => args[0].eval(x).cbrt(),
        "sqr" | "square" => {
            let z = args[0].eval(x);
            z * z
        }
        "sin" => args[0].eval(x).sin(),
        "cos" => args[0].eval(x).cos(),
        "tan" => args[0].eval(x).tan(),
        "asin" | "arcsin" => args[0].eval(x).asin(),
        "acos" | "arccos" => args[0].eval(x).acos(),
        "atan" | "arctan" => args[0].eval(x).atan(),
        "sinh" => args[0].eval(x).sinh(),
        "cosh" => args[0].eval(x).cosh(),
        "tanh" => args[0].eval(x).tanh(),
        "asinh" | "arcsinh" => args[0].eval(x).asinh(),
        "acosh" | "arccosh" => args[0].eval(x).acosh(),
        "atanh" | "arctanh" => args[0].eval(x).atanh(),
        "sec" => one / args[0].eval(x).cos(),
        "csc" => one / args[0].eval(x).sin(),
        "cot" => one / args[0].eval(x).tan(),
        "sech" => one / args[0].eval(x).cosh(),
        "csch" => one / args[0].eval(x).sinh(),
        "coth" => one / args[0].eval(x).tanh(),
        "pow" | "powc" => args[0].eval(x).powc(args[1].eval(x)),
        "powf" => args[0].eval(x).powf(args[1].eval(x).re),
        "powi" => args[0].eval(x).powi(complex_to_i32(args[1].eval(x))),
        "powu" => args[0].eval(x).powu(complex_to_u32(args[1].eval(x))),
        "root" => args[0].eval(x).powc(one / args[1].eval(x)),
        "scale" => args[0].eval(x).scale(args[1].eval(x).re),
        "unscale" => args[0].eval(x).unscale(args[1].eval(x).re),
        "fdiv" => args[0].eval(x).fdiv(args[1].eval(x)),
        "complex" | "rect" | "new" => C::new(args[0].eval(x).re, args[1].eval(x).re),
        "polar" | "from_polar" => C::from_polar(args[0].eval(x).re, args[1].eval(x).re),
        "floor" => {
            let z = args[0].eval(x);
            C::new(z.re.floor(), z.im.floor())
        }
        "ceil" => {
            let z = args[0].eval(x);
            C::new(z.re.ceil(), z.im.ceil())
        }
        "round" => {
            let z = args[0].eval(x);
            C::new(z.re.round(), z.im.round())
        }
        "trunc" => {
            let z = args[0].eval(x);
            C::new(z.re.trunc(), z.im.trunc())
        }
        "frac" | "fract" => {
            let z = args[0].eval(x);
            C::new(z.re.fract(), z.im.fract())
        }
        "minabs" => {
            let a = args[0].eval(x);
            let b = args[1].eval(x);
            if a.norm() <= b.norm() { a } else { b }
        }
        "maxabs" => {
            let a = args[0].eval(x);
            let b = args[1].eval(x);
            if a.norm() >= b.norm() { a } else { b }
        }
        "avg" => (args[0].eval(x) + args[1].eval(x)) / two,
        "gamma" | "tgamma" => complex_gamma(args[0].eval(x)),
        "factorial" | "fact" => complex_gamma(args[0].eval(x) + one),
        _ => C::new(f64::NAN, f64::NAN), // unreachable after parser validation
    }
}

// Gamma function via the Lanczos approximation (g = 7, n = 9), with the
// reflection formula for Re(z) < 0.5.
fn complex_gamma(z: C) -> C {
    // Canonical published Lanczos coefficients; keep full literature precision.
    #[allow(clippy::excessive_precision)]
    const COEF: [f64; 9] = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.9843695780195716e-6,
        1.5056327351493116e-7,
    ];
    const G: f64 = 7.0;
    let pi = std::f64::consts::PI;
    if z.re < 0.5 {
        let pi_c = C::new(pi, 0.0);
        pi_c / ((pi_c * z).sin() * complex_gamma(C::new(1.0, 0.0) - z))
    } else {
        let z = z - C::new(1.0, 0.0);
        let mut x = C::new(COEF[0], 0.0);
        for (i, &c) in COEF.iter().enumerate().skip(1) {
            x += C::new(c, 0.0) / (z + C::new(i as f64, 0.0));
        }
        let t = z + C::new(G + 0.5, 0.0);
        C::new((2.0 * pi).sqrt(), 0.0) * t.powc(z + C::new(0.5, 0.0)) * (-t).exp() * x
    }
}

fn bool_to_complex(v: bool) -> C {
    C::new(if v { 1.0 } else { 0.0 }, 0.0)
}

fn complex_to_i32(z: C) -> i32 {
    if !z.re.is_finite() {
        0
    } else if z.re > i32::MAX as f64 {
        i32::MAX
    } else if z.re < i32::MIN as f64 {
        i32::MIN
    } else {
        z.re.round() as i32
    }
}

fn complex_to_u32(z: C) -> u32 {
    if !z.re.is_finite() || z.re <= 0.0 {
        0
    } else if z.re > u32::MAX as f64 {
        u32::MAX
    } else {
        z.re.round() as u32
    }
}

#[derive(Clone, Copy, Debug)]
enum UnaryOp {
    Plus,
    Minus,
}

#[derive(Clone, Copy, Debug)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    pos: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
    End,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse(input: &str) -> Result<Expr, String> {
        let tokens = tokenize(input)?;
        let mut parser = Parser { tokens, pos: 0 };
        let expr = parser.parse_expr()?;
        if !matches!(parser.current(), TokenKind::End) {
            return Err(parser.error_here("expected end of expression"));
        }
        Ok(expr)
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_sum()
    }

    fn parse_sum(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_product()?;
        loop {
            match self.current() {
                TokenKind::Plus => {
                    self.bump();
                    let rhs = self.parse_product()?;
                    expr = Expr::Binary(BinaryOp::Add, Box::new(expr), Box::new(rhs));
                }
                TokenKind::Minus => {
                    self.bump();
                    let rhs = self.parse_product()?;
                    expr = Expr::Binary(BinaryOp::Sub, Box::new(expr), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_product(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_unary()?;
        loop {
            match self.current() {
                TokenKind::Star => {
                    self.bump();
                    let rhs = self.parse_unary()?;
                    expr = Expr::Binary(BinaryOp::Mul, Box::new(expr), Box::new(rhs));
                }
                TokenKind::Slash => {
                    self.bump();
                    let rhs = self.parse_unary()?;
                    expr = Expr::Binary(BinaryOp::Div, Box::new(expr), Box::new(rhs));
                }
                _ if self.starts_implicit_factor() => {
                    let rhs = self.parse_unary()?;
                    expr = Expr::Binary(BinaryOp::Mul, Box::new(expr), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn starts_implicit_factor(&self) -> bool {
        matches!(
            self.current(),
            TokenKind::Number(_) | TokenKind::Ident(_) | TokenKind::LParen
        )
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.current() {
            TokenKind::Plus => {
                self.bump();
                Ok(Expr::Unary(UnaryOp::Plus, Box::new(self.parse_unary()?)))
            }
            TokenKind::Minus => {
                self.bump();
                Ok(Expr::Unary(UnaryOp::Minus, Box::new(self.parse_unary()?)))
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let base = self.parse_primary()?;
        if matches!(self.current(), TokenKind::Caret) {
            self.bump();
            let exponent = self.parse_unary()?;
            Ok(Expr::Binary(BinaryOp::Pow, Box::new(base), Box::new(exponent)))
        } else {
            Ok(base)
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.current().clone() {
            TokenKind::Number(value) => {
                self.bump();
                Ok(Expr::Const(C::new(value, 0.0)))
            }
            TokenKind::Ident(raw_name) => {
                self.bump();
                let name = normalize_ident(&raw_name);
                if matches!(self.current(), TokenKind::LParen) && is_function_name(&name) {
                    self.parse_function_call(name)
                } else {
                    constant_or_variable(&name)
                        .ok_or_else(|| self.error_prev(&format!("unknown identifier `{raw_name}`")))
                }
            }
            TokenKind::LParen => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect_rparen()?;
                Ok(expr)
            }
            _ => Err(self.error_here("expected a number, identifier, function call, or `(`")),
        }
    }

    fn parse_function_call(&mut self, name: String) -> Result<Expr, String> {
        self.expect_lparen()?;
        let mut args = Vec::new();
        if !matches!(self.current(), TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.current(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect_rparen()?;
        validate_function(&name, args.len()).map_err(|msg| self.error_prev(&msg))?;
        Ok(Expr::Func(name, args))
    }

    fn expect_lparen(&mut self) -> Result<(), String> {
        if matches!(self.current(), TokenKind::LParen) {
            self.bump();
            Ok(())
        } else {
            Err(self.error_here("expected `(`"))
        }
    }

    fn expect_rparen(&mut self) -> Result<(), String> {
        if matches!(self.current(), TokenKind::RParen) {
            self.bump();
            Ok(())
        } else {
            Err(self.error_here("expected `)`"))
        }
    }

    fn current(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn bump(&mut self) {
        if !matches!(self.current(), TokenKind::End) {
            self.pos += 1;
        }
    }

    fn error_here(&self, message: &str) -> String {
        format!("{message} at byte {}", self.tokens[self.pos].pos)
    }

    fn error_prev(&self, message: &str) -> String {
        let pos = self.pos.saturating_sub(1);
        format!("{message} near byte {}", self.tokens[pos].pos)
    }
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < chars.len() {
        let (byte_pos, ch) = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        let kind = match ch {
            '+' => {
                i += 1;
                TokenKind::Plus
            }
            '-' | '−' => {
                i += 1;
                TokenKind::Minus
            }
            '*' | '·' => {
                i += 1;
                TokenKind::Star
            }
            '/' | '÷' => {
                i += 1;
                TokenKind::Slash
            }
            '^' => {
                i += 1;
                TokenKind::Caret
            }
            '(' => {
                i += 1;
                TokenKind::LParen
            }
            ')' => {
                i += 1;
                TokenKind::RParen
            }
            ',' => {
                i += 1;
                TokenKind::Comma
            }
            _ if ch.is_ascii_digit() || (ch == '.' && peek_is_digit(&chars, i + 1)) => {
                let start_i = i;
                i = consume_number(&chars, i);
                let start_byte = chars[start_i].0;
                let end_byte = if i < chars.len() { chars[i].0 } else { input.len() };
                let text = &input[start_byte..end_byte];
                let value = text
                    .parse::<f64>()
                    .map_err(|_| format!("invalid number `{text}` at byte {start_byte}"))?;
                TokenKind::Number(value)
            }
            _ if is_ident_start(ch) => {
                let start_i = i;
                i += 1;
                while i < chars.len() && is_ident_continue(chars[i].1) {
                    i += 1;
                }
                let start_byte = chars[start_i].0;
                let end_byte = if i < chars.len() { chars[i].0 } else { input.len() };
                TokenKind::Ident(input[start_byte..end_byte].to_owned())
            }
            _ => return Err(format!("unexpected character `{ch}` at byte {byte_pos}")),
        };
        tokens.push(Token { kind, pos: byte_pos });
    }

    tokens.push(Token {
        kind: TokenKind::End,
        pos: input.len(),
    });
    Ok(tokens)
}

fn consume_number(chars: &[(usize, char)], mut i: usize) -> usize {
    while i < chars.len() && chars[i].1.is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i].1 == '.' {
        i += 1;
        while i < chars.len() && chars[i].1.is_ascii_digit() {
            i += 1;
        }
    }
    if i < chars.len() && (chars[i].1 == 'e' || chars[i].1 == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j].1 == '+' || chars[j].1 == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].1.is_ascii_digit() {
            i = j + 1;
            while i < chars.len() && chars[i].1.is_ascii_digit() {
                i += 1;
            }
        }
    }
    i
}

fn peek_is_digit(chars: &[(usize, char)], i: usize) -> bool {
    i < chars.len() && chars[i].1.is_ascii_digit()
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch == 'π' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch == 'π' || ch.is_alphanumeric()
}

fn normalize_ident(name: &str) -> String {
    match name {
        "π" => "pi".to_owned(),
        _ => name.to_ascii_lowercase(),
    }
}

fn constant_or_variable(name: &str) -> Option<Expr> {
    match name {
        "x" | "z" => Some(Expr::Var),
        "i" | "j" => Some(Expr::Const(C::new(0.0, 1.0))),
        "pi" => Some(Expr::Const(C::new(std::f64::consts::PI, 0.0))),
        "tau" => Some(Expr::Const(C::new(std::f64::consts::TAU, 0.0))),
        "e" => Some(Expr::Const(C::new(std::f64::consts::E, 0.0))),
        "inf" | "infinity" => Some(Expr::Const(C::new(f64::INFINITY, 0.0))),
        "nan" => Some(Expr::Const(C::new(f64::NAN, 0.0))),
        _ => None,
    }
}


fn is_function_name(name: &str) -> bool {
    matches!(
        name,
        "abs" | "mag"
            | "mod"
            | "norm"
            | "abs2"
            | "mag2"
            | "norm_sqr"
            | "normsq"
            | "l1_norm"
            | "l1"
            | "manhattan"
            | "taxicab"
            | "arg"
            | "phase"
            | "to_polar"
            | "polar_r"
            | "radius"
            | "polar_theta"
            | "theta"
            | "re"
            | "real"
            | "im"
            | "imag"
            | "conj"
            | "conjugate"
            | "recip"
            | "inverse"
            | "inv"
            | "finv"
            | "is_nan"
            | "isnan"
            | "is_infinite"
            | "isinf"
            | "isinfinite"
            | "is_finite"
            | "isfinite"
            | "is_normal"
            | "isnormal"
            | "sgn"
            | "sign"
            | "signum"
            | "cis"
            | "exp"
            | "exp2"
            | "expf"
            | "ln"
            | "log"
            | "log2"
            | "log10"
            | "sqrt"
            | "cbrt"
            | "sqr"
            | "square"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "arcsin"
            | "acos"
            | "arccos"
            | "atan"
            | "arctan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "arcsinh"
            | "acosh"
            | "arccosh"
            | "atanh"
            | "arctanh"
            | "sec"
            | "csc"
            | "cot"
            | "sech"
            | "csch"
            | "coth"
            | "pow"
            | "powc"
            | "powf"
            | "powi"
            | "powu"
            | "root"
            | "scale"
            | "unscale"
            | "fdiv"
            | "complex"
            | "rect"
            | "new"
            | "polar"
            | "from_polar"
            | "floor"
            | "ceil"
            | "round"
            | "trunc"
            | "frac"
            | "fract"
            | "minabs"
            | "maxabs"
            | "avg"
            | "gamma"
            | "tgamma"
            | "factorial"
            | "fact"
    )
}

fn validate_function(name: &str, arity: usize) -> Result<(), String> {
    let ok = match name {
        "abs" | "mag" | "mod" | "norm" | "abs2" | "mag2" | "norm_sqr" | "normsq"
        | "l1_norm" | "l1" | "manhattan" | "taxicab" | "arg" | "phase" | "to_polar"
        | "polar_r" | "radius" | "polar_theta" | "theta" | "re" | "real" | "im"
        | "imag" | "conj" | "conjugate" | "recip" | "inverse" | "inv" | "finv" | "is_nan"
        | "isnan" | "is_infinite" | "isinf" | "isinfinite" | "is_finite" | "isfinite"
        | "is_normal" | "isnormal" | "sgn" | "sign" | "signum" | "cis" | "exp" | "exp2"
        | "ln" | "log2" | "log10" | "sqrt" | "cbrt" | "sqr" | "square" | "sin" | "cos"
        | "tan" | "asin" | "arcsin" | "acos" | "arccos" | "atan" | "arctan" | "sinh"
        | "cosh" | "tanh" | "asinh" | "arcsinh" | "acosh" | "arccosh" | "atanh"
        | "arctanh" | "sec" | "csc" | "cot" | "sech" | "csch" | "coth" | "floor"
        | "ceil" | "round" | "trunc" | "frac" | "fract" | "gamma" | "tgamma"
        | "factorial" | "fact" => arity == 1,
        "pow" | "powc" | "powf" | "powi" | "powu" | "root" | "scale" | "unscale"
        | "fdiv" | "complex" | "rect" | "new" | "polar" | "from_polar" | "minabs"
        | "maxabs" | "avg" | "expf" => arity == 2,
        "log" => arity == 1 || arity == 2,
        _ => return Err(format!("unknown function `{name}`")),
    };

    if ok {
        Ok(())
    } else {
        Err(format!("wrong number of arguments for `{name}`: got {arity}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_str(input: &str, x: C) -> C {
        Parser::parse(input).expect("parse failed").eval(x)
    }

    fn assert_close(actual: C, expected: C, tol: f64) {
        assert!(
            (actual - expected).norm() <= tol,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn parses_operator_precedence() {
        assert_close(eval_str("2+3*4", C::new(0.0, 0.0)), C::new(14.0, 0.0), 1e-12);
        assert_close(eval_str("(2+3)*4", C::new(0.0, 0.0)), C::new(20.0, 0.0), 1e-12);
        assert_close(eval_str("2^3^2", C::new(0.0, 0.0)), C::new(512.0, 0.0), 1e-9);
        assert_close(eval_str("-2^2", C::new(0.0, 0.0)), C::new(-4.0, 0.0), 1e-12);
        assert_close(eval_str("6/3/2", C::new(0.0, 0.0)), C::new(1.0, 0.0), 1e-12);
    }

    #[test]
    fn parses_implicit_multiplication() {
        let x = C::new(3.0, 0.0);
        assert_close(eval_str("2x", x), C::new(6.0, 0.0), 1e-12);
        assert_close(eval_str("(x+1)(x-1)", x), C::new(8.0, 0.0), 1e-12);
        assert_close(eval_str("2sin(0)x", x), C::new(0.0, 0.0), 1e-12);
        assert_close(eval_str("3(2)", C::new(0.0, 0.0)), C::new(6.0, 0.0), 1e-12);
    }

    #[test]
    fn parses_constants_and_variables() {
        let x = C::new(1.5, -0.5);
        assert_close(eval_str("x", x), x, 0.0);
        assert_close(eval_str("z", x), x, 0.0);
        assert_close(eval_str("i*j", x), C::new(-1.0, 0.0), 1e-12);
        assert_close(eval_str("pi", x), C::new(std::f64::consts::PI, 0.0), 1e-12);
        assert_close(eval_str("e", x), C::new(std::f64::consts::E, 0.0), 1e-12);
    }

    #[test]
    fn rejects_invalid_expressions() {
        assert!(Parser::parse("").is_err());
        assert!(Parser::parse("2+").is_err());
        assert!(Parser::parse("sin()").is_err());
        assert!(Parser::parse("sin(x,x)").is_err());
        assert!(Parser::parse("unknownfn(x)").is_err());
        assert!(Parser::parse("bogus").is_err());
        assert!(Parser::parse("(x").is_err());
        assert!(Parser::parse("x)").is_err());
    }

    #[test]
    fn evaluates_complex_identities() {
        let x = C::new(0.7, 0.3);
        // e^(i*pi) = -1
        assert_close(eval_str("exp(i*pi)", x), C::new(-1.0, 0.0), 1e-12);
        // sin^2 + cos^2 = 1 holds for complex arguments.
        assert_close(eval_str("sin(x)^2+cos(x)^2", x), C::new(1.0, 0.0), 1e-9);
        // ln(exp(x)) = x within the principal branch.
        assert_close(eval_str("ln(exp(x))", x), x, 1e-12);
        assert_close(eval_str("conj(x)", x), C::new(0.7, -0.3), 1e-12);
        assert_close(eval_str("re(x)+i*im(x)", x), x, 1e-12);
        assert_close(eval_str("abs(3+4i)", x), C::new(5.0, 0.0), 1e-12);
    }

    #[test]
    fn evaluates_gamma() {
        let x = C::new(0.0, 0.0);
        // gamma(n) = (n-1)!
        assert_close(eval_str("gamma(5)", x), C::new(24.0, 0.0), 1e-9);
        assert_close(eval_str("gamma(1)", x), C::new(1.0, 0.0), 1e-12);
        // gamma(1/2) = sqrt(pi)
        assert_close(
            eval_str("gamma(0.5)", x),
            C::new(std::f64::consts::PI.sqrt(), 0.0),
            1e-10,
        );
        // Reflection-formula branch: gamma(-0.5) = -2*sqrt(pi)
        assert_close(
            eval_str("gamma(-0.5)", x),
            C::new(-2.0 * std::f64::consts::PI.sqrt(), 0.0),
            1e-9,
        );
        // factorial(n) = gamma(n+1)
        assert_close(eval_str("factorial(4)", x), C::new(24.0, 0.0), 1e-9);
        // |gamma(i)|^2 = pi / sinh(pi)
        let g_i = eval_str("gamma(i)", x);
        let expected = std::f64::consts::PI / std::f64::consts::PI.sinh();
        assert!((g_i.norm_sqr() - expected).abs() < 1e-10);
    }

    #[test]
    fn numeric_derivatives_match_analytic() {
        let expr = Parser::parse("x^3").unwrap();
        let domain = Domain {
            re: Range::new(-2.0, 2.0).unwrap(),
            im: Range::new(-2.0, 2.0).unwrap(),
        };
        let x = C::new(1.0, 1.0);
        let h1 = deriv_step(domain, 1);
        let h2 = deriv_step(domain, 2);
        // d/dx x^3 = 3x^2 -> 3(1+i)^2 = 6i
        assert_close(eval_target(&expr, x, 1, h1), C::new(0.0, 6.0), 1e-6);
        // d2/dx2 x^3 = 6x -> 6+6i
        assert_close(eval_target(&expr, x, 2, h2), C::new(6.0, 6.0), 1e-4);

        let expr = Parser::parse("exp(x)").unwrap();
        let x = C::new(0.5, -0.25);
        let expected = x.exp();
        assert_close(eval_target(&expr, x, 1, h1), expected, 1e-6);
        assert_close(eval_target(&expr, x, 2, h2), expected, 1e-4);
    }

    #[test]
    fn y_scale_roundtrips() {
        for scale in [YScale::Linear, YScale::Arsinh, YScale::Log10] {
            for v in [-1e6, -12.5, -1.0, -1e-9, 0.0, 1e-9, 0.5, 3.0, 1e8] {
                let t = scale.apply(v);
                assert!(t.is_finite());
                let back = scale.invert(t);
                let tol = 1e-9 * v.abs().max(1.0);
                assert!(
                    (back - v).abs() <= tol,
                    "{} roundtrip failed for {v}: got {back}",
                    scale.label()
                );
            }
            // Monotonicity and sign preservation.
            assert!(scale.apply(-2.0) < scale.apply(-1.0));
            assert!(scale.apply(1.0) < scale.apply(2.0));
            assert_eq!(scale.apply(0.0), 0.0);
        }
    }

    #[test]
    fn hsv_to_rgb_is_sane() {
        let (r, g, b) = hsv_to_rgb(0.0, 1.0, 1.0);
        assert!((r - 1.0).abs() < 1e-6 && g.abs() < 1e-6 && b.abs() < 1e-6);
        let (r, g, b) = hsv_to_rgb(1.0 / 3.0, 1.0, 1.0);
        assert!(r.abs() < 1e-6 && (g - 1.0).abs() < 1e-6 && b.abs() < 1e-6);
        // Hue wraps around.
        let a = hsv_to_rgb(0.25, 0.8, 0.9);
        let b2 = hsv_to_rgb(1.25, 0.8, 0.9);
        assert!((a.0 - b2.0).abs() < 1e-5 && (a.1 - b2.1).abs() < 1e-5 && (a.2 - b2.2).abs() < 1e-5);
    }

    #[test]
    fn default_colormap_hits_stops_and_wraps_seamlessly() {
        let close = |a: (f32, f32, f32), b: (f32, f32, f32), tol: f32| {
            (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol && (a.2 - b.2).abs() <= tol
        };
        let map = ColorMap::default();
        let segments = (map.stops.len() - 1) as f32;
        // Every stop is reproduced exactly at its position, including the last at t = 1.
        for (i, stop) in map.stops.iter().enumerate() {
            let t = i as f32 / segments;
            assert!(close(map.color(t), *stop, 1e-5), "stop {i} mismatch");
        }
        // Gray ends, black just above the minimum, white just below the maximum.
        assert!(close(map.color(0.0), (0.5, 0.5, 0.5), 1e-6));
        assert!(close(map.color(1.0), (0.5, 0.5, 0.5), 1e-6));
        assert!(close(map.color(1.0 / segments), (0.0, 0.0, 0.0), 1e-5));
        assert!(close(map.color(11.0 / segments), (1.0, 1.0, 1.0), 1e-5));
        // Cyclic and continuous across the wrap; in range everywhere.
        assert!(close(map.color(0.999), map.color(-0.001), 1e-4));
        assert!(close(map.color(0.37), map.color(1.37), 1e-4));
        for i in 0..=1000 {
            let (r, g, b) = map.color(i as f32 / 1000.0);
            for c in [r, g, b] {
                assert!((0.0..=1.0).contains(&c), "component {c} out of range at {i}");
            }
        }
        // Non-finite input does not panic and yields a valid color.
        let (r, g, b) = map.color(f32::NAN);
        assert!(r.is_finite() && g.is_finite() && b.is_finite());
    }

    #[test]
    fn colormap_spec_parses_and_roundtrips() {
        // The default map survives a spec round trip (this is what `--help` prints).
        let default = ColorMap::default();
        assert_eq!(ColorMap::parse(&default.to_spec()).unwrap(), default);

        // Lenient separators: missing comma between stops, whitespace, newlines.
        let spec = "12,(.5,.5,.5),(0,0,0),(0,.5,1),(0,0,1),(0,1,1),(0,1,0),\n(1,1,0), (1,.5,0) (1,0,0),(1,0,.5),(1,1,1)(.5,.5,.5)";
        let map = ColorMap::parse(spec).unwrap();
        assert_eq!(map.stops.len(), 12);
        assert_eq!(map.stops[0], (0.5, 0.5, 0.5));
        assert_eq!(map.stops[2], (0.0, 0.5, 1.0));
        assert_eq!(map.stops[11], (0.5, 0.5, 0.5));
        // Stops are evenly spaced: stop k sits at t = k / (N - 1).
        let (r, g, b) = map.color(2.0 / 11.0);
        assert!(r.abs() < 1e-5 && (g - 0.5).abs() < 1e-5 && (b - 1.0).abs() < 1e-5);
        // Midway between black and (0,.5,1).
        let (r, g, b) = map.color(1.5 / 11.0);
        assert!(r.abs() < 1e-5 && (g - 0.25).abs() < 1e-5 && (b - 0.5).abs() < 1e-5);

        // Two stops is the minimum: a plain gradient from the first to the last stop.
        let map = ColorMap::parse("2,(0,0,0),(1,1,1)").unwrap();
        assert_eq!(map.color(0.5), (0.5, 0.5, 0.5));
        assert_eq!(map.color(0.0), (0.0, 0.0, 0.0));
        // The maximum of the colored component (color01 == 1 exactly) is the LAST stop,
        // not a wrap-around to the first one; only out-of-range values wrap.
        assert_eq!(map.color(1.0), (1.0, 1.0, 1.0));
        let ramp = ColorMap::parse("3,(0,0,0),(1,0,0),(1,1,1)").unwrap();
        assert_eq!(ramp.color(1.0), (1.0, 1.0, 1.0));
        assert_eq!(ramp.color(0.5), (1.0, 0.0, 0.0));
        assert_eq!(ramp.color(1.5), (1.0, 0.0, 0.0));
        assert_eq!(ramp.color(-0.5), (1.0, 0.0, 0.0));
        assert_eq!(ramp.color(2.0), (0.0, 0.0, 0.0));

        // Errors: count mismatch, too few stops, bad numbers, out-of-range, garbage.
        assert!(ColorMap::parse("3,(0,0,0),(1,1,1)").is_err());
        assert!(ColorMap::parse("2,(0,0,0),(1,1,1),(0,0,0)").is_err());
        assert!(ColorMap::parse("1,(0,0,0)").is_err());
        assert!(ColorMap::parse("2,(0,0),(1,1,1)").is_err());
        assert!(ColorMap::parse("2,(0,0,x),(1,1,1)").is_err());
        assert!(ColorMap::parse("2,(0,0,2),(1,1,1)").is_err());
        assert!(ColorMap::parse("2,(0,0,-0.1),(1,1,1)").is_err());
        assert!(ColorMap::parse("2,(0,0,0),(1,1,1").is_err());
        assert!(ColorMap::parse("(0,0,0),(1,1,1)").is_err());
        assert!(ColorMap::parse("").is_err());
        assert!(ColorMap::parse("two,(0,0,0),(1,1,1)").is_err());
        // Absurd counts are plain errors, never allocation failures.
        assert!(ColorMap::parse("18446744073709551615,(0,0,0),(1,1,1)").is_err());
        assert!(ColorMap::parse("100000000000,(0,0,0),(1,1,1)").is_err());
        assert!(ColorMap::parse("99999999999999999999999,(0,0,0),(1,1,1)").is_err());

        // `@FILE` loads the spec from a file.
        let path = std::env::temp_dir().join(format!(
            "complex_surface_viewer_colormap_{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "3,\n(0,0,0),\n(1,0,0),\n(1,1,1)\n").unwrap();
        let arg = format!("@{}", path.display());
        let loaded = ColorMap::from_cli_arg(&arg).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.stops, vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 1.0)]);
        assert!(ColorMap::from_cli_arg("@/nonexistent/colormap.txt").is_err());
    }

    #[test]
    fn surface_partners_pair_cartesian_and_polar_components() {
        assert_eq!(SurfaceKind::Real.partner(), SurfaceKind::Imag);
        assert_eq!(SurfaceKind::Imag.partner(), SurfaceKind::Real);
        assert_eq!(SurfaceKind::Abs.partner(), SurfaceKind::Arg);
        assert_eq!(SurfaceKind::Arg.partner(), SurfaceKind::Abs);
        for kind in SurfaceKind::ALL {
            assert_eq!(kind.partner().partner(), kind);
        }
    }

    #[test]
    fn color_mode_cycle_visits_every_mode_once() {
        let mut seen = vec![ColorMode::Solid];
        let mut mode = ColorMode::Solid.next();
        while mode != ColorMode::Solid {
            assert!(!seen.contains(&mode));
            seen.push(mode);
            mode = mode.next();
        }
        assert_eq!(seen.len(), 5);
        assert!(seen.contains(&ColorMode::FourD));
        assert!(ColorMode::FourD.uses_hue());
        assert_eq!(ColorMode::from_cli_name("4d"), Some(ColorMode::FourD));
        assert_eq!(ColorMode::from_cli_name("RINGS"), Some(ColorMode::Rings));
        assert_eq!(ColorMode::from_cli_name("nope"), None);
    }

    fn shading(mode: ColorMode, transparent: bool, colormap: &ColorMap) -> Shading<'_> {
        Shading {
            mode,
            hue_offset: 0.0,
            transparent,
            colormap,
        }
    }

    #[test]
    fn four_d_mode_colors_surface_by_partner_component() {
        // f(x) = x: Re surface height is re(x), its color must follow im(x).
        let expr = Parser::parse("x").unwrap();
        let domain = Domain {
            re: Range::new(-1.0, 1.0).unwrap(),
            im: Range::new(-3.0, 5.0).unwrap(),
        };
        let visibility = SurfaceVisibility::from_cli_list("re").unwrap();
        let colormap = ColorMap::parse("3,(0,0,0),(1,0,0),(1,1,1)").unwrap();
        let n = 9;
        let plot = build_plot(
            &expr,
            domain,
            n,
            visibility,
            shading(ColorMode::FourD, false, &colormap),
            false,
            0,
            0,
            YScale::Linear,
        );

        // Only the requested surface is built, but every range is known.
        assert!(!plot.surface(SurfaceKind::Real).meshes.is_empty());
        assert!(plot.surface(SurfaceKind::Imag).meshes.is_empty());
        let im_range = plot.surface(SurfaceKind::Imag).value_range.unwrap();
        assert!((im_range.min - -3.0).abs() < 1e-12 && (im_range.max - 5.0).abs() < 1e-12);

        let surface = plot.surface(SurfaceKind::Real);
        let mesh = &surface.meshes[0];
        let infos = &surface.vertex_infos[0];
        assert_eq!(mesh.vertices.len(), infos.len());
        assert_eq!(mesh.vertices.len(), n * n);
        for (row, chunk) in infos.chunks(n).enumerate() {
            // Rows share im(x), so color01 is constant along a row and grows with it.
            let expected = row as f32 / (n - 1) as f32;
            for info in chunk {
                assert!((info.color01 - expected).abs() < 1e-5, "row {row}: {}", info.color01);
            }
        }
        // Vertex colors come from the custom map applied to color01 (opaque).
        for (vertex, info) in mesh.vertices.iter().zip(infos.iter()) {
            let (r, g, b) = colormap.color(info.color01);
            let expected: [u8; 4] = Color::new(r, g, b, 1.0).into();
            assert_eq!(vertex.color, expected);
        }
        // The vertex color of the `4d` mode depends only on the partner component.
        let s = plot.samples[0];
        let a = vertex_color(shading(ColorMode::FourD, false, &colormap), SurfaceKind::Real, s, 0.1, 0.25);
        let b = vertex_color(shading(ColorMode::FourD, false, &colormap), SurfaceKind::Abs, s, 0.9, 0.25);
        assert_eq!(a.r, b.r);
        assert_eq!(a.g, b.g);
        assert_eq!(a.b, b.b);
        // color01 = 0.25 is halfway between black and red in this map.
        assert!((a.r - 0.5).abs() < 1e-6 && a.g.abs() < 1e-6 && a.b.abs() < 1e-6);
        // Transparency applies to the new mode as well.
        let t = vertex_color(shading(ColorMode::FourD, true, &colormap), SurfaceKind::Real, s, 0.1, 0.25);
        assert!((t.a - TRANSPARENT_ALPHA).abs() < 1e-6);
    }

    #[test]
    fn existing_color_modes_ignore_color01_and_colormap() {
        let s = Sample {
            real: 0.3,
            imag: -0.8,
            abs: 0.85,
            arg: -1.2,
            valid: true,
        };
        let default_map = ColorMap::default();
        let other_map = ColorMap::parse("2,(0,0,0),(1,1,1)").unwrap();
        for mode in [ColorMode::Solid, ColorMode::Phase, ColorMode::Height, ColorMode::Rings] {
            let a = vertex_color(shading(mode, false, &default_map), SurfaceKind::Imag, s, 0.4, 0.0);
            let b = vertex_color(shading(mode, false, &other_map), SurfaceKind::Imag, s, 0.4, 1.0);
            assert_eq!((a.r, a.g, a.b, a.a), (b.r, b.g, b.b, b.a), "{}", mode.label());
        }
        // Solid mode still yields the fixed per-surface colors.
        let solid = vertex_color(shading(ColorMode::Solid, false, &default_map), SurfaceKind::Real, s, 0.4, 0.0);
        let expected = SurfaceKind::Real.color(false);
        assert_eq!((solid.r, solid.g, solid.b, solid.a), (expected.r, expected.g, expected.b, expected.a));
    }

    #[test]
    fn parses_cli_options() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let cli = parse_cli(&args(&["x^2"])).unwrap();
        assert_eq!(cli.function, "x^2");
        assert_eq!(cli.color_mode, ColorMode::Solid);
        assert_eq!(cli.visibility, SurfaceVisibility::default());
        assert_eq!(cli.colormap, ColorMap::default());
        assert!(cli.screenshot.is_none());
        assert_eq!((cli.re.min, cli.re.max, cli.im.min, cli.im.max), (-2.0, 2.0, -2.0, 2.0));

        let cli = parse_cli(&args(&[
            "--color=4d",
            "--show=im,arg",
            "--colormap=3,(0,0,0),(1,0,0),(1,1,1)",
            "--screenshot=out.png",
            "-x",
            "-1",
            "1.5",
            "-2",
            "3",
        ]))
        .unwrap();
        assert_eq!(cli.function, "-x");
        assert_eq!(cli.color_mode, ColorMode::FourD);
        assert_eq!(
            cli.visibility,
            SurfaceVisibility {
                show_real: false,
                show_imag: true,
                show_abs: false,
                show_arg: true,
            }
        );
        assert_eq!(cli.colormap.stops, vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 1.0)]);
        assert_eq!(cli.screenshot.as_deref(), Some("out.png"));
        assert_eq!((cli.re.min, cli.re.max, cli.im.min, cli.im.max), (-1.0, 1.5, -2.0, 3.0));

        assert!(parse_cli(&args(&["x", "1"])).is_err());
        assert!(parse_cli(&args(&["x", "1", "0", "-1", "1"])).is_err());
        assert!(parse_cli(&args(&["x", "a", "1", "-1", "1"])).is_err());
        assert!(parse_cli(&args(&["--color=neon", "x"])).is_err());
        assert!(parse_cli(&args(&["--show=re,foo", "x"])).is_err());
        assert!(parse_cli(&args(&["--colormap=2,(0,0,0)", "x"])).is_err());
        assert!(parse_cli(&args(&["--screenshot=", "x"])).is_err());
        assert!(parse_cli(&args(&["--bogus", "x"])).is_err());
        assert!(parse_cli(&args(&["--x"])).is_err());
        // "--" ends option parsing so double-negated expressions still work.
        let cli = parse_cli(&args(&["--color=4d", "--", "--x"])).unwrap();
        assert_eq!(cli.function, "--x");
        assert_eq!(cli.color_mode, ColorMode::FourD);
    }

    #[test]
    fn iso_intersection_finds_crossings() {
        let p0 = vec3(0.0, 0.0, 0.0);
        let p1 = vec3(1.0, 0.0, 0.0);
        let t = iso_crossing(0.0, 1.0, 0.5, 1e-12).unwrap();
        let hit = p0 + (p1 - p0) * t;
        assert!((hit.x - 0.5).abs() < 1e-6);
        assert_eq!(iso_crossing(0.0, 1.0, 0.0, 1e-12), Some(0.0));
        assert_eq!(iso_crossing(0.0, 1.0, 1.0, 1e-12), Some(1.0));
        assert!(iso_crossing(0.0, 1.0, 2.0, 1e-12).is_none());
        assert!(iso_crossing(0.5, 0.5, 0.5, 1e-12).is_none());

        // Interpolated attributes follow the same parameter as the position.
        let mut points = Vec::new();
        push_iso_intersection(&mut points, (p0, 0.0), 0.0, (p1, 1.0), 4.0, 1.0, 1e-12);
        assert_eq!(points.len(), 1);
        assert!((points[0].0.x - 0.25).abs() < 1e-6 && (points[0].1 - 0.25).abs() < 1e-6);
        // Duplicate positions are not pushed twice.
        push_iso_intersection(&mut points, (p0, 0.0), 0.0, (p1, 1.0), 4.0, 1.0, 1e-12);
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn build_plot_handles_poles() {
        let expr = Parser::parse("1/x").unwrap();
        let domain = Domain {
            re: Range::new(-1.0, 1.0).unwrap(),
            im: Range::new(-1.0, 1.0).unwrap(),
        };
        let visibility = SurfaceVisibility {
            show_real: true,
            show_imag: true,
            show_abs: true,
            show_arg: true,
        };
        // Odd sample count puts a sample exactly on the pole at 0.
        let colormap = ColorMap::default();
        let plot = build_plot(
            &expr,
            domain,
            21,
            visibility,
            shading(ColorMode::Rings, false, &colormap),
            true,
            4,
            0,
            YScale::Arsinh,
        );
        assert_eq!(plot.total_sample_count, 441);
        assert!(plot.finite_sample_count < plot.total_sample_count);
        assert!(plot.finite_sample_count > 0);
        assert!(plot.y.min < plot.y.max);
        for kind in SurfaceKind::ALL {
            let surface = plot.surface(kind);
            assert!(!surface.meshes.is_empty());
            assert_eq!(surface.meshes.len(), surface.vertex_infos.len());
            for (mesh, infos) in surface.meshes.iter().zip(surface.vertex_infos.iter()) {
                assert_eq!(mesh.vertices.len(), infos.len());
            }
            assert!(!surface.iso_lines.is_empty());
            for seg in &surface.iso_lines {
                assert!(seg.a.is_finite() && seg.b.is_finite());
                assert!((0.0..=1.0).contains(&seg.color01), "{seg:?}");
            }
        }
    }

    #[test]
    fn iso_segments_carry_partner_color() {
        // f(x) = x on a 9x9 grid: Re contours are lines re(x) = c crossing whole cells
        // between two sample rows, so each segment's color01 (from im(x)) must be the
        // midpoint of two neighbouring row values.
        let expr = Parser::parse("x").unwrap();
        let domain = Domain {
            re: Range::new(-1.0, 1.0).unwrap(),
            im: Range::new(-3.0, 5.0).unwrap(),
        };
        let visibility = SurfaceVisibility::from_cli_list("re").unwrap();
        let colormap = ColorMap::default();
        let n = 9;
        let plot = build_plot(
            &expr,
            domain,
            n,
            visibility,
            shading(ColorMode::FourD, false, &colormap),
            true,
            4,
            0,
            YScale::Linear,
        );
        let segments = &plot.surface(SurfaceKind::Real).iso_lines;
        // 4 contours, each crossing the 8 cell rows once.
        assert_eq!(segments.len(), 4 * (n - 1));
        let mut rows_seen = vec![0usize; n - 1];
        for seg in segments {
            let row = seg.color01 * (n - 1) as f32 - 0.5;
            assert!((row - row.round()).abs() < 1e-4, "{seg:?}");
            rows_seen[row.round() as usize] += 1;
        }
        assert!(rows_seen.iter().all(|&count| count == 4), "{rows_seen:?}");
        // Surfaces that are hidden get no contours, but keep their range.
        assert!(plot.surface(SurfaceKind::Imag).iso_lines.is_empty());
        assert!(plot.surface(SurfaceKind::Imag).value_range.is_some());
    }
}
