pub(crate) const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tiff", "tif",
];

pub(crate) fn is_image_file(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    IMAGE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}

pub(crate) const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "msi", "sh", "ps1", "vbs", "app", "dmg", "pkg", "deb", "rpm",
];

pub(crate) fn is_executable_file(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    EXECUTABLE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}

pub(crate) const IFRAME_SAFE_EXTENSIONS: &[&str] = &[
    "txt", "html", "htm", "css", "js", "json", "xml", "pdf", "csv", "log", "yml", "yaml", "toml",
    "ini", "conf", "cfg",
];

pub(crate) fn is_safe_for_iframe(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    // Allow text files, web files, and documents that browsers can display safely
    IFRAME_SAFE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}
