use anyhow::Result;
use std::ffi::CStr;
use std::fs;
use std::os::raw::c_char;
use std::process::Command;

/// Converts a string literal into a C-compatible string pointer (`*const c_char`).
///
/// # Examples
/// ```
/// use std::os::raw::c_char;
///
/// let name = literal_as_c_char!("Test Plugin");
/// // name is now a *const c_char pointing to "Test Plugin"
/// ```
macro_rules! literal_as_c_char {
    ($s:expr) => {
        concat!($s, "\0").as_ptr() as *const c_char
    };
}


#[repr(C)]
pub struct PluginInfo {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub default_prefix: *const c_char,
}

#[allow(dead_code)]
#[derive(Clone)]
struct AppInfo {
    name: String,
    description: String,
    path: String,
    icon: Option<String>,
    emoji: Option<String>,
}

#[repr(C)]
pub struct Entry {
    pub name: *const c_char,        // the display name
    pub description: *const c_char, // still not sure what ill use this for
    pub value: *const c_char,       // the value that is gonna be passed to `handle_selection`
    pub icon: *const c_char,        // icon path (can be null)
    pub emoji: *const c_char,       // emoji (can be null)
}

unsafe impl Send for Entry {}
unsafe impl Sync for Entry {}

#[repr(C)]
pub struct EntryList {
    pub entries: *const Entry,
    pub length: usize,
}

unsafe impl Send for PluginInfo {}
unsafe impl Sync for PluginInfo {}

#[unsafe(no_mangle)]
pub static PLUGIN_INFO: PluginInfo = PluginInfo {
    name: literal_as_c_char!("Test Plugin"),
    version: literal_as_c_char!("1.0.0"),
    description: literal_as_c_char!("yaal"),
    author: literal_as_c_char!("Ri"),
    default_prefix: literal_as_c_char!(""),
};

#[unsafe(no_mangle)]
pub extern "C" fn handle_selection(selection: *const c_char) {
    let sel = unsafe { CStr::from_ptr(selection) };

    let status = Command::new("gio")
        .args([
            "launch",
            sel.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to open .desktop file");

    if !status.success() {
        println!("app : {}", sel.to_string_lossy());
        panic!("Error launching application");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn get_entries() -> EntryList {
    let apps = load_applications().unwrap();
    let mut entries = Vec::new();

    for app in apps {
        let name = Box::leak(format!("{}\0", app.name).into_boxed_str());
        let path = Box::leak(format!("{}\0", app.path).into_boxed_str());
        let description = Box::leak(format!("{}\0", app.description).into_boxed_str());
        let icon = app
            .icon
            .map(|i| Box::leak(format!("{}\0", i).into_boxed_str()));
        let emoji = std::ptr::null();

        entries.push(Entry {
            name: name.as_ptr() as *const c_char,
            description: description.as_ptr() as *const c_char,
            value: path.as_ptr() as *const c_char,
            icon: icon.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
            emoji: emoji,
        });
    }
    let list = EntryList {
        entries: entries.as_ptr() as *const Entry,
        length: entries.len(),
    };
    std::mem::forget(entries);
    list
}

fn parse_desktop_file(content: &str, path: &str) -> Option<AppInfo> {
    let mut name = None;
    let mut icon = None;
    let mut description = None;
    let mut _emoji: Option<String> = None; // i don't use emoji in this plugin
    let mut in_desktop_entry = false;
    let mut no_display = false;

    for line in content.lines() {
        let line = line.trim();

        if line == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if line.starts_with('[') {
            in_desktop_entry = false;
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "Name" => name = Some(value.trim().to_string()),
                "Icon" => icon = Some(value.trim().to_string()),
                "Comment" => description = Some(value.trim().to_string()),
                "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }

    if no_display {
        return None;
    }

    match (name, icon, description) {
        (Some(name), Some(icon), Some(description)) => Some(AppInfo {
            name,
            description,
            path: path.to_string(),
            icon: Some(icon),
            emoji: None,
        }),
        _ => None,
    }
}

fn load_applications() -> Result<Vec<AppInfo>> {
    let xdg_dirs = xdg::BaseDirectories::new()?;
    let mut apps = Vec::new();

    for entry in xdg_dirs.get_data_dirs() {
        let apps_dir = entry.join("applications");
        if !apps_dir.exists() {
            continue;
        }

        for entry in std::fs::read_dir(apps_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&path) {
                println!("try to load app: {}", path.to_string_lossy());
                if let Some(app_info) = parse_desktop_file(&content, &path.to_string_lossy()) {
                    apps.push(app_info);
                }
            }
        }
    }

    Ok(apps)
}
