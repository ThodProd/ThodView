use crate::process::{self, Result};
use serde_json::Value;
use std::{collections::BTreeMap, io::Write, path::Path};

#[derive(Clone)]
pub struct Tag {
    pub key: String,
    pub label: String,
    pub value: String,
    pub editable: bool,
}

pub fn invoke(path: &Path, options: &[String], max_output: usize) -> Result<Vec<u8>> {
    if options.iter().any(|s| s.contains(['\r', '\n', '\0'])) {
        return Err("Недопустимый символ в аргументе ExifTool".into());
    }
    let path = std::path::absolute(path)?;
    let path = path.to_str().ok_or("Путь содержит некорректные символы")?;
    if path.contains(['\r', '\n', '\0']) {
        return Err("Недопустимый символ в имени файла".into());
    }
    let mut file = tempfile::NamedTempFile::new()?;
    for option in options {
        writeln!(file, "{option}")?;
    }
    writeln!(file, "{path}")?;
    file.flush()?;
    let mut args = process::args(&["-charset", "filename=UTF8", "-@"]);
    args.push(file.path().into());
    process::run(&process::exiftool(), &args, max_output)
}

pub fn read(path: &Path) -> Result<Vec<Tag>> {
    let args = ["-j", "-G1", "-a", "-l", "-lang", "ru"].map(str::to_owned);
    let output = invoke(path, &args, 32 * 1024 * 1024)?;
    let parsed: Value = serde_json::from_slice(&output)?;
    let fields = parsed
        .get(0)
        .and_then(Value::as_object)
        .ok_or("Нет метаданных")?;
    Ok(fields
        .iter()
        .map(|(key, field)| {
            let value = field.get("val").unwrap_or(field);
            let label = field.get("desc").and_then(Value::as_str).unwrap_or(key);
            let group = key.split(':').next().unwrap_or_default();
            let readonly = [
                "File",
                "ExifTool",
                "System",
                "Composite",
                "ICC_Profile",
                "MakerNotes",
            ];
            Tag {
                key: key.to_owned(),
                label: label.to_owned(),
                value: value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
                editable: key.contains(':')
                    && !readonly.contains(&group)
                    && !value.is_array()
                    && !value.is_object(),
            }
        })
        .collect())
}

pub fn valid_key(key: &str) -> bool {
    key.split_once(':').is_some_and(|(group, tag)| {
        [group, tag].iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
    })
}

pub fn write(path: &Path, original: &[Tag], edits: &[Tag]) -> Result<()> {
    let values = |tags: &[Tag]| -> BTreeMap<String, String> {
        tags.iter()
            .filter(|tag| tag.editable)
            .map(|tag| (tag.key.to_owned(), tag.value.to_owned()))
            .collect()
    };
    let before = values(original);
    let after = values(edits);
    if after.len() != edits.iter().filter(|t| t.editable).count() {
        return Err("Есть повторяющиеся имена полей".into());
    }
    if after.keys().any(|k| !valid_key(k)) {
        return Err("Укажите имя поля в виде Группа:Тег, например EXIF:Artist".into());
    }
    let mut args = vec!["-overwrite_original".to_owned()];
    for (key, value) in &after {
        if before.get(key) != Some(value) {
            args.push(format!("-{key}={}", value.replace(['\r', '\n'], " ")));
        }
    }
    for key in before
        .keys()
        .filter(|k| valid_key(k) && !after.contains_key(*k))
    {
        args.push(format!("-{key}="));
    }
    if args.len() > 1 {
        apply(path, &args)?;
    }
    Ok(())
}

pub fn strip(path: &Path) -> Result<()> {
    apply(path, &["-overwrite_original".into(), "-all=".into()])
}

fn apply(path: &Path, args: &[String]) -> Result<()> {
    let output = invoke(path, args, 256 * 1024)?;
    let output = String::from_utf8_lossy(&output);
    if !output.contains("1 image files updated") && !output.contains("1 image files unchanged") {
        return Err(format!("ExifTool не подтвердил сохранение. {}", output.trim()).into());
    }
    Ok(())
}
