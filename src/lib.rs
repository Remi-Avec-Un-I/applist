use anyhow::Result;
use std::ffi::CStr;
use std::fs;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::process::Command;
use std::collections::HashSet;

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
    description: Option<String>,
    path: String,
    icon: Option<String>,
    emoji: Option<String>,
    terminal: bool,
}

#[repr(C)]
pub struct Entry {
    pub name: *const c_char,        // the display name
    pub description: *const c_char, // still not sure what ill use this for, optional
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
    name: literal_as_c_char!("Application finder"),
    version: literal_as_c_char!("1.0.0"),
    description: literal_as_c_char!("Find applications on your system"),
    author: literal_as_c_char!("Ri"),
    default_prefix: literal_as_c_char!(""),
};

#[unsafe(no_mangle)]
pub extern "C" fn init_config(config: *const c_char) -> bool {
    if config.is_null() {
        println!("Config is null");
        return false;
    }
    
    let config_str = unsafe { CStr::from_ptr(config) };
    let config_json = config_str.to_str().unwrap_or("");
    println!("Applist Plugin received config: {}", config_json);
    
    // For now, just acknowledge the config
    // You can add actual config parsing here later if needed
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_selection(selection: *const c_char) -> bool {
    let sel = unsafe { CStr::from_ptr(selection) };
    let path = sel.to_str().unwrap();
    
    // Load applications to check if this is a terminal app
    if let Ok(apps) = load_applications() {
        if let Some(app) = apps.iter().find(|app| app.path == path) {
            return execute_gio_launch(path, app.terminal);
        }
    }
    
    // Fallback to default behavior
    execute_gio_launch(path, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn get_entries(query: *const c_char) -> EntryList {
    let apps = load_applications().unwrap();
    
    let mut entries = Vec::new();
    for app in apps {
        let name = Box::leak(format!("{}\0", app.name).into_boxed_str());
        let path = Box::leak(format!("{}\0", app.path).into_boxed_str());
        let description = app
            .description
            .map(|d| Box::leak(format!("{}\0", d).into_boxed_str()));
        let icon = app
            .icon
            .map(|i| Box::leak(format!("{}\0", i).into_boxed_str()));
        let emoji = std::ptr::null();

        entries.push(Entry {
            name: name.as_ptr() as *const c_char,
            value: path.as_ptr() as *const c_char,
            description: description.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
            icon: icon.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
            emoji: emoji,
        });
    }

    let mut filtered_entries = Vec::new();
    let query_str = if query.is_null() || query as usize == 1 {
        "".to_string()
    } else {
        unsafe { CStr::from_ptr(query).to_string_lossy().into_owned() }
    };
    let query_str = query_str.to_lowercase();
    
    for entry in entries {
        let name = unsafe { CStr::from_ptr(entry.name).to_string_lossy() };
        let name_lower = name.to_lowercase();
        if query_str.is_empty() || name_lower.contains(&query_str) {
            filtered_entries.push(entry);
        }
    }
    
    let list = EntryList {
        entries: filtered_entries.as_ptr() as *const Entry,
        length: filtered_entries.len(),
    };
    std::mem::forget(filtered_entries);
    list
}

fn parse_desktop_file(content: &str, path: &str) -> Option<AppInfo> {
    let mut name = None;
    let mut icon = None;
    let mut description = None;
    let mut _emoji: Option<String> = None; // i don't use emoji in this plugin
    let mut in_desktop_entry = false;
    let mut no_display = false;
    let mut hidden = false;
    let mut terminal = false; // Add terminal detection

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
                "Hidden" => hidden = value.trim().eq_ignore_ascii_case("true"),
                "Terminal" => terminal = value.trim().eq_ignore_ascii_case("true"), // Detect terminal apps
                _ => {}
            }
        }
    }

    if no_display || hidden {
        return None;
    }

    match (name, icon) {
        (Some(name), Some(icon)) => Some(AppInfo {
            name,
            description,
            path: path.to_string(),
            icon: Some(icon),
            emoji: None,
            terminal, // Set the terminal flag
        }),
        _ => None,
    }
}

fn load_applications() -> Result<Vec<AppInfo>> {
    let xdg_dirs = xdg::BaseDirectories::new();
    let mut apps = Vec::new();
    let mut seen_names = HashSet::new();
    let home_dir = home::home_dir();
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(home_dir) = home_dir {
        let local_apps = home_dir.join(".local/share/applications");
        paths.push(local_apps);
        let xdg_paths = xdg_dirs.get_data_dirs();
        paths.extend(xdg_paths);
    } else {
        return Err(anyhow::anyhow!("Failed to get home directory"));
    }

    for path in paths {
        let apps_dir = if path.ends_with("applications") {
            path
        } else {
            path.join("applications")
        };
        if !apps_dir.exists() {
            continue;
        }

        for entry in std::fs::read_dir(apps_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }

            if path.starts_with("/tmp") {
                continue;
            }

            if !path.exists() || !path.is_file() {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    println!("Failed to read desktop file: {}", e);
                    continue;
                }
            };
            if let Some(app_info) = parse_desktop_file(&content, &path.to_string_lossy()) {
                if seen_names.insert(app_info.name.clone()) {
                    apps.push(app_info);
                }
            }
        }
    }
    Ok(apps)
}

#[cfg(not(test))]
fn execute_gio_launch(path: &str, terminal: bool) -> bool {
    // First, validate the desktop file
    let validate_result = Command::new("desktop-file-validate")
        .arg(path)
        .output();
    
    if let Ok(output) = validate_result {
        if !output.status.success() {
            println!("Desktop file validation failed: {}", String::from_utf8_lossy(&output.stderr));
            return false;
        }
    }
    
    // For terminal applications, launch them directly in a terminal
    if terminal {
        // Try to find a terminal emulator
        let terminals = ["gnome-terminal", "konsole", "xterm", "alacritty", "kitty", "urxvt", "st"];
        
        for terminal_cmd in &terminals {
            // Extract the executable from the desktop file
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(exec_line) = content.lines().find(|line| line.starts_with("Exec=")) {
                    let exec_cmd = exec_line.strip_prefix("Exec=").unwrap_or("");
                    // Remove % parameters and clean up the command
                    let clean_cmd = exec_cmd.split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim();
                    
                    if !clean_cmd.is_empty() {
                        let result = Command::new(terminal_cmd)
                            .args(["-e", clean_cmd])
                            .spawn();
                        
                        if let Ok(_) = result {
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    // For non-terminal apps, try gtk-launch first
    let gtk_result = Command::new("gtk-launch")
        .arg(path)
        .status();
    
    if let Ok(status) = gtk_result {
        if status.success() {
            return true;
        }
    }
    
    // Fallback to gio launch
    Command::new("gio")
        .args(["launch", path])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
fn execute_gio_launch(_path: &str, _terminal: bool) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_applications() {
        let apps = load_applications().unwrap();
        assert!(!apps.is_empty());
    }

    #[test]
    fn get_entries_test() {
        let entries = get_entries(literal_as_c_char!(""));
        assert!(!entries.entries.is_null());
        assert!(entries.length > 0);
    }

    #[test]
    fn handle_selection_test() {
        let selection = literal_as_c_char!("firefox");
        let result = handle_selection(selection);
        assert!(result);
    }
    
}
