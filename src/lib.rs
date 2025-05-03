use anyhow::Result;
use std::os::raw::c_char;
use std::ffi::CStr;
use std::fs;

#[repr(C)]
pub struct PluginInfo {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub default_prefix: *const c_char,
}

#[derive(Clone)]
struct AppInfo {
    name: String,
    exec: String,
    icon: Option<String>,
    emoji: Option<String>,
}

#[repr(C)]
pub struct Entry {
    pub name: *const c_char, // the display name
    pub description: *const c_char, // still not sure what ill use this for
    pub value: *const c_char, // the value that is gonna be passed to `handle_selection`
    pub icon: *const c_char, // icon path (can be null)
    pub emoji: *const c_char, // emoji (can be null)
}

// SAFETY: Entry only contains raw pointers which are safe to share between threads
unsafe impl Send for Entry {}
unsafe impl Sync for Entry {}

#[repr(C)]
pub struct EntryList {
    pub entries: *const Entry,
    pub length: usize,
}

// SAFETY: PluginInfo only contains raw pointers which are safe to share between threads
unsafe impl Send for PluginInfo {}
unsafe impl Sync for PluginInfo {}

#[unsafe(no_mangle)]
pub static PLUGIN_INFO: PluginInfo = PluginInfo {
    name: b"Test Plugin\0".as_ptr() as *const c_char,
    version: b"1.0.0\0".as_ptr() as *const c_char,
    description: b"yaal\0".as_ptr() as *const c_char,
    author: b"Ri\0".as_ptr() as *const c_char,
    default_prefix: b"\0".as_ptr() as *const c_char,
};

#[unsafe(no_mangle)]
pub extern "C" fn handle_selection(selection: *const c_char) {
    println!("yey");
    let sel = unsafe { CStr::from_ptr(selection) };
    println!("{}", sel.to_string_lossy());
}

#[unsafe(no_mangle)]
pub extern "C" fn get_entries() -> EntryList {
    let apps = load_applications().unwrap();
    let mut entries = Vec::new();
    
    for app in apps {
        let name = Box::leak(format!("{}\0", app.name).into_boxed_str());
        let exec = Box::leak(format!("{}\0", app.exec).into_boxed_str());
        let icon = app.icon.map(|i| Box::leak(format!("{}\0", i).into_boxed_str()));
        let emoji = app.emoji.map(|e| Box::leak(format!("{}\0", e).into_boxed_str()));

        entries.push(Entry {
            name: name.as_ptr() as *const c_char,
            description: name.as_ptr() as *const c_char,
            value: exec.as_ptr() as *const c_char,
            icon: icon.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
            emoji: emoji.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
        });
    }
    let list = EntryList {
        entries: entries.as_ptr() as *const Entry,
        length: entries.len(),
    };
    std::mem::forget(entries);
    list
}

fn parse_desktop_file(content: &str) -> Option<AppInfo> {
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut _emoji: Option<String> = None;
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
                "Exec" => exec = Some(value.trim().to_string()),
                "Icon" => icon = Some(value.trim().to_string()),
                "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }

    if no_display {
        return None;
    }

    match (name, exec, icon) {
        (Some(name), Some(exec), Some(icon)) => Some(AppInfo {
            name,
            exec,
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
                if let Some(app_info) = parse_desktop_file(&content) {
                    apps.push(app_info);
                }
            }
        }
    }

    Ok(apps)
}
