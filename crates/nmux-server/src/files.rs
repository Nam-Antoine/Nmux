//! Read-only file access for the files panel, and staging of pasted images.
//!
//! Every path a client sends is resolved (symlinks included) and must end up
//! inside one of the workspace folders. The window is not trusted to stay in
//! its lane: the check is here.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use nmux_proto::{DirEntry, FileContent, FilePreview};

/// Text files are previewed up to this many bytes.
const MAX_TEXT: u64 = 512 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 5000;
const MAX_PASTE: usize = 10 * 1024 * 1024;
const PASTE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Drop the `\\?\` prefix Windows adds when resolving a path; it confuses
/// both people and most programs a path gets handed to.
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    }
}

/// Resolve `path` and require it to be inside one of `roots`.
pub fn resolve_inside(path: &str, roots: &[String]) -> Result<PathBuf, String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err("bad path".into());
    }
    let resolved = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    let inside = roots.iter().filter_map(|root| std::fs::canonicalize(root).ok()).any(|root| resolved.starts_with(&root));
    if inside {
        Ok(resolved)
    } else {
        Err("that path is outside the workspace folders".into())
    }
}

pub fn list_dir(path: &str, roots: &[String]) -> Result<(String, Vec<DirEntry>), String> {
    let dir = resolve_inside(path, roots)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        // Follows links, so a link to a folder opens like one; a dangling link is skipped.
        let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok((display(&dir), entries))
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        // Not SVG: it is markup that can carry script, and it previews fine as text.
        _ => None,
    }
}

pub fn read_file(path: &str, roots: &[String]) -> Result<FilePreview, String> {
    let file = resolve_inside(path, roots)?;
    let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    let size = meta.len();
    let content = if let Some(mime) = image_mime(&file) {
        if size > MAX_IMAGE {
            FileContent::TooLarge
        } else {
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            FileContent::Image { mime: mime.to_string(), data: base64::engine::general_purpose::STANDARD.encode(bytes) }
        }
    } else {
        let mut bytes = Vec::new();
        std::fs::File::open(&file)
            .map_err(|e| e.to_string())?
            .take(MAX_TEXT)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.contains(&0) {
            FileContent::Binary
        } else {
            FileContent::Text { text: String::from_utf8_lossy(&bytes).into_owned(), truncated: size > MAX_TEXT }
        }
    };
    Ok(FilePreview { path: display(&file), size, content })
}

/// The image type the bytes really are, whatever the client called them.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

/// Write a pasted image to a file under the data directory and return its
/// path. Agents take an image as a path typed into their prompt.
pub fn stage_pasted_image(data_dir: &Path, base64_data: &str) -> Result<PathBuf, String> {
    if base64_data.len() > MAX_PASTE * 4 / 3 + 4 {
        return Err("that image is too large to paste".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data).map_err(|_| "bad image data".to_string())?;
    let extension = sniff_image(&bytes).ok_or("the clipboard does not hold an image Nmux can paste")?;

    let dir = data_dir.join("pasted-images");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    remove_stale(&dir);
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("paste-{stamp}-{}.{extension}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn remove_stale(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > PASTE_MAX_AGE);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> Vec<String> {
        vec![dir.to_string_lossy().into_owned()]
    }

    #[test]
    fn listing_sorts_folders_first_and_stays_inside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("b.txt"), "bb").unwrap();
        std::fs::write(root.join("A.md"), "a").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();

        let (path, entries) = list_dir(&root.to_string_lossy(), &roots(&root)).unwrap();
        assert!(!path.starts_with(r"\\?\"), "{path}");
        let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.dir, e.size)).collect();
        assert_eq!(names, [("src", true, 0), ("A.md", false, 1), ("b.txt", false, 2)]);

        // Parent, sibling and dot-dot paths are refused.
        assert!(list_dir(&dir.path().to_string_lossy(), &roots(&root)).is_err());
        let sneaky = root.join("src").join("..").join("..").join("secret.txt");
        assert!(read_file(&sneaky.to_string_lossy(), &roots(&root)).unwrap_err().contains("outside"));
        assert!(read_file(&root.join("missing").to_string_lossy(), &roots(&root)).is_err());
    }

    #[test]
    fn previews() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let p = |name: &str| dir.path().join(name).to_string_lossy().into_owned();

        std::fs::write(p("a.rs"), "fn main() {}").unwrap();
        assert_eq!(read_file(&p("a.rs"), &r).unwrap().content, FileContent::Text { text: "fn main() {}".into(), truncated: false });

        std::fs::write(p("big.txt"), vec![b'x'; MAX_TEXT as usize + 10]).unwrap();
        assert!(matches!(read_file(&p("big.txt"), &r).unwrap().content, FileContent::Text { truncated: true, .. }));

        std::fs::write(p("blob.bin"), [1u8, 0, 2]).unwrap();
        assert_eq!(read_file(&p("blob.bin"), &r).unwrap().content, FileContent::Binary);

        std::fs::write(p("pic.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert!(matches!(read_file(&p("pic.png"), &r).unwrap().content, FileContent::Image { .. }));
        assert!(read_file(&dir.path().to_string_lossy(), &r).is_err(), "a folder is not a file");
    }

    #[test]
    fn pasted_images_are_checked_and_staged() {
        let dir = tempfile::tempdir().unwrap();
        let encode = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);

        let path = stage_pasted_image(dir.path(), &encode(b"\x89PNG\r\n\x1a\npixels")).unwrap();
        assert_eq!(path.extension().unwrap(), "png");
        assert!(path.starts_with(dir.path().join("pasted-images")));
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG\r\n\x1a\npixels");

        assert!(stage_pasted_image(dir.path(), &encode(b"#!/bin/sh\nrm -rf /")).is_err(), "not an image");
        assert!(stage_pasted_image(dir.path(), "not base64 !!").is_err());
    }
}
