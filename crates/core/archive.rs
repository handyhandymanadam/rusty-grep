/*!
Support for searching the members of zip and tar archives (`-z/--search-zip`).

Each member is searched as if it were a file named `ARCHIVE{MEMBER}`, so
matches show which file inside the archive they came from.
*/

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// The kind of archive a path names, judged by its file name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    /// A zip file (including `.jar`, `.whl`, and similar).
    Zip,
    /// A tar file, possibly compressed (`.tar.gz`, `.tgz`, ...). Any
    /// compression is handled by the caller before the tar data is read.
    Tar,
}

impl Kind {
    /// Returns the archive kind for the given path, if its name looks like an
    /// archive.
    pub(crate) fn of(path: &Path) -> Option<Kind> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        const ZIP: &[&str] = &[".zip", ".jar", ".whl", ".apk"];
        const TAR: &[&str] = &[
            ".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz",
            ".txz", ".tar.zst", ".tar.lz4", ".tar.lzma", ".tar.z",
        ];
        if ZIP.iter().any(|ext| name.ends_with(ext)) {
            Some(Kind::Zip)
        } else if TAR.iter().any(|ext| name.ends_with(ext)) {
            Some(Kind::Tar)
        } else {
            None
        }
    }
}

/// The display path of a member inside an archive: `archive{member}`.
fn member_path(archive: &Path, member: &str) -> PathBuf {
    let mut s = archive.as_os_str().to_os_string();
    s.push("{");
    s.push(member);
    s.push("}");
    PathBuf::from(s)
}

/// Calls `search` once for every regular file inside the zip archive at
/// `path`, passing the member's display path and its contents.
pub(crate) fn each_zip_member<F>(path: &Path, mut search: F) -> io::Result<()>
where
    F: FnMut(&Path, &mut dyn Read) -> io::Result<()>,
{
    let mut archive = zip::ZipArchive::new(File::open(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if !entry.is_file() {
            continue;
        }
        let display = member_path(path, entry.name());
        search(&display, &mut entry)?;
    }
    Ok(())
}

/// Calls `search` once for every regular file in the tar data read from
/// `rdr`, passing the member's display path (based on `archive`) and its
/// contents.
pub(crate) fn each_tar_member<R, F>(
    archive: &Path,
    rdr: R,
    mut search: F,
) -> io::Result<()>
where
    R: Read,
    F: FnMut(&Path, &mut dyn Read) -> io::Result<()>,
{
    let mut tar = tar::Archive::new(rdr);
    for entry in tar.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path()?.to_string_lossy().into_owned();
        let display = member_path(archive, &name);
        search(&display, &mut entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        let k = |s: &str| Kind::of(Path::new(s));
        assert_eq!(k("a.zip"), Some(Kind::Zip));
        assert_eq!(k("dir/A.JAR"), Some(Kind::Zip));
        assert_eq!(k("a.tar"), Some(Kind::Tar));
        assert_eq!(k("a.tar.gz"), Some(Kind::Tar));
        assert_eq!(k("a.tgz"), Some(Kind::Tar));
        assert_eq!(k("a.gz"), None);
        assert_eq!(k("a.txt"), None);
        assert_eq!(k("tar"), None);
    }

    #[test]
    fn member_display() {
        assert_eq!(
            member_path(Path::new("x/a.zip"), "d/f.txt"),
            PathBuf::from("x/a.zip{d/f.txt}")
        );
    }
}
