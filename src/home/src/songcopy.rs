// The internal song library (/data/songs): copying a portable drive into it, listing it, deleting from it.
// Runs as the unprivileged player: the only thing root does is mount the portable drive (yargos-songs, through the
// helper). Everything here works on plain folders so it can be tested with temporary directories.

use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const ROOT: &str = "/data/songs";
/// Where yargos-songs mounts the drive being imported (read-only).
pub const MOUNT: &str = "/run/yargos-import/mnt";
/// Written in each imported folder: which drive it came from, so a second drive with the same name gets its own folder.
const MARKER: &str = ".yargos-source";
/// A file is copied under this suffix and renamed when complete, so a cut never leaves a half file under the real name.
const PART: &str = ".yargos-part";
const CHUNK: usize = 1 << 20;
/// How many files in a row may fail before the drive is taken to be gone.
const GIVE_UP_AFTER: u32 = 20;

#[derive(Debug, PartialEq)]
pub enum Stop {
    Cancelled,
    /// The internal drive is down to the space kept free for the system.
    Full,
    /// The portable drive stopped answering (unplugged?).
    Gone,
    Other(String),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Plan {
    pub files: u64,
    pub bytes: u64,
    /// What is not on the internal drive yet.
    pub todo_files: u64,
    pub todo_bytes: u64,
    /// Folders that could not be read.
    pub unreadable: u64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    pub copied_files: u64,
    pub copied_bytes: u64,
    pub skipped_files: u64,
    pub failed_files: u64,
}

/// Folders Windows and file systems keep for themselves, at the top of a drive. Not songs, not copied.
fn housekeeping(name: &OsStr) -> bool {
    let n = name.to_string_lossy();
    n.starts_with('.') || n.starts_with('$') || n == "System Volume Information" || n == "lost+found" || n == "RECYCLER" || n.starts_with("found.")
}

fn ours(name: &OsStr) -> bool {
    let n = name.to_string_lossy();
    n == MARKER || n.ends_with(PART)
}

/// The text of one entry, for the folder name of an imported drive: no slashes or control characters, not hidden.
pub fn clean_label(label: &str) -> String {
    let s: String = label.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').collect();
    let s: String = s.trim().trim_start_matches('.').trim().chars().take(48).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "USB drive".into() } else { s }
}

/// The folder an import goes into: the drive's name, or "name (2)" and so on if a different drive already took it.
/// `existing(name)` is None when there is no such folder, else the drive id recorded in it ("" if none).
pub fn dest_name(label: &str, uuid: &str, existing: &dyn Fn(&str) -> Option<String>) -> String {
    let base = clean_label(label);
    for n in 1u32.. {
        let cand = if n == 1 { base.clone() } else { format!("{base} ({n})") };
        match existing(&cand) {
            None => return cand,
            Some(u) if u.is_empty() || uuid.is_empty() || u == uuid => return cand,
            Some(_) => {}
        }
    }
    unreachable!()
}

pub fn read_marker(root: &Path, name: &str) -> Option<String> {
    let dir = root.join(name);
    fs::symlink_metadata(&dir).ok().filter(|m| m.is_dir())?;
    Some(fs::read_to_string(dir.join(MARKER)).map(|s| s.trim().to_string()).unwrap_or_default())
}

/// What there is to copy from `src` into `dest` (which may not exist yet): everything except housekeeping folders,
/// links and special files; files already there with the same size count as done.
pub fn plan(src: &Path, dest: &Path, cancel: &AtomicBool) -> Result<Plan, Stop> {
    let mut p = Plan::default();
    walk(src, dest, cancel, &mut |entry| {
        match entry {
            Item::File { size, done, .. } => {
                p.files += 1;
                p.bytes += size;
                if !done {
                    p.todo_files += 1;
                    p.todo_bytes += size;
                }
            }
            Item::Unreadable => p.unreadable += 1,
            Item::Dir { .. } => {}
        }
        Ok(())
    })?;
    Ok(p)
}

enum Item<'a> {
    Dir { dest: &'a Path },
    File { src: &'a Path, dest: &'a Path, size: u64, done: bool },
    Unreadable,
}

fn walk(src: &Path, dest: &Path, cancel: &AtomicBool, f: &mut dyn FnMut(Item) -> Result<(), Stop>) -> Result<(), Stop> {
    let mut stack: Vec<(PathBuf, PathBuf, bool)> = vec![(src.to_path_buf(), dest.to_path_buf(), true)];
    while let Some((s, d, top)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let Ok(rd) = fs::read_dir(&s) else {
            f(Item::Unreadable)?;
            continue;
        };
        f(Item::Dir { dest: &d })?;
        let mut names: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let name = e.file_name();
            if (top && housekeeping(&name)) || ours(&name) {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            let (sp, dp) = (s.join(&name), d.join(&name));
            if t.is_dir() {
                stack.push((sp, dp, false));
            } else if t.is_file() {
                let Ok(m) = e.metadata() else {
                    f(Item::Unreadable)?;
                    continue;
                };
                let done = fs::symlink_metadata(&dp).map(|dm| dm.is_file() && dm.len() == m.len()).unwrap_or(false);
                f(Item::File { src: &sp, dest: &dp, size: m.len(), done })?;
            }
        }
    }
    Ok(())
}

/// Copy what `plan` found. `free_kib` says how much room the internal drive has (checked now and then), `reserve_kib`
/// is what must stay free. `progress(done, total)` is told about every chunk written.
pub fn run(
    src: &Path,
    dest: &Path,
    uuid: &str,
    todo_bytes: u64,
    reserve_kib: u64,
    free_kib: &dyn Fn() -> Option<u64>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Report, Stop> {
    fs::create_dir_all(dest).map_err(|e| Stop::Other(format!("Could not make {}: {e}", dest.display())))?;
    let _ = fs::write(dest.join(MARKER), format!("{uuid}\n"));
    let mut r = Report::default();
    let (mut done, mut since_check, mut failing) = (0u64, 0u64, 0u32);
    let mut stop = None;
    let walked = walk(src, dest, cancel, &mut |item| {
        match item {
            Item::Dir { dest } => {
                if let Err(e) = fs::create_dir_all(dest) {
                    if e.raw_os_error() == Some(28) {
                        return Err(Stop::Full);
                    }
                    r.failed_files += 1;
                }
            }
            Item::File { done: true, .. } => r.skipped_files += 1,
            Item::File { src, dest, size, .. } => {
                let mut wrote = 0u64;
                let res = copy_file(src, dest, cancel, &mut |n| {
                    wrote += n;
                    done += n;
                    since_check += n;
                    progress(done, todo_bytes);
                    if since_check >= 128 << 20 {
                        since_check = 0;
                        if free_kib().is_some_and(|f| f < reserve_kib) {
                            return Err(Stop::Full);
                        }
                    }
                    Ok(())
                });
                match res {
                    Ok(()) => {
                        r.copied_files += 1;
                        r.copied_bytes += size;
                        failing = 0;
                    }
                    Err(Stop::Other(_)) => {
                        done = done.saturating_sub(wrote); // not copied: take it back out of the count
                        r.failed_files += 1;
                        failing += 1;
                        if failing >= GIVE_UP_AFTER {
                            stop = Some(Stop::Gone);
                            return Err(Stop::Gone);
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            Item::Unreadable => r.failed_files += 1,
        }
        Ok(())
    });
    walked.map_err(|e| stop.take().unwrap_or(e))?;
    Ok(r)
}

pub fn copy_file(src: &Path, dest: &Path, cancel: &AtomicBool, on_chunk: &mut dyn FnMut(u64) -> Result<(), Stop>) -> Result<(), Stop> {
    let part = PathBuf::from(format!("{}{PART}", dest.display()));
    let result = (|| {
        let mut input = fs::File::open(src).map_err(|e| Stop::Other(e.to_string()))?;
        let mtime = input.metadata().and_then(|m| m.modified()).ok();
        // A folder or odd leftover in the way is not ours to keep.
        if fs::symlink_metadata(dest).map(|m| !m.is_file()).unwrap_or(false) {
            return Err(Stop::Other(format!("{} is in the way", dest.display())));
        }
        let mut out = fs::File::create(&part).map_err(|e| if e.raw_os_error() == Some(28) { Stop::Full } else { Stop::Other(e.to_string()) })?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Stop::Cancelled);
            }
            let n = input.read(&mut buf).map_err(|e| Stop::Other(e.to_string()))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| if e.raw_os_error() == Some(28) { Stop::Full } else { Stop::Other(e.to_string()) })?;
            on_chunk(n as u64)?;
        }
        if let Some(t) = mtime {
            let _ = out.set_modified(t);
        }
        drop(out);
        fs::rename(&part, dest).map_err(|e| Stop::Other(e.to_string()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

// ---- looking at and deleting the library ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Folder,
    /// A folder with a song in it (song.ini / notes.chart / notes.mid), or a packed .sng file.
    Song,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
}

fn is_song_folder(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()).any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n == "song.ini" || n == "notes.chart" || n == "notes.mid"
            })
        })
        .unwrap_or(false)
}

/// What is in a folder of the library, folders and songs first-class, hidden things left out.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<Entry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let t = e.file_type().ok()?;
            let kind = if t.is_dir() {
                if is_song_folder(&e.path()) { Kind::Song } else { Kind::Folder }
            } else if t.is_file() {
                if name.to_lowercase().ends_with(".sng") { Kind::Song } else { Kind::File }
            } else {
                return None;
            };
            Some(Entry { name, kind })
        })
        .collect();
    v.sort_by_key(|e| e.name.to_lowercase());
    v
}

/// The folder for a path of names below the library, if every name is a plain one (no "..", no slashes).
pub fn library_path(root: &Path, names: &[String]) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for n in names {
        if n.is_empty() || n == "." || n == ".." || n.contains('/') || n.contains('\0') {
            return None;
        }
        p.push(n);
    }
    Some(p)
}

/// Delete one thing in the library (a song, a folder with everything in it), or, with no names, everything in it.
/// Links are removed, never followed.
pub fn remove(root: &Path, names: &[String]) -> Result<u64, String> {
    let target = library_path(root, names).ok_or("That is not a folder of the song library")?;
    if !fs::symlink_metadata(root).map(|m| m.is_dir()).unwrap_or(false) {
        return Err("The internal song folder is missing".into());
    }
    let mut count = 0u64;
    let victims: Vec<PathBuf> = if names.is_empty() {
        fs::read_dir(root).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).collect()
    } else {
        vec![target]
    };
    for v in victims {
        let Ok(m) = fs::symlink_metadata(&v) else { continue };
        let r = if m.is_dir() { fs::remove_dir_all(&v) } else { fs::remove_file(&v) };
        r.map_err(|e| format!("Could not delete {}: {e}", v.file_name().unwrap_or_default().to_string_lossy()))?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    static N: AtomicU32 = AtomicU32::new(0);

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("yargos-songcopy-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn put(p: &Path, rel: &str, bytes: usize) {
        let f = p.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, vec![7u8; bytes]).unwrap();
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn copy(src: &Path, dest: &Path) -> Result<Report, Stop> {
        let p = plan(src, dest, &no_cancel())?;
        run(src, dest, "ID1", p.todo_bytes, 0, &|| Some(u64::MAX), &no_cancel(), &mut |_, _| {})
    }

    #[test]
    fn the_folder_structure_is_copied_as_it_is() {
        let (src, dest) = (tmp(), tmp().join("Drive"));
        put(&src, "Rock/Band - Song/song.ini", 10);
        put(&src, "Rock/Band - Song/song.opus", 3000);
        put(&src, "Pop/Deep/Er/Still/notes.mid", 5);
        put(&src, "loose.sng", 100);
        let r = copy(&src, &dest).unwrap();
        assert_eq!((r.copied_files, r.copied_bytes, r.failed_files), (4, 3115, 0));
        assert_eq!(fs::read(dest.join("Rock/Band - Song/song.opus")).unwrap().len(), 3000);
        assert!(dest.join("Pop/Deep/Er/Still/notes.mid").is_file());
        assert!(dest.join("loose.sng").is_file());
        // The source is untouched.
        assert!(src.join("Rock/Band - Song/song.ini").is_file());
    }

    #[test]
    fn housekeeping_links_and_leftovers_are_not_copied() {
        let (src, dest) = (tmp(), tmp().join("D"));
        put(&src, "$RECYCLE.BIN/x", 1);
        put(&src, "System Volume Information/y", 1);
        put(&src, ".Trashes/z", 1);
        put(&src, "Songs/a/song.ini", 1);
        put(&src, "Songs/a/leftover.yargos-part", 1);
        std::os::unix::fs::symlink("/etc", src.join("Songs/link")).unwrap();
        let r = copy(&src, &dest).unwrap();
        assert_eq!(r.copied_files, 1);
        assert!(!dest.join("$RECYCLE.BIN").exists() && !dest.join(".Trashes").exists() && !dest.join("Songs/link").exists());
    }

    #[test]
    fn a_second_run_skips_what_is_there_and_finishes_what_is_not() {
        let (src, dest) = (tmp(), tmp().join("D"));
        put(&src, "a/song.ini", 10);
        put(&src, "b/song.ini", 20);
        copy(&src, &dest).unwrap();
        // A file cut short (wrong size) is copied again; a part file is never taken for a finished one.
        fs::write(dest.join("b/song.ini"), b"short").unwrap();
        put(&dest, "c/song.ini.yargos-part", 4);
        let p = plan(&src, &dest, &no_cancel()).unwrap();
        assert_eq!((p.files, p.todo_files, p.todo_bytes), (2, 1, 20));
        let r = run(&src, &dest, "ID1", p.todo_bytes, 0, &|| None, &no_cancel(), &mut |_, _| {}).unwrap();
        assert_eq!((r.copied_files, r.skipped_files), (1, 1));
        assert_eq!(fs::read(dest.join("b/song.ini")).unwrap().len(), 20);
    }

    #[test]
    fn cancelling_leaves_no_half_files() {
        let (src, dest) = (tmp(), tmp().join("D"));
        put(&src, "a/big.bin", 5 << 20);
        let cancel = AtomicBool::new(false);
        let p = plan(&src, &dest, &cancel).unwrap();
        let r = run(&src, &dest, "ID1", p.todo_bytes, 0, &|| None, &cancel, &mut |done, _| {
            if done >= 1 << 20 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(r, Err(Stop::Cancelled));
        assert!(!dest.join("a/big.bin").exists() && !dest.join("a/big.bin.yargos-part").exists());
        // Cancelling while looking.
        assert_eq!(plan(&src, &dest, &AtomicBool::new(true)), Err(Stop::Cancelled));
    }

    #[test]
    fn it_stops_before_the_space_kept_for_the_system_is_used() {
        let (src, dest) = (tmp(), tmp().join("D"));
        put(&src, "a/one.bin", 200 << 20);
        let p = plan(&src, &dest, &no_cancel()).unwrap();
        let r = run(&src, &dest, "ID1", p.todo_bytes, 1000, &|| Some(10), &no_cancel(), &mut |_, _| {});
        assert_eq!(r, Err(Stop::Full));
        assert!(!dest.join("a/one.bin").exists());
    }

    #[test]
    fn progress_counts_bytes_of_what_is_copied() {
        let (src, dest) = (tmp(), tmp().join("D"));
        put(&src, "a/x", 1500);
        put(&src, "b/y", 2500);
        let p = plan(&src, &dest, &no_cancel()).unwrap();
        let mut last = (0, 0);
        run(&src, &dest, "ID1", p.todo_bytes, 0, &|| None, &no_cancel(), &mut |d, t| last = (d, t)).unwrap();
        assert_eq!(last, (4000, 4000));
    }

    #[test]
    fn folder_names_come_from_the_label_and_never_mix_two_drives() {
        assert_eq!(clean_label("My Songs"), "My Songs");
        assert_eq!(clean_label(".hidden/../x"), "hidden..x");
        assert_eq!(clean_label("  "), "USB drive");
        assert_eq!(clean_label("a\nb"), "ab");
        assert_eq!(clean_label(&"x".repeat(100)).chars().count(), 48);
        let lib = tmp();
        let existing = |n: &str| read_marker(&lib, n);
        assert_eq!(dest_name("YARGSONGS", "AAA", &existing), "YARGSONGS");
        fs::create_dir_all(lib.join("YARGSONGS")).unwrap();
        fs::write(lib.join("YARGSONGS").join(MARKER), "AAA\n").unwrap();
        // The same drive again goes to the same folder; another drive with the same name gets its own.
        assert_eq!(dest_name("YARGSONGS", "AAA", &existing), "YARGSONGS");
        assert_eq!(dest_name("YARGSONGS", "BBB", &existing), "YARGSONGS (2)");
        fs::create_dir_all(lib.join("YARGSONGS (2)")).unwrap();
        fs::write(lib.join("YARGSONGS (2)").join(MARKER), "BBB\n").unwrap();
        assert_eq!(dest_name("YARGSONGS", "CCC", &existing), "YARGSONGS (3)");
        assert_eq!(dest_name("YARGSONGS", "BBB", &existing), "YARGSONGS (2)");
        // A folder with no record (made by hand) is reused, and so is a drive with no id.
        fs::create_dir_all(lib.join("Plain")).unwrap();
        assert_eq!(dest_name("Plain", "ZZZ", &existing), "Plain");
        assert_eq!(dest_name("YARGSONGS", "", &existing), "YARGSONGS");
    }

    #[test]
    fn the_library_is_listed_as_folders_songs_and_files() {
        let lib = tmp();
        put(&lib, "Drive/Rock/Band - Song/Song.INI", 1);
        put(&lib, "Drive/Rock/Other/notes.chart", 1);
        put(&lib, "Drive/Rock/readme.txt", 1);
        put(&lib, "Drive/Rock/packed.sng", 1);
        put(&lib, "Drive/Rock/.hidden", 1);
        put(&lib, "Drive/Rock/Box/inner/song.ini", 1);
        let l = list(&lib.join("Drive/Rock"));
        let names: Vec<(&str, Kind)> = l.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(
            names,
            vec![("Band - Song", Kind::Song), ("Box", Kind::Folder), ("Other", Kind::Song), ("packed.sng", Kind::Song), ("readme.txt", Kind::File)]
        );
        assert_eq!(list(&lib).iter().map(|e| e.kind).collect::<Vec<_>>(), vec![Kind::Folder]);
        assert!(list(&lib.join("nothing")).is_empty());
    }

    #[test]
    fn deleting_works_at_song_folder_and_library_level_and_follows_nothing() {
        let lib = tmp();
        let outside = tmp();
        put(&outside, "precious.txt", 1);
        put(&lib, "D/Rock/One/song.ini", 1);
        put(&lib, "D/Rock/Two/song.ini", 1);
        put(&lib, "E/Pop/Three/song.ini", 1);
        std::os::unix::fs::symlink(&outside, lib.join("E/Pop/link")).unwrap();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(remove(&lib, &s(&["D", "Rock", "One"])), Ok(1));
        assert!(!lib.join("D/Rock/One").exists() && lib.join("D/Rock/Two").exists());
        assert_eq!(remove(&lib, &s(&["D"])), Ok(1));
        assert!(!lib.join("D").exists());
        assert_eq!(remove(&lib, &[]), Ok(1));
        assert!(lib.is_dir() && fs::read_dir(&lib).unwrap().count() == 0);
        assert!(outside.join("precious.txt").exists(), "a link in the library must not lead the delete outside it");
        // Names that climb out are refused.
        assert!(remove(&lib, &s(&[".."])).is_err());
        assert!(remove(&lib, &s(&["a/b"])).is_err());
        assert!(remove(&lib, &s(&[""])).is_err());
        assert_eq!(library_path(Path::new("/x"), &s(&["a", "b"])), Some(PathBuf::from("/x/a/b")));
    }
}
