use macroquad::camera::Camera;
use macroquad::prelude::*;
use num_complex::{Complex64, ComplexFloat};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SAMPLES_PER_AXIS: usize = 121;
const MIN_SAMPLES_PER_AXIS: usize = 5;
const MAX_SAMPLES_PER_AXIS: usize = 1024;
const MAX_MESH_SAMPLES_PER_AXIS: usize = 255;
const DOMAIN_EXTENT: f32 = 1.55;
const Y_EXTENT: f32 = 1.25;
const AUTO_ROTATE_RADIANS_PER_SEC: f32 = 0.28;
const MANUAL_ROTATE_RADIANS_PER_SEC: f32 = 1.35;
const TRANSPARENT_ALPHA: f32 = 0.50;

type C = Complex64;

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

#[macroquad::main(window_conf)]
async fn main() {
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
        show_real: true,
        show_imag: true,
        show_abs: true,
        transparent_surfaces: false,
        wireframe_mode: false,
        fullscreen: false,
        yaw: 0.75,
        pitch: 0.52,
        roll: 0.0,
        auto_rotate: true,
        show_help: true,
        status: String::new(),
    };

    state.rebuild_plot();

    loop {
        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        let mut rebuild_plot = false;

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
        if is_key_pressed(KeyCode::Z) {
            state.domain.scale_about_center(1.1);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::X) {
            state.domain.scale_about_center(1.0 / 1.1);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::H) {
            state.domain.shift_re(-0.10);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::L) {
            state.domain.shift_re(0.10);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::J) {
            state.domain.shift_im(-0.10);
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::K) {
            state.domain.shift_im(0.10);
            rebuild_plot = true;
        }

        // Resolution controls.
        if is_key_pressed(KeyCode::N) {
            rebuild_plot |= state.scale_samples(1.0 / 1.1);
        }
        if is_key_pressed(KeyCode::M) {
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
        if is_key_pressed(KeyCode::T) {
            state.transparent_surfaces = !state.transparent_surfaces;
            rebuild_plot = true;
        }
        if is_key_pressed(KeyCode::F) {
            state.wireframe_mode = !state.wireframe_mode;
        }

        if rebuild_plot {
            state.rebuild_plot();
        }

        let dt = get_frame_time();
        if state.auto_rotate {
            state.yaw += AUTO_ROTATE_RADIANS_PER_SEC * dt;
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
            if state.wireframe_mode {
                if state.show_abs {
                    draw_wireframe_surface(
                        &plot.samples,
                        plot.samples_per_axis,
                        state.domain,
                        plot.y,
                        SurfaceKind::Abs,
                        state.transparent_surfaces,
                    );
                }
                if state.show_imag {
                    draw_wireframe_surface(
                        &plot.samples,
                        plot.samples_per_axis,
                        state.domain,
                        plot.y,
                        SurfaceKind::Imag,
                        state.transparent_surfaces,
                    );
                }
                if state.show_real {
                    draw_wireframe_surface(
                        &plot.samples,
                        plot.samples_per_axis,
                        state.domain,
                        plot.y,
                        SurfaceKind::Real,
                        state.transparent_surfaces,
                    );
                }
            } else {
                if state.show_abs {
                    draw_meshes(&plot.abs_meshes);
                }
                if state.show_imag {
                    draw_meshes(&plot.imag_meshes);
                }
                if state.show_real {
                    draw_meshes(&plot.real_meshes);
                }
            }
            labels = draw_axes_and_ticks(&state.domain, plot);
        }

        set_default_camera();
        for (pos, text, color) in labels {
            draw_label_3d(&camera, pos, &text, color, 18.0);
        }
        draw_hud(&state);

        if is_key_pressed(KeyCode::P) {
            let filename = format!("complex_view_{}.png", unix_timestamp_seconds());
            get_screen_data().export_png(&filename);
            state.status = format!("saved {filename}");
            println!("Saved screenshot to {filename}");
        }

        next_frame().await;
    }
}

fn parse_cli_or_exit() -> Cli {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") || args.is_empty() {
        eprintln!("Usage:");
        eprintln!("  complex_surface_viewer \"exp(x)-ln(x)\" [re_min re_max im_min im_max]");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  complex_surface_viewer \"exp(x)-ln(x)\" -2 2 -2 2");
        eprintln!("  complex_surface_viewer \"sin(x)/x\"");
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }

    if args.len() != 1 && args.len() != 5 {
        eprintln!("Expected either 1 argument or 5 arguments.");
        eprintln!("Usage: complex_surface_viewer \"f(x)\" [re_min re_max im_min im_max]");
        std::process::exit(2);
    }

    let re = if args.len() == 5 {
        Range::new(parse_f64_arg(&args[1], "re_min"), parse_f64_arg(&args[2], "re_max"))
            .unwrap_or_else(|err| fatal(&err))
    } else {
        Range::new(-2.0, 2.0).unwrap()
    };

    let im = if args.len() == 5 {
        Range::new(parse_f64_arg(&args[3], "im_min"), parse_f64_arg(&args[4], "im_max"))
            .unwrap_or_else(|err| fatal(&err))
    } else {
        Range::new(-2.0, 2.0).unwrap()
    };

    Cli {
        function: args[0].clone(),
        re,
        im,
    }
}

fn parse_f64_arg(s: &str, name: &str) -> f64 {
    s.parse::<f64>()
        .unwrap_or_else(|_| fatal(&format!("{name} must be a finite floating-point number")))
}

fn fatal<T>(message: &str) -> T {
    eprintln!("{message}");
    std::process::exit(2);
}

#[derive(Clone)]
struct Cli {
    function: String,
    re: Range,
    im: Range,
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
    transparent_surfaces: bool,
    wireframe_mode: bool,
    fullscreen: bool,
    yaw: f32,
    pitch: f32,
    roll: f32,
    auto_rotate: bool,
    show_help: bool,
    status: String,
}

impl AppState {
    fn rebuild_plot(&mut self) {
        let visibility = self.visibility();
        self.plot = Some(build_plot(
            &self.expr,
            self.domain,
            self.samples_per_axis,
            visibility,
            self.transparent_surfaces,
        ));
        if let Some(plot) = &self.plot {
            self.status = format!(
                "domain re=[{}, {}] im=[{}, {}]  y=[{}, {}]  samples: {}x{}  finite: {}/{}  visible: {}{}{}",
                fmt_axis(self.domain.re.min),
                fmt_axis(self.domain.re.max),
                fmt_axis(self.domain.im.min),
                fmt_axis(self.domain.im.max),
                fmt_axis(plot.y.min),
                fmt_axis(plot.y.max),
                self.samples_per_axis,
                self.samples_per_axis,
                plot.finite_sample_count,
                plot.total_sample_count,
                if self.show_real { "Re " } else { "" },
                if self.show_imag { "Im " } else { "" },
                if self.show_abs { "|f|" } else { "" },
            );
        }
    }

    fn visibility(&self) -> SurfaceVisibility {
        SurfaceVisibility {
            show_real: self.show_real,
            show_imag: self.show_imag,
            show_abs: self.show_abs,
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
    real_meshes: Vec<Mesh>,
    imag_meshes: Vec<Mesh>,
    abs_meshes: Vec<Mesh>,
    y: Range,
    finite_sample_count: usize,
    total_sample_count: usize,
    samples_per_axis: usize,
    samples: Vec<Sample>,
}

#[derive(Clone, Copy)]
struct Sample {
    real: f64,
    imag: f64,
    abs: f64,
    valid: bool,
}

impl Sample {
    fn invalid() -> Self {
        Self {
            real: f64::NAN,
            imag: f64::NAN,
            abs: f64::NAN,
            valid: false,
        }
    }
}

#[derive(Clone, Copy)]
struct SurfaceVisibility {
    show_real: bool,
    show_imag: bool,
    show_abs: bool,
}


fn build_plot(
    expr: &Expr,
    domain: Domain,
    samples_per_axis: usize,
    visibility: SurfaceVisibility,
    transparent_surfaces: bool,
) -> PlotData {
    let n = samples_per_axis;
    assert!(n >= 2);
    let steps = n - 1;

    let mut samples = vec![Sample::invalid(); n * n];
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut finite_sample_count = 0;
    let mut visible_value_count = 0usize;

    for iz in 0..n {
        let im = lerp_f64(domain.im.min, domain.im.max, iz as f64 / steps as f64);
        for ix in 0..n {
            let re = lerp_f64(domain.re.min, domain.re.max, ix as f64 / steps as f64);
            let x = C::new(re, im);
            let f = expr.eval(x);
            let abs = f.norm();
            let valid = f.re.is_finite() && f.im.is_finite() && abs.is_finite();
            let idx = iz * n + ix;
            if valid {
                finite_sample_count += 1;
                if visibility.show_real {
                    y_min = y_min.min(f.re);
                    y_max = y_max.max(f.re);
                    visible_value_count += 1;
                }
                if visibility.show_imag {
                    y_min = y_min.min(f.im);
                    y_max = y_max.max(f.im);
                    visible_value_count += 1;
                }
                if visibility.show_abs {
                    y_min = y_min.min(abs);
                    y_max = y_max.max(abs);
                    visible_value_count += 1;
                }
                samples[idx] = Sample {
                    real: f.re,
                    imag: f.im,
                    abs,
                    valid: true,
                };
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
    let real_meshes = if visibility.show_real {
        build_surface_meshes(
            &samples,
            n,
            domain,
            y,
            SurfaceKind::Real,
            transparent_surfaces,
        )
    } else {
        Vec::new()
    };
    let imag_meshes = if visibility.show_imag {
        build_surface_meshes(
            &samples,
            n,
            domain,
            y,
            SurfaceKind::Imag,
            transparent_surfaces,
        )
    } else {
        Vec::new()
    };
    let abs_meshes = if visibility.show_abs {
        build_surface_meshes(
            &samples,
            n,
            domain,
            y,
            SurfaceKind::Abs,
            transparent_surfaces,
        )
    } else {
        Vec::new()
    };

    PlotData {
        real_meshes,
        imag_meshes,
        abs_meshes,
        y,
        finite_sample_count,
        total_sample_count: n * n,
        samples_per_axis: n,
        samples,
    }
}

#[derive(Clone, Copy)]
enum SurfaceKind {
    Real,
    Imag,
    Abs,
}

impl SurfaceKind {
    fn color(self, transparent: bool) -> Color {
        let alpha = if transparent { TRANSPARENT_ALPHA } else { 1.0 };
        match self {
            SurfaceKind::Real => Color::new(1.0, 0.04, 0.02, alpha),
            SurfaceKind::Imag => Color::new(0.08, 0.20, 1.0, alpha),
            SurfaceKind::Abs => Color::new(0.05, 0.70, 0.10, alpha),
        }
    }

    fn value(self, s: Sample) -> f64 {
        match self {
            SurfaceKind::Real => s.real,
            SurfaceKind::Imag => s.imag,
            SurfaceKind::Abs => s.abs,
        }
    }
}

fn build_surface_meshes(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    transparent: bool,
) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    let tile_stride = MAX_MESH_SAMPLES_PER_AXIS - 1;
    let mut z0 = 0usize;

    while z0 < samples_per_axis - 1 {
        let z1 = (z0 + tile_stride).min(samples_per_axis - 1);
        let mut x0 = 0usize;
        while x0 < samples_per_axis - 1 {
            let x1 = (x0 + tile_stride).min(samples_per_axis - 1);
            meshes.push(build_surface_mesh_tile(
                samples,
                samples_per_axis,
                domain,
                y_range,
                kind,
                transparent,
                x0,
                x1,
                z0,
                z1,
            ));
            x0 = x1;
        }
        z0 = z1;
    }

    meshes
}

fn build_surface_mesh_tile(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    transparent: bool,
    x0: usize,
    x1: usize,
    z0: usize,
    z1: usize,
) -> Mesh {
    let n = samples_per_axis;
    let steps = n - 1;
    let tile_w = x1 - x0 + 1;
    let tile_h = z1 - z0 + 1;
    assert!(tile_w <= MAX_MESH_SAMPLES_PER_AXIS);
    assert!(tile_h <= MAX_MESH_SAMPLES_PER_AXIS);
    assert!(tile_w * tile_h <= u16::MAX as usize);

    let mut vertices = Vec::with_capacity(tile_w * tile_h);
    let color = kind.color(transparent);

    for lz in 0..tile_h {
        let iz = z0 + lz;
        let im_t = iz as f64 / steps as f64;
        let im = lerp_f64(domain.im.min, domain.im.max, im_t);
        for lx in 0..tile_w {
            let ix = x0 + lx;
            let re_t = ix as f64 / steps as f64;
            let re = lerp_f64(domain.re.min, domain.re.max, re_t);
            let sample = samples[iz * n + ix];
            let y = if sample.valid { kind.value(sample) } else { y_range.min };
            let pos = vec3(
                map_re_to_world(domain.re, re),
                map_y_to_world(y_range, y),
                map_im_to_world(domain.im, im),
            );
            vertices.push(Vertex::new2(pos, vec2(re_t as f32, im_t as f32), color));
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

    Mesh {
        vertices,
        indices,
        texture: None,
    }
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

fn sample_world_pos(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
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
        map_y_to_world(y_range, kind.value(sample)),
        map_im_to_world(domain.im, im),
    ))
}

fn draw_wireframe_surface(
    samples: &[Sample],
    samples_per_axis: usize,
    domain: Domain,
    y_range: Range,
    kind: SurfaceKind,
    transparent: bool,
) {
    let n = samples_per_axis;
    let color = kind.color(transparent);

    for iz in 0..n {
        for ix in 0..(n - 1) {
            if let (Some(a), Some(b)) = (
                sample_world_pos(samples, n, domain, y_range, kind, ix, iz),
                sample_world_pos(samples, n, domain, y_range, kind, ix + 1, iz),
            ) {
                draw_line_3d(a, b, color);
            }
        }
    }

    for iz in 0..(n - 1) {
        for ix in 0..n {
            if let (Some(a), Some(b)) = (
                sample_world_pos(samples, n, domain, y_range, kind, ix, iz),
                sample_world_pos(samples, n, domain, y_range, kind, ix, iz + 1),
            ) {
                draw_line_3d(a, b, color);
            }
        }
    }
}

fn draw_axes_and_ticks(domain: &Domain, plot: &PlotData) -> Vec<(Vec3, String, Color)> {
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
            fmt_axis(value),
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
        "Re(f), Im(f), |f|".to_owned(),
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
        &format!("f(x) = {}", state.function_text),
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
                fmt_axis(plot.y.min),
                fmt_axis(plot.y.max)
            ),
            14.0,
            y,
            font_size,
            BLACK,
        );
        y += line;
        draw_text(
            &format!(
                "samples={}x{}  mode={}  alpha={}  fullscreen={}  visible: [1]Re={} [2]Im={} [3]|f|={}",
                state.samples_per_axis,
                state.samples_per_axis,
                if state.wireframe_mode { "wireframe" } else { "filled" },
                if state.transparent_surfaces { "0.5" } else { "1.0" },
                on_off(state.fullscreen),
                on_off(state.show_real),
                on_off(state.show_imag),
                on_off(state.show_abs),
            ),
            14.0,
            y,
            font_size,
            BLACK,
        );
        y += line;
    }

    draw_text(
        "red: Re(f)   blue: Im(f)   green: |f|",
        14.0,
        y,
        font_size,
        BLACK,
    );
    y += line;

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
            "1/2/3 toggle Re(f)/Im(f)/|f| visibility",
            "T toggle transparency, F toggle filled vs wireframe",
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
        _ => C::new(f64::NAN, f64::NAN), // unreachable after parser validation
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
        | "ceil" | "round" | "trunc" | "frac" | "fract" => arity == 1,
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
