use crate::{decode::EXTENSIONS, process::Result};
use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::{Shell::*, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{Interface, PCWSTR, w},
};
use winreg::{
    RegKey, RegValue,
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_NONE},
};

const CLASSES: &str = r"Software\Classes";
const CAPABILITIES: &str = r"Software\ThodView\Capabilities";
const REGISTERED: &str = r"Software\RegisteredApplications";
const APPLICATION: &str = r"Applications\ThodView.exe";
const APP_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\ThodView.exe";
const PROG_ID: &str = "ThodView.Image.Native";
const LEGACY: &str = "ThodView.Image";
const OWNER_VALUE: &str = "ThodView.Owner";
const OWNER: &str = "ThodView";
const OLD_OWNER: &str = "ThodHUB.ThodView.v1";
const VERB: &str = "ThodView.Open";
const LABEL: &str = "Открыть в ThodView";

pub struct Com;

impl Com {
    pub fn new() -> Result<Self> {
        // An initialized apartment must be balanced on the same thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}

fn current_user() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

pub fn system_light() -> bool {
    current_user()
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .is_ok_and(|value| value != 0)
}

fn owned(key: &RegKey) -> bool {
    key.get_value::<String, _>(OWNER_VALUE)
        .is_ok_and(|v| v == OWNER)
        || key
            .get_value::<String, _>("ThodHUB.Owner")
            .is_ok_and(|v| v == OLD_OWNER)
}

fn create_owned(parent: &RegKey, key: &str) -> Result<RegKey> {
    let (key, disposition) = parent.create_subkey(key)?;
    if disposition == winreg::enums::REG_OPENED_EXISTING_KEY && !owned(&key) {
        return Err("В реестре уже есть запись с таким именем".into());
    }
    key.set_value(OWNER_VALUE, &OWNER)?;
    Ok(key)
}

fn set_default(parent: &RegKey, subkey: &str, value: &str) -> Result<()> {
    parent.create_subkey(subkey)?.0.set_value("", &value)?;
    Ok(())
}

fn notify() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

pub fn register(context_menu: bool, shortcut: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let user = current_user();
    let classes = user.create_subkey(CLASSES)?.0;
    let command = format!("\"{}\" -- \"%1\"", exe.display());
    let icon = format!("\"{}\",0", exe.display());

    let app_path = create_owned(&user, APP_PATH)?;
    app_path.set_value("", &exe.as_os_str())?;

    let prog = create_owned(&classes, PROG_ID)?;
    prog.set_value("", &"Изображение ThodView")?;
    prog.set_value("FriendlyTypeName", &"Изображение ThodView")?;
    let prog_app = prog.create_subkey("Application")?.0;
    prog_app.set_value("ApplicationName", &OWNER)?;
    prog_app.set_value("ApplicationDescription", &"Просмотр изображений")?;
    prog_app.set_value("ApplicationIcon", &icon)?;
    set_default(&prog, "DefaultIcon", &icon)?;
    set_default(&prog, r"shell\open\command", &command)?;

    let app = create_owned(&classes, APPLICATION)?;
    app.set_value("FriendlyAppName", &OWNER)?;
    set_default(&app, "DefaultIcon", &icon)?;
    set_default(&app, r"shell\open\command", &command)?;
    let supported = app.create_subkey("SupportedTypes")?.0;

    let capabilities = create_owned(&user, CAPABILITIES)?;
    capabilities.set_value("ApplicationName", &OWNER)?;
    capabilities.set_value("ApplicationDescription", &"Быстрый просмотр изображений")?;
    capabilities.set_value("ApplicationIcon", &icon)?;
    let associations = capabilities.create_subkey("FileAssociations")?.0;

    if let Ok(legacy) = classes.open_subkey_with_flags(LEGACY, KEY_READ | KEY_WRITE) {
        if owned(&legacy) {
            set_default(&legacy, "DefaultIcon", &icon)?;
            set_default(&legacy, r"shell\open\command", &command)?;
        }
    }

    for ext in EXTENSIONS {
        let dotted = format!(".{ext}");
        supported.set_value(&dotted, &"")?;
        associations.set_value(&dotted, &PROG_ID)?;
        let entry = classes
            .create_subkey(format!(r"{dotted}\OpenWithProgids"))?
            .0;
        entry.set_raw_value(
            PROG_ID,
            &RegValue {
                bytes: vec![],
                vtype: REG_NONE,
            },
        )?;

        let path = format!(r"SystemFileAssociations\{dotted}\shell\{VERB}");
        if context_menu {
            let menu = create_owned(&classes, &path)?;
            menu.set_value("", &LABEL)?;
            menu.set_value("Icon", &icon)?;
            menu.set_value("MultiSelectModel", &"Single")?;
            set_default(&menu, "command", &command)?;
        } else {
            delete_owned(&classes, &path)?;
        }
    }
    user.create_subkey(REGISTERED)?
        .0
        .set_value(OWNER, &CAPABILITIES)?;
    if shortcut {
        start_menu_shortcut(Some(&exe))?;
    }
    notify();
    Ok(())
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn delete_owned(parent: &RegKey, path: &str) -> Result<()> {
    match parent.open_subkey(path) {
        Ok(key) if owned(&key) => parent.delete_subkey_all(path)?,
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub fn unregister() -> Result<()> {
    let user = current_user();
    let exe = std::env::current_exe()?;
    let expected = exe.as_os_str();
    // An older installation can be removed after a newer executable took over.
    let path = user.open_subkey(APP_PATH);
    if !path.is_ok_and(|key| {
        owned(&key)
            && key
                .get_value::<std::ffi::OsString, _>("")
                .is_ok_and(|v| v == expected)
    }) {
        return Ok(());
    }
    let classes = user.create_subkey(CLASSES)?.0;
    for ext in EXTENSIONS {
        let entry = format!(r".{ext}\OpenWithProgids");
        if let Ok(key) = classes.open_subkey_with_flags(entry, KEY_WRITE) {
            ignore_missing(key.delete_value(PROG_ID))?;
        }
        delete_owned(
            &classes,
            &format!(r"SystemFileAssociations\.{ext}\shell\{VERB}"),
        )?;
    }
    for path in [PROG_ID, LEGACY, APPLICATION] {
        delete_owned(&classes, path)?;
    }
    delete_owned(&user, CAPABILITIES)?;
    if let Ok(registered) = user.open_subkey_with_flags(REGISTERED, KEY_READ | KEY_WRITE) {
        if registered
            .get_value::<String, _>(OWNER)
            .is_ok_and(|path| path == CAPABILITIES)
        {
            ignore_missing(registered.delete_value(OWNER))?;
        }
    }
    start_menu_shortcut(None)?;
    delete_owned(&user, APP_PATH)?;
    notify();
    Ok(())
}

pub fn registered_installation_exists() -> bool {
    current_user().open_subkey(APP_PATH).is_ok_and(|key| {
        key.get_value::<String, _>(OWNER_VALUE)
            .is_ok_and(|v| v == OWNER)
            && key
                .get_value::<std::ffi::OsString, _>("")
                .is_ok_and(|path| Path::new(&path).is_file())
    })
}

fn start_menu_shortcut(exe: Option<&Path>) -> Result<()> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Ok(());
    };
    let path = Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\ThodView.lnk");
    let _com = Com::new()?;
    let filename = wide(path.as_os_str());
    // Both interfaces reference the same ShellLink COM object.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        if path.is_file() {
            file.Load(PCWSTR(filename.as_ptr()), STGM_READ)?;
            let mut description = [0_u16; 260];
            link.GetDescription(&mut description)?;
            let end = description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(description.len());
            if !matches!(
                String::from_utf16_lossy(&description[..end]).as_str(),
                "ThodView" | "ThodView · ThodHUB"
            ) {
                return Ok(());
            }
            if exe.is_none() {
                std::fs::remove_file(&path)?;
                return Ok(());
            }
        }
        if let Some(exe) = exe {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let target = wide(exe.as_os_str());
            let dir = wide(
                exe.parent()
                    .ok_or("Не найдена папка программы")?
                    .as_os_str(),
            );
            link.SetPath(PCWSTR(target.as_ptr()))?;
            link.SetIconLocation(PCWSTR(target.as_ptr()), 0)?;
            link.SetWorkingDirectory(PCWSTR(dir.as_ptr()))?;
            link.SetDescription(w!("ThodView"))?;
            file.Save(PCWSTR(filename.as_ptr()), true)?;
        }
    }
    Ok(())
}

pub fn defaults() -> Result<()> {
    launch(
        w!("ms-settings:defaultapps?registeredAppUser=ThodView"),
        PCWSTR::null(),
    )
    .or_else(|_| launch(w!("ms-settings:defaultapps"), PCWSTR::null()))
}

pub fn reveal(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let explorer = Path::new(&std::env::var_os("WINDIR").ok_or("Не найдена папка Windows")?)
        .join("explorer.exe");
    let mut argument = std::ffi::OsString::from("/select,\"");
    argument.push(path.as_os_str());
    argument.push("\"");
    std::process::Command::new(explorer)
        .raw_arg(argument)
        .spawn()?;
    Ok(())
}

fn launch(target: PCWSTR, arguments: PCWSTR) -> Result<()> {
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            target,
            arguments,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!(
            "Windows не удалось открыть настройки ({})",
            result.0 as isize
        )
        .into());
    }
    Ok(())
}
