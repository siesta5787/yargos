// Venues (YARG's backgrounds) from the song drive: the files in a folder called "venues" on the drive are copied to the
// folder YARG reads them from (<persistent data path>/venue = /data/yarg/venue; YARG picks one at random for each song
// from everything in it, subfolders included). The dashboard runs as the player, so everything written here belongs to
// the player and nothing follows a link: a link on the drive is never copied, and removing deletes links, not their targets.

use crate::songcopy::{self, Stop};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub const DEST: &str = "/data/yarg/venue";
pub const DRIVE: &str = "/media/songs";

/// What YARG loads from its venue folder.
const EXTENSIONS: [&str; 7] = ["yarground", "mp4", "mov", "webm", "png", "jpg", "jpeg"];
const MAX_FILES: usize = 5000;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct VenueFile {
    pub src: PathBuf,
    /// Where it goes below the venue folder (the folders it was in are kept, so equal names do not clash).
    pub rel: PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VenuePlan {
    pub found: u64,
    pub todo: u64,
    pub todo_bytes: u64,
    /// Room on the data drive and what must stay free (filled in by the caller).
    pub free_kib: u64,
    pub reserve_kib: u64,
}

impl VenuePlan {
    pub fn fits(&self) -> bool {
        self.free_kib >= self.reserve_kib + self.todo_bytes / 1024
    }
}

fn wanted(name: &str) -> bool {
    let low = name.to_lowercase();
    !name.starts_with('.') && low.rsplit_once('.').is_some_and(|(_, e)| EXTENSIONS.contains(&e))
}

/// The venue files in the "venues" (or "venue") folder at the top of `root`, any case, with its subfolders (the YARC
/// Launcher's own layout, Venues/<id>/installation/..., works too). Err says what to do when there are none.
pub fn find(root: &Path) -> Result<Vec<VenueFile>, String> {
    let top = fs::read_dir(root)
        .map_err(|_| "No song drive found. Plug it in first.".to_string())?
        .filter_map(|e| e.ok())
        .find(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            (n == "venues" || n == "venue") && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
        })
        .map(|e| e.path())
        .ok_or_else(|| "No folder called venues on the song drive. Put the venue files (.yarground) in a folder called venues at the top of the drive.".to_string())?;
    let mut found = vec![];
    let mut stack = vec![(top.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(t) = e.file_type() else { continue };
            let path = e.path();
            if t.is_dir() && depth < MAX_DEPTH {
                stack.push((path, depth + 1));
            } else if t.is_file() && wanted(&e.file_name().to_string_lossy()) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let rel = path.strip_prefix(&top).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from(e.file_name()));
                found.push(VenueFile { src: path, rel, size });
                if found.len() >= MAX_FILES {
                    return Ok(found);
                }
            }
        }
    }
    if found.is_empty() {
        return Err("The venues folder has no venue files in it (.yarground, or video and picture backgrounds).".into());
    }
    Ok(found)
}

fn already_there(f: &VenueFile, dest: &Path) -> bool {
    fs::symlink_metadata(dest.join(&f.rel)).map(|m| m.is_file() && m.len() == f.size).unwrap_or(false)
}

pub fn plan(files: &[VenueFile], dest: &Path) -> VenuePlan {
    let todo: Vec<_> = files.iter().filter(|f| !already_there(f, dest)).collect();
    VenuePlan { found: files.len() as u64, todo: todo.len() as u64, todo_bytes: todo.iter().map(|f| f.size).sum(), ..VenuePlan::default() }
}

/// Copy what is not there yet. `free_kib` is the room on the data drive, `reserve_kib` what must stay free.
/// Returns (files copied, files that failed).
pub fn copy(
    files: &[VenueFile],
    dest: &Path,
    reserve_kib: u64,
    free_kib: &dyn Fn() -> Option<u64>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(u64, u64), Stop> {
    let todo: Vec<_> = files.iter().filter(|f| !already_there(f, dest)).collect();
    let total: u64 = todo.iter().map(|f| f.size).sum();
    let never = AtomicBool::new(false);
    let (mut done, mut copied, mut failed) = (0u64, 0u64, 0u64);
    for f in todo {
        let target = dest.join(&f.rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Stop::Other(format!("Could not make {}: {e}", parent.display())))?;
        }
        if free_kib().is_some_and(|k| k < reserve_kib + f.size / 1024) {
            return Err(Stop::Full);
        }
        match songcopy::copy_file(&f.src, &target, &never, &mut |n| {
            done += n;
            progress(done, total);
            Ok(())
        }) {
            Ok(()) => copied += 1,
            Err(Stop::Full) => return Err(Stop::Full),
            Err(_) => failed += 1,
        }
    }
    Ok((copied, failed))
}

/// How many venue files are in the venue folder (what was added, or put there by hand).
pub fn installed(dest: &Path) -> u64 {
    let mut n = 0;
    let mut stack = vec![(dest.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() && depth < MAX_DEPTH {
                stack.push((e.path(), depth + 1));
            } else if t.is_file() && wanted(&e.file_name().to_string_lossy()) {
                n += 1;
            }
        }
    }
    n
}

/// Delete everything in the venue folder (the folder stays). Links are removed, never followed. Returns the number of files.
pub fn remove_all(dest: &Path) -> Result<u64, String> {
    let before = installed(dest);
    let Ok(rd) = fs::read_dir(dest) else { return Ok(0) };
    for e in rd.filter_map(|e| e.ok()) {
        let Ok(t) = e.file_type() else { continue };
        let path = e.path();
        let r = if t.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        r.map_err(|err| format!("Could not remove {}: {err}", path.display()))?;
    }
    Ok(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yargos-venues-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn finds_venue_files_in_a_folder_called_venues_in_any_case_and_below_it() {
        let root = tmp("find");
        assert!(find(&root).unwrap_err().contains("No folder called venues"));
        fs::create_dir_all(root.join("VENUES/abc-123/installation")).unwrap();
        fs::write(root.join("VENUES/Outdoor.yarground"), b"x").unwrap();
        fs::write(root.join("VENUES/abc-123/installation/Club.YARGROUND"), b"yy").unwrap();
        fs::write(root.join("VENUES/readme.txt"), b"no").unwrap();
        fs::write(root.join("VENUES/.hidden.png"), b"no").unwrap();
        fs::write(root.join("VENUES/bg.mp4"), b"v").unwrap();
        let mut rels: Vec<_> = find(&root).unwrap().into_iter().map(|f| f.rel.to_string_lossy().to_string()).collect();
        rels.sort();
        assert_eq!(rels, ["Outdoor.yarground", "abc-123/installation/Club.YARGROUND", "bg.mp4"]);
        // an empty venues folder says so
        let empty = tmp("empty");
        fs::create_dir_all(empty.join("venues")).unwrap();
        assert!(find(&empty).unwrap_err().contains("no venue files"));
        // a file called venues is not a folder
        let file = tmp("file");
        fs::write(file.join("venues"), b"x").unwrap();
        assert!(find(&file).is_err());
    }

    #[test]
    fn links_are_never_copied_and_a_copy_is_repeatable() {
        let root = tmp("copy-src");
        let dest = tmp("copy-dest");
        fs::create_dir_all(root.join("venues/sub")).unwrap();
        fs::write(root.join("venues/a.yarground"), vec![7u8; 3000]).unwrap();
        fs::write(root.join("venues/sub/b.yarground"), vec![8u8; 10]).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("venues/evil.yarground")).unwrap();
        let files = find(&root).unwrap();
        assert_eq!(files.len(), 2, "the link is not a venue file");
        assert_eq!(plan(&files, &dest), VenuePlan { found: 2, todo: 2, todo_bytes: 3010, free_kib: 0, reserve_kib: 0 });
        let mut last = (0, 0);
        let (copied, failed) = copy(&files, &dest, 0, &|| Some(u64::MAX), &mut |d, t| last = (d, t)).unwrap();
        assert_eq!((copied, failed, last), (2, 0, (3010, 3010)));
        assert_eq!(fs::read(dest.join("sub/b.yarground")).unwrap(), vec![8u8; 10]);
        assert_eq!(installed(&dest), 2);
        // again: nothing to do
        assert_eq!(plan(&files, &dest).todo, 0);
        assert_eq!(copy(&files, &dest, 0, &|| Some(u64::MAX), &mut |_, _| {}).unwrap(), (0, 0));
    }

    #[test]
    fn a_copy_stops_when_the_room_left_would_go_below_the_reserve() {
        let root = tmp("full-src");
        let dest = tmp("full-dest");
        fs::create_dir_all(root.join("venues")).unwrap();
        fs::write(root.join("venues/big.yarground"), vec![1u8; 4 << 20]).unwrap();
        let files = find(&root).unwrap();
        // 4 MB file, 1 GB reserve, 1.003 GB free: refused before anything is written
        assert_eq!(copy(&files, &dest, 1 << 20, &|| Some((1 << 20) + 3000), &mut |_, _| {}), Err(Stop::Full));
        assert_eq!(installed(&dest), 0);
        assert!(copy(&files, &dest, 1 << 20, &|| Some((1 << 20) + 8000), &mut |_, _| {}).is_ok());
    }

    #[test]
    fn removing_deletes_files_and_folders_but_follows_no_link() {
        let dest = tmp("rm-dest");
        let outside = tmp("rm-outside");
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        fs::create_dir_all(dest.join("sub")).unwrap();
        fs::write(dest.join("a.yarground"), b"x").unwrap();
        fs::write(dest.join("sub/b.png"), b"y").unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("link")).unwrap();
        assert_eq!(remove_all(&dest), Ok(2));
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0, "the folder stays, empty");
        assert!(outside.join("keep.txt").exists(), "what the link pointed to is untouched");
        assert_eq!(remove_all(Path::new("/nonexistent/venue")), Ok(0));
    }
}
