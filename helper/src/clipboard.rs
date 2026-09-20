use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const WL_COPY: &str = "/usr/bin/wl-copy";

pub fn write_text(text: &str) -> Result<(), String> {
    write_text_with(Path::new(WL_COPY), text)
}

pub fn clipboard_command(binary: &Path) -> Command {
    Command::new(binary)
}

pub fn write_text_with(binary: &Path, text: &str) -> Result<(), String> {
    if !binary.is_file() {
        return Err("Clipboard is unavailable".into());
    }

    let mut child = clipboard_command(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Clipboard is unavailable".to_string())?;

    {
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            return Err("Clipboard is unavailable".into());
        };
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.kill();
            return Err("The clipboard could not be written".into());
        }
    }

    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        _ => Err("The clipboard could not be written".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intended_command_is_absolute_wl_copy_with_no_args() {
        let command = clipboard_command(Path::new(WL_COPY));
        assert_eq!(command.get_program(), WL_COPY);
        assert_eq!(command.get_args().len(), 0);
    }

    #[test]
    fn missing_binary_rejects_without_spawning() {
        let missing = Path::new("/tmp/keepassxc-control-missing-wl-copy");
        assert!(!missing.is_file());
        let error = write_text_with(missing, "secret-must-not-be-argv").unwrap_err();
        assert_eq!(error, "Clipboard is unavailable");
    }
}
