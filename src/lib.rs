use anyhow::Result;
use std::ffi::CStr;
use std::fs;
use std::os::raw::c_char;
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

// TODO:
// - search though all system, exluding: 
//let mut exclude_dirs: Vec<PathBuf> = vec![
//    "/proc",
//    "/sys",
//    "/dev",
//    "/run",
//    "/tmp",
//    "/var/lib/docker",
//]
// .into_iter()
// .map(PathBuf::from)
// .collect();
// - check for duplicates


#[repr(C)]
pub struct PluginInfo {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub default_prefix: *const c_char,
}

unsafe impl Send for PluginInfo {}
unsafe impl Sync for PluginInfo {}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct AppInfo {
    name: String,
    description: Option<String>,
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

// This is gonna be loaded from the main process before each get_entries call
// it's gonna contain all the needed data by the plugin
// paths: bool - all the paths from the system, NOT RECOMMENDED
// light_paths: bool - all files from the system, excluding .gitignore contents (using ignore crate),
//      and some root directories (like /proc, /sys, /dev, /run, /tmp, /var/lib/docker)
// local_paths: bool - all file in ~/ 
// xdg_paths: bool - all files in xdg::BaseDirectories::get_data_dirs() 
//      -> BaseDirectories allows to look up paths to configuration, data, cache and runtime files in well-known locations
#[repr(C)]
pub struct ReqData {
    pub paths: bool,
    pub light_paths: bool,
    pub local_paths: bool,
    pub xdg_paths: bool,
}

// This is gonna be loaded from the main process before each get_entries call, can be changed at runtime before a new get_entries call
#[unsafe(no_mangle)]
pub static mut REQ_DATA: ReqData = ReqData {
    paths: false,
    light_paths: false,
    local_paths: true,
    xdg_paths: true,
};

// this is gonna be used by the main process to send the paths to the plugin
#[repr(C)]
pub struct PathsArray {
    pub paths: *const *const c_char,
    pub length: usize,
}

#[repr(C)]
pub struct RespData {
    pub paths: *const PathsArray,
    pub light_paths: *const PathsArray,
    pub local_paths: *const PathsArray,
    pub xdg_paths: *const PathsArray,
}

#[unsafe(no_mangle)]
pub static mut RESP_DATA: RespData = RespData {
    paths: std::ptr::null(),
    light_paths: std::ptr::null(),
    local_paths: std::ptr::null(),
    xdg_paths: std::ptr::null(),
};

#[unsafe(no_mangle)]
pub extern "C" fn set_data(paths: *const PathsArray, light_paths: *const PathsArray, local_paths: *const PathsArray, xdg_paths: *const PathsArray) {
    unsafe {
        RESP_DATA.paths = paths;
        RESP_DATA.light_paths = light_paths;
        RESP_DATA.local_paths = local_paths;
        RESP_DATA.xdg_paths = xdg_paths;
    }
}


#[unsafe(no_mangle)]
pub static PLUGIN_INFO: PluginInfo = PluginInfo {
    name: literal_as_c_char!("Test Plugin"),
    version: literal_as_c_char!("1.0.0"),
    description: literal_as_c_char!("yaal"),
    author: literal_as_c_char!("Ri"),
    default_prefix: literal_as_c_char!(""),
};

#[unsafe(no_mangle)]
pub extern "C" fn handle_selection(selection: *const c_char) -> bool {
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
        return false;
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn get_entries() -> EntryList {
    unsafe {
        REQ_DATA.local_paths = false;
        REQ_DATA.xdg_paths = false;
    }
    let mut entries = Vec::new();
    let mut apps = Vec::new();
    apps.extend(find_apps(unsafe { RESP_DATA.local_paths }).unwrap());
    apps.extend(find_apps(unsafe { RESP_DATA.xdg_paths }).unwrap());
    for app in apps {
        let name = Box::leak(format!("{}\0", app.name).into_boxed_str());
        let path = Box::leak(format!("{}\0", app.path).into_boxed_str());
        let description = app.description.map(|d| Box::leak(format!("{}\0", d).into_boxed_str()));
        let icon = app
            .icon
            .map(|i| Box::leak(format!("{}\0", i).into_boxed_str()));
        let emoji = std::ptr::null();

        entries.push(Entry {
            name: name.as_ptr() as *const c_char,
            description: description.map_or(std::ptr::null(), |s| s.as_ptr() as *const c_char),
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

fn find_apps(paths: *const PathsArray) -> Result<Vec<AppInfo>> {
    let mut apps = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    
    if paths.is_null() {
        println!("Paths pointer is null");
        return Ok(apps);
    }
    
    let paths_array = unsafe { &*paths };
    
    if paths_array.paths.is_null() {
        println!("Paths array pointer is null");
        return Ok(apps);
    }
    
    // Get the paths slice
    let paths = unsafe { std::slice::from_raw_parts(paths_array.paths, paths_array.length) };
    println!("Total paths to process: {}", paths.len());
    
    for (i, path_ptr) in paths.iter().enumerate() {
        if path_ptr.is_null() {
            println!("Path pointer {} is null", i);
            continue;
        }
        if (*path_ptr).is_null() {
            println!("Dereferenced path pointer {} is null", i);
            continue;
        }
        
        let path = unsafe { CStr::from_ptr(*path_ptr) };
        let str_path = match path.to_str() {
            Ok(s) => s,
            Err(e) => {
                println!("Error converting path {} to string: {:?}", i, e);
                continue;
            }
        };
        
        // Skip if not a file
        let path_obj = std::path::Path::new(str_path);
        if !path_obj.is_file() {
            continue;
        }
        
        if let Some(ext) = path_obj.extension() {
            if ext == "desktop" {
                if str_path.contains("Celeste") {
                    println!("Processing Celeste desktop file: {}", str_path);
                }
                let content = match fs::read_to_string(str_path) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("Error reading file {}: {}", str_path, e);
                        continue;
                    }
                };
                if let Some(app_info) = parse_desktop_file(&content, str_path) {
                    if seen_names.insert(app_info.name.clone()) {
                        apps.push(app_info);
                    }
                }
            }
        }
    }
    println!("Found {} apps", apps.len());
    Ok(apps)
}


fn parse_desktop_file(content: &str, path: &str) -> Option<AppInfo> {
    let mut name = None;
    let mut icon = None;
    let mut description = None;
    let mut _emoji: Option<String> = None; // i don't use emoji in this plugin
    let mut in_desktop_entry = false;
    let mut no_display = false;
    let mut hidden = false;

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
                "Comment" | "Description" => description = Some(value.trim().to_string()),
                "NoDisplay" => {
                    no_display = value.trim().eq_ignore_ascii_case("true");
                },
                "Hidden" => {
                    hidden = value.trim().eq_ignore_ascii_case("true");
                },
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
        }),
        _ => {
            None
        }
    }
}

fn load_applications() -> Result<Vec<AppInfo>> {
    let xdg_dirs = xdg::BaseDirectories::new();
    let mut apps = Vec::new();
    let mut seen_names = HashSet::new();

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

            if path.starts_with("/tmp") {
                continue;
            }

            if !path.exists() || !path.is_file() {
                println!("Skipping inaccessible file: {:?}", path);
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    println!("Could not read file {:?}: {}", path, e);
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

