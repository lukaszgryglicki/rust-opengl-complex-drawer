use num_complex::Complex64;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static REPORTED_ERRORS: AtomicUsize = AtomicUsize::new(0);

pub fn evaluate(base: Complex64, height: Complex64) -> Complex64 {
    solve(None, base, height)
}

/// The height `h` with `base^^h = value`.
pub fn super_log(base: Complex64, value: Complex64) -> Complex64 {
    solve(Some("--slog"), base, value)
}

/// The base `b` with `b^^height = value`.
pub fn super_root(value: Complex64, height: Complex64) -> Complex64 {
    solve(Some("--sroot"), value, height)
}

fn solve(mode: Option<&str>, a: Complex64, b: Complex64) -> Complex64 {
    match evaluate_with(Path::new("tet"), mode, a, b) {
        Ok(value) => value,
        Err(err) => {
            if REPORTED_ERRORS.fetch_add(1, Ordering::Relaxed) < 3 {
                let mode = mode.map(|mode| format!(" {mode}")).unwrap_or_default();
                eprintln!("tet{mode}({a}, {b}): {err}");
            }
            Complex64::new(f64::NAN, f64::NAN)
        }
    }
}

fn evaluate_with(
    program: &Path,
    mode: Option<&str>,
    a: Complex64,
    b: Complex64,
) -> Result<Complex64, String> {
    if !a.is_finite() || !b.is_finite() {
        return Err("tetration operands must be finite".to_owned());
    }
    let output = Command::new(program)
        .arg("-q")
        .args(mode)
        .arg("10")
        .args([
            a.re.to_string(),
            a.im.to_string(),
            b.re.to_string(),
            b.im.to_string(),
        ])
        .env("SILENT", "1")
        .env("RAYON_NUM_THREADS", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("could not run tet: {err}; ensure tet is on PATH"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason: String = stderr.trim().chars().take(200).collect();
        return Err(format!("tet exited with {}: {reason}", output.status));
    }
    parse_output(&output.stdout)
}

fn parse_output(output: &[u8]) -> Result<Complex64, String> {
    let text = std::str::from_utf8(output).map_err(|_| "tet output is not UTF-8".to_owned())?;
    let mut parts = text.split_whitespace();
    let mut number = || {
        parts
            .next()
            .and_then(|part| part.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| "tet did not return two finite f64 numbers".to_owned())
    };
    let value = Complex64::new(number()?, number()?);
    if parts.next().is_some() {
        return Err("tet returned unexpected extra output".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_quiet_solver_output_and_rejects_malformed_or_nonfinite_results() {
        assert_eq!(
            parse_output(b" 1.835609672\r\n5.827999776e-1\n").unwrap(),
            Complex64::new(1.835609672, 0.5827999776)
        );
        for output in ["", "1", "1 bad", "NaN 0", "0 inf", "1e999 0", "1 2 3"] {
            assert!(parse_output(output.as_bytes()).is_err(), "{output}");
        }
        assert!(parse_output(&[0xff, b'\n', b'0']).is_err());
    }

    #[test]
    fn rejects_nonfinite_operands_before_launching_and_reports_missing_programs() {
        let program =
            std::env::temp_dir().join(format!("complex-drawer-missing-tet-{}", std::process::id()));
        assert!(!program.exists());
        let finite = Complex64::new(1.0, 0.0);
        let nonfinite = Complex64::new(f64::NAN, 0.0);
        for (base, height) in [(nonfinite, finite), (finite, nonfinite)] {
            assert_eq!(
                evaluate_with(&program, None, base, height).unwrap_err(),
                "tetration operands must be finite"
            );
        }
        assert!(evaluate_with(&program, None, finite, finite)
            .unwrap_err()
            .contains("ensure tet is on PATH"));
    }
}
