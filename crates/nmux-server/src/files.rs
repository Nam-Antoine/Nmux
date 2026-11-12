use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use nmux_proto::{DirEntry, FileContent, FilePreview};

const MAX_TEXT: u64 = 512 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 5000;
const MAX_PASTE: usize = 10 * 1024 * 1024;
const PASTE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    }
}

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
    Ok((display(&dir), sorted(entries)))
}

fn entry_inside(path: &str, roots: &[String]) -> Result<PathBuf, String> {
    let wanted = Path::new(path);
    let (Some(parent), Some(name)) = (wanted.parent(), wanted.file_name()) else { return Err("bad path".into()) };
    Ok(resolve_inside(&parent.to_string_lossy(), roots)?.join(name))
}

pub fn plain_name(name: &str) -> Result<(), String> {
    let one = matches!(Path::new(name).components().collect::<Vec<_>>()[..], [Component::Normal(_)]);
    let odd = cfg!(windows) && (name.ends_with(['.', ' ']) || name.contains(['<', '>', ':', '"', '|', '?', '*']));
    if one && !odd {
        Ok(())
    } else {
        Err("that name will not work on this computer".into())
    }
}

fn named_inside(path: &str, roots: &[String]) -> Result<PathBuf, String> {
    let target = entry_inside(path, roots)?;
    plain_name(&target.file_name().unwrap_or_default().to_string_lossy())?;
    Ok(target)
}

fn taken(path: &Path) -> String {
    format!("{} already exists", path.file_name().unwrap_or_default().to_string_lossy())
}

pub fn taken_names(names: &[&str]) -> String {
    format!("already exists: {}", names.join(", ")).chars().take(300).collect()
}

fn fresh(path: &str, roots: &[String]) -> Result<PathBuf, String> {
    let target = named_inside(path, roots)?;
    if target.symlink_metadata().is_ok() {
        return Err(taken(&target));
    }
    Ok(target)
}

fn movable(path: &str, roots: &[String]) -> Result<PathBuf, String> {
    let entry = entry_inside(path, roots)?;
    let real = std::fs::canonicalize(&entry).map_err(|e| format!("{path}: {e}"))?;
    if roots.iter().filter_map(|root| std::fs::canonicalize(root).ok()).any(|root| root.starts_with(&real)) {
        return Err("a workspace folder cannot be renamed or removed".into());
    }
    Ok(entry)
}

pub fn make_dir(path: &str, roots: &[String]) -> Result<(), String> {
    std::fs::create_dir(fresh(path, roots)?).map_err(|e| e.to_string())
}

pub fn make_file(path: &str, roots: &[String]) -> Result<(), String> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(fresh(path, roots)?).map(drop).map_err(|e| e.to_string())
}

pub fn rename(from: &str, to: &str, roots: &[String]) -> Result<(), String> {
    let from = movable(from, roots)?;
    let to = named_inside(to, roots)?;
    if to.symlink_metadata().is_ok() && std::fs::canonicalize(&to).ok() != std::fs::canonicalize(&from).ok() {
        return Err(taken(&to));
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

pub fn remove(path: &str, roots: &[String]) -> Result<(), String> {
    trash::delete(display(&movable(path, roots)?)).map_err(|e| e.to_string())
}

pub fn restore(path: &str, roots: &[String]) -> Result<(), String> {
    let wanted = display(&fresh(path, roots)?);
    let item = trash::os_limited::list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|item| display(&item.original_path()).eq_ignore_ascii_case(&wanted))
        .max_by_key(|item| item.time_deleted)
        .ok_or("nothing to undo")?;
    trash::os_limited::restore_all([item]).map_err(|e| e.to_string())
}

pub fn discard_download(path: &str, downloads: &Path) -> Result<(), String> {
    let file = Path::new(path);
    let folder = file.parent().and_then(|parent| std::fs::canonicalize(parent).ok());
    if folder.is_none() || folder != std::fs::canonicalize(downloads).ok() || file.symlink_metadata().is_err() {
        return Err("that is not a download".into());
    }
    trash::delete(file).map_err(|e| e.to_string())
}

pub fn upload_names(sources: &[String]) -> Result<Vec<(PathBuf, String)>, String> {
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for source in sources {
        let path = PathBuf::from(source);
        let name = path.file_name().filter(|_| path.is_absolute()).ok_or("bad path")?.to_string_lossy().into_owned();
        if !path.exists() {
            return Err(format!("{source}: not found"));
        }
        if found.iter().any(|(_, other)| *other == name) {
            return Err(format!("two of those are named {name}"));
        }
        found.push((path, name));
    }
    Ok(found)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.symlink_metadata().is_ok_and(|meta| meta.is_symlink()) {
        return Err(std::io::Error::other(format!("{} is a link and cannot be replaced", display(to))));
    }
    if !from.is_dir() {
        return std::fs::copy(from, to).map(drop);
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

pub fn upload(sources: &[String], to: &str, overwrite: bool, roots: &[String]) -> Result<(), String> {
    let dir = resolve_inside(to, roots)?;
    let found = upload_names(sources)?;
    for (path, name) in &found {
        let real = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        if dir.starts_with(&real) || real.parent() == Some(dir.as_path()) {
            return Err(format!("{name} cannot be copied into itself"));
        }
    }
    let there: Vec<&str> = found.iter().map(|(_, name)| name.as_str()).filter(|name| dir.join(name).symlink_metadata().is_ok()).collect();
    if !overwrite && !there.is_empty() {
        return Err(taken_names(&there));
    }
    found.iter().try_for_each(|(path, name)| copy_tree(path, &dir.join(name))).map_err(|e| e.to_string())
}

pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let wanted = Path::new(name);
    let stem = wanted.file_stem().unwrap_or_default().to_string_lossy();
    let extension = wanted.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (0u32..)
        .map(|n| if n == 0 { dir.join(name) } else { dir.join(format!("{stem} ({n}){extension}")) })
        .find(|path| path.symlink_metadata().is_err())
        .unwrap_or_else(|| dir.join(name))
}

pub fn downloads(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let profile = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("USERPROFILE")).map(|(_, v)| v);
    Some(PathBuf::from(profile.or_else(|| env.get("HOME"))?).join("Downloads"))
}

pub fn sorted(mut entries: Vec<DirEntry>) -> Vec<DirEntry> {
    entries.truncate(MAX_ENTRIES);
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
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
    let (limit, whole) = wanted(&file);
    let mut bytes = Vec::new();
    if !whole || size <= limit {
        std::fs::File::open(&file).map_err(|e| e.to_string())?.take(limit).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    }
    Ok(FilePreview { path: display(&file), size, content: content(&file, size, bytes) })
}

pub fn wanted(path: &Path) -> (u64, bool) {
    if image_mime(path).is_some() {
        (MAX_IMAGE, true)
    } else {
        (MAX_TEXT, false)
    }
}

pub fn content(path: &Path, size: u64, bytes: Vec<u8>) -> FileContent {
    match image_mime(path) {
        Some(_) if size > MAX_IMAGE => FileContent::TooLarge,
        Some(mime) => FileContent::Image { mime: mime.to_string(), data: base64::engine::general_purpose::STANDARD.encode(bytes) },
        None if bytes.contains(&0) => FileContent::Binary,
        None => FileContent::Text { text: String::from_utf8_lossy(&bytes).into_owned(), truncated: size > MAX_TEXT },
    }
}

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

        assert!(list_dir(&dir.path().to_string_lossy(), &roots(&root)).is_err());
        let sneaky = root.join("src").join("..").join("..").join("secret.txt");
        assert!(read_file(&sneaky.to_string_lossy(), &roots(&root)).unwrap_err().contains("outside"));
        assert!(read_file(&root.join("missing").to_string_lossy(), &roots(&root)).is_err());
    }

    #[test]
    fn making_renaming_and_removing_stay_inside_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        let inner = root.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let r = vec![root.to_string_lossy().into_owned(), inner.to_string_lossy().into_owned()];
        let p = |rest: &str| root.join(rest).to_string_lossy().into_owned();
        let outside = dir.path().join("out").to_string_lossy().into_owned();

        make_dir(&p("src"), &r).unwrap();
        make_file(&p("src/a.txt"), &r).unwrap();
        assert!(make_dir(&p("src"), &r).unwrap_err().contains("src already exists"));
        assert!(make_file(&p("src/a.txt"), &r).unwrap_err().contains("already exists"));
        assert!(make_dir(&outside, &r).unwrap_err().contains("outside"));
        assert!(make_file(&p("src/../../escaped.txt"), &r).unwrap_err().contains("outside"));
        assert!(make_dir(&p("src/.."), &r).is_err());
        assert!(make_dir(&p("missing/deep"), &r).is_err());
        assert_eq!(make_file(&p("bad:name"), &r).is_err(), cfg!(windows));

        std::fs::write(p("src/b.txt"), "b").unwrap();
        assert!(rename(&p("src/a.txt"), &p("src/b.txt"), &r).unwrap_err().contains("b.txt already exists"));
        assert_eq!(std::fs::read_to_string(p("src/b.txt")).unwrap(), "b");
        rename(&p("src/a.txt"), &p("moved.txt"), &r).unwrap();
        assert!(root.join("moved.txt").is_file() && !root.join("src/a.txt").exists());
        rename(&p("moved.txt"), &p("MOVED.txt"), &r).unwrap();
        assert!(rename(&p("MOVED.txt"), &outside, &r).unwrap_err().contains("outside"));
        assert!(rename(&dir.path().join("secret.txt").to_string_lossy(), &p("stolen.txt"), &r).unwrap_err().contains("outside"));

        for root_path in [p(""), p("inner"), p("inner/."), root.to_string_lossy().to_uppercase()] {
            assert!(remove(&root_path, &r).is_err(), "{root_path}");
            assert!(rename(&root_path, &p("renamed"), &r).is_err(), "{root_path}");
        }
        assert!(remove(&p("INNER"), &r).is_err());
        assert!(remove(&dir.path().join("secret.txt").to_string_lossy(), &r).unwrap_err().contains("outside"));
        assert!(remove(&p("missing"), &r).is_err());
        assert!(inner.is_dir() && dir.path().join("secret.txt").is_file());

        std::fs::write(p("src/kept.txt"), "kept").unwrap();
        remove(&p("src"), &r).unwrap();
        assert!(!root.join("src").exists());
        restore(&p("src"), &r).unwrap();
        assert_eq!(std::fs::read_to_string(p("src/kept.txt")).unwrap(), "kept", "a delete can be undone");
        assert!(restore(&p("src"), &r).unwrap_err().contains("already exists"));
        assert!(restore(&p("never-there"), &r).unwrap_err().contains("nothing to undo"));
        assert!(restore(&outside, &r).unwrap_err().contains("outside"));
    }

    #[test]
    fn uploads_copy_trees_and_stop_before_a_collision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        let from = dir.path().join("from");
        std::fs::create_dir_all(from.join("my folder/deep")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(from.join("my folder/deep/x.txt"), "x").unwrap();
        std::fs::write(from.join("one.txt"), "1").unwrap();
        std::fs::write(from.join("two.txt"), "2").unwrap();
        let r = roots(&root);
        let to = root.to_string_lossy().into_owned();
        let s = |name: &str| from.join(name).to_string_lossy().into_owned();

        upload(&[s("one.txt"), s("my folder")], &to, false, &r).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("my folder/deep/x.txt")).unwrap(), "x");

        std::fs::write(from.join("one.txt"), "new").unwrap();
        let err = upload(&[s("two.txt"), s("one.txt")], &to, false, &r).unwrap_err();
        assert_eq!(err, "already exists: one.txt");
        let err = upload(&[s("my folder"), s("two.txt"), s("one.txt")], &to, false, &r).unwrap_err();
        assert_eq!(err, "already exists: my folder, one.txt", "every taken name is told");
        assert!(!root.join("two.txt").exists(), "nothing is copied when one name is taken");
        assert_eq!(std::fs::read_to_string(root.join("one.txt")).unwrap(), "1");

        upload(&[s("two.txt"), s("one.txt")], &to, true, &r).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("one.txt")).unwrap(), "new");

        assert!(upload(&[s("one.txt")], &from.to_string_lossy(), true, &r).unwrap_err().contains("outside"));
        assert!(upload(&[root.join("one.txt").to_string_lossy().into_owned()], &to, true, &r).unwrap_err().contains("itself"));
        assert!(upload(&[to.clone()], &root.join("my folder").to_string_lossy(), true, &r).unwrap_err().contains("itself"));
        assert!(upload(&["one.txt".to_string()], &to, true, &r).is_err());
        assert!(upload(&[s("missing.txt")], &to, true, &r).unwrap_err().contains("not found"));
        assert!(upload_names(&[s("one.txt"), root.join("one.txt").to_string_lossy().into_owned()]).unwrap_err().contains("named one.txt"));
    }

    fn link(target: &Path, at: &Path) -> bool {
        #[cfg(windows)]
        return std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(at).arg(target).output().is_ok_and(|out| out.status.success());
        #[cfg(not(windows))]
        return std::os::unix::fs::symlink(target, at).is_ok();
    }

    #[test]
    fn uploads_never_write_through_a_link_out_of_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        let outside = dir.path().join("outside");
        let from = dir.path().join("from");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(from.join("shared")).unwrap();
        std::fs::create_dir_all(from.join("docs/deep")).unwrap();
        std::fs::write(outside.join("keep.txt"), "old").unwrap();
        std::fs::write(from.join("shared/keep.txt"), "new").unwrap();
        std::fs::write(from.join("docs/deep/keep.txt"), "new").unwrap();
        assert!(link(&outside, &root.join("shared")) && link(&outside, &root.join("docs").join("deep")));
        let r = roots(&root);
        let to = root.to_string_lossy().into_owned();
        let s = |name: &str| from.join(name).to_string_lossy().into_owned();

        assert_eq!(upload(&[s("shared")], &to, false, &r).unwrap_err(), "already exists: shared");
        assert!(upload(&[s("shared")], &to, true, &r).unwrap_err().contains("is a link and cannot be replaced"));
        assert!(upload(&[s("docs")], &to, true, &r).unwrap_err().contains("is a link and cannot be replaced"));
        assert_eq!(std::fs::read_to_string(outside.join("keep.txt")).unwrap(), "old");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    }

    #[test]
    fn names_from_another_machine_must_be_one_plain_name_here() {
        assert!(plain_name("notes.txt").is_ok() && plain_name("my folder").is_ok() && plain_name(".env").is_ok());
        for bad in ["", ".", "..", "a/b", "../notes.txt", "/abs"] {
            assert!(plain_name(bad).is_err(), "{bad:?}");
        }
        for bad in ["..\\notes.txt", "a\\b", "C:notes.txt", "report ", "ends.", "what?"] {
            assert_eq!(plain_name(bad).is_err(), cfg!(windows), "{bad:?}");
        }
    }

    #[test]
    fn the_downloads_folder_is_in_the_profile_before_home() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
        assert_eq!(downloads(&env(&[("HOME", "/d/home"), ("UserProfile", "/c/me")])), Some(PathBuf::from("/c/me").join("Downloads")));
        assert_eq!(downloads(&env(&[("HOME", "/home/me")])), Some(PathBuf::from("/home/me").join("Downloads")));
        assert_eq!(downloads(&env(&[("PATH", "x")])), None);
    }

    #[test]
    fn only_something_in_downloads_can_be_discarded_as_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        std::fs::create_dir_all(downloads.join("deep")).unwrap();
        std::fs::write(downloads.join("got.txt"), "").unwrap();
        std::fs::write(downloads.join("deep/inner.txt"), "").unwrap();
        std::fs::write(dir.path().join("other.txt"), "").unwrap();
        let at = |path: PathBuf| path.to_string_lossy().into_owned();
        for not in [at(dir.path().join("other.txt")), at(downloads.join("deep/inner.txt")), at(downloads.join("missing.txt")), at(downloads.join("../other.txt")), at(downloads.clone())] {
            assert!(discard_download(&not, &downloads).is_err(), "{not}");
        }
        discard_download(&at(downloads.join("got.txt")), &downloads).unwrap();
        assert!(!downloads.join("got.txt").exists() && dir.path().join("other.txt").exists());
    }

    #[test]
    fn downloads_never_take_a_used_name() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_name(dir.path(), "a.txt"), dir.path().join("a.txt"));
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("a (1).txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("logs")).unwrap();
        std::fs::write(dir.path().join(".env"), "").unwrap();
        assert_eq!(free_name(dir.path(), "a.txt"), dir.path().join("a (2).txt"));
        assert_eq!(free_name(dir.path(), "logs"), dir.path().join("logs (1)"));
        assert_eq!(free_name(dir.path(), ".env"), dir.path().join(".env (1)"));
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
