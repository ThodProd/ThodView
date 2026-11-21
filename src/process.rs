use std::{
    ffi::{OsStr, OsString},
    io::{self, Read},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn tool(name: &str, variable: &str, bundled: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(variable).filter(|p| Path::new(p).is_file()) {
        return path.into();
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dirs = exe.ancestors().skip(1);
        if let Some(dir) = dirs.next() {
            let adjacent = dir.join(bundled);
            if adjacent.is_file() {
                return adjacent;
            }
        }
    }
    // Locate PATH binaries explicitly. Windows otherwise searches the current directory.
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap_or_default()
                .with_file_name(name)
        })
}

pub fn ffmpeg() -> PathBuf {
    tool("ffmpeg.exe", "THODVIEW_FFMPEG", "runtime/ffmpeg/ffmpeg.exe")
}

pub fn exiftool() -> PathBuf {
    tool(
        "exiftool.exe",
        "THODVIEW_EXIFTOOL",
        "runtime/exiftool/ExifTool.exe",
    )
}

pub fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

pub fn run(exe: &Path, args: &[OsString], max_output: usize) -> Result<Vec<u8>> {
    let mut child = Command::new(exe)
        .args(args)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            format!(
                "{}: {err}",
                exe.file_name().unwrap_or(OsStr::new("")).to_string_lossy()
            )
        })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read_out = thread::spawn(move || read_limited(stdout, max_output));
    let read_err = thread::spawn(move || read_limited(stderr, 16 * 1024));

    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(result.err().unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "Время ожидания истекло")
                }));
            }
        }
    };
    let output = read_out.join().map_err(|_| "Ошибка чтения результата")?;
    let error = read_err.join().map_err(|_| "Ошибка чтения диагностики")?;
    let status = status?;
    if !status.success() {
        let error = error.unwrap_or_default();
        let message = String::from_utf8_lossy(&error);
        return Err(if message.trim().is_empty() {
            format!("{}: {status}", exe.display()).into()
        } else {
            message.trim().chars().take(1200).collect::<String>().into()
        });
    }
    Ok(output?)
}

fn read_limited(mut stream: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    (&mut stream)
        .take(limit as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(io::Error::other("Превышен предел размера результата"));
    }
    Ok(data)
}
