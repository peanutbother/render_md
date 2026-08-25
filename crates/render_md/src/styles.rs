use crate::Error;
use std::env;
use std::path::Path;
use std::process::Command;

/// Runs tailwind to compile `style_path` into {public_dir}/assets/style.css. The command runs in {base_dir}.
///
/// If `TAILWIND_BIN` is set in environment this will be used for invocation. Otherwise it relies on PATH resolution.
pub fn compile_styles(base_path: &Path, style_path: &Path, public_dir: &Path) -> Result<(), Error> {
    let tailwind_bin = env::var("TAILWIND_BIN").unwrap_or_else(|_| "tailwindcss".to_string());
    let output_css = public_dir.join("assets/style.css");

    let output = Command::new(tailwind_bin)
        .args([
            "-i",
            style_path.to_str().unwrap(),
            "-o",
            output_css.to_str().unwrap(),
            "--minify",
            "--silent",
        ])
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-mono")
        .current_dir(base_path)
        .output()
        .map_err(|e| {
            Error::tailwind_execution_failed(format!("Failed to execute tailwindcss: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(Error::tailwind_compilation_failed(format!(
            "Tailwind compilation failed with status: {}.\nstdout: {}\nstderr: {}",
            output.status, stdout, stderr
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_compile_styles_reports_error_when_binary_is_missing() {
        let tmp = TempDir::new().unwrap();
        let base_path = tmp.path().to_path_buf();
        let style_path = base_path.join("style.css");
        std::fs::write(&style_path, "").unwrap();
        let public_dir = base_path.join("public");
        std::fs::create_dir_all(&public_dir).unwrap();

        // SAFETY: this test doesn't run concurrently with anything else that
        // reads or writes TAILWIND_BIN.
        unsafe {
            env::set_var("TAILWIND_BIN", "definitely-not-a-real-tailwind-binary");
        }
        let result = compile_styles(&base_path, &style_path, &public_dir);
        unsafe {
            env::remove_var("TAILWIND_BIN");
        }

        match &result {
            Err(e) => match e.inner() {
                crate::ErrorKind::TailwindExecutionFailed(msg) => {
                    assert!(msg.contains("Failed to execute tailwindcss"))
                }
                _ => panic!("expected TailwindExecutionFailed error, got {:?}", result),
            },
            _ => panic!("expected TailwindExecutionFailed error, got {:?}", result),
        }
    }
}
