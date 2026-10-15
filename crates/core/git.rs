/*!
`--git-modified` and `--git-tracked`: search only the files git says are
changed, or known to git.

The lists come from the `git` program (`git ls-files`, `git status`), run once
when the options are read. Outside a git repository, or without `git`, that is
an error (`G0011`), never an empty search.
*/

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
};

use crate::agent::{Coded, Severity};

/// The set of files a search is limited to.
#[derive(Debug)]
pub(crate) struct GitFilter {
    /// Absolute paths (with the real path of the working directory).
    files: HashSet<PathBuf>,
    /// The real path of the current directory, for turning the relative paths
    /// the walk makes into absolute ones.
    cwd: PathBuf,
    /// (search path as given, its real path), for paths through symlinks.
    remap: Vec<(PathBuf, PathBuf)>,
}

fn git_error(what: &str, stderr: &[u8], not_installed: bool) -> anyhow::Error {
    let detail = String::from_utf8_lossy(stderr).trim().to_string();
    let text = if not_installed {
        format!(
            "--git-modified and --git-tracked need the git program, which \
             could not be run: {what}"
        )
    } else {
        format!(
            "--git-modified and --git-tracked need a git repository: {}",
            if detail.is_empty() { what } else { &detail }
        )
    };
    let mut d = Coded::new("G0011", Severity::Fatal, text);
    d.suggestions.push((
        if not_installed {
            "install git, or search without the git options".to_string()
        } else {
            "run it inside a git repository, or search without the git \
             options"
                .to_string()
        },
        None,
    ));
    anyhow::Error::new(d)
}

fn run_git(root: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("git")
        .env_remove("RUSTY_AGENT_FD")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| git_error(&e.to_string(), b"", true))?;
    if !out.status.success() {
        return Err(git_error("git failed", &out.stderr, false));
    }
    Ok(out.stdout)
}

impl GitFilter {
    /// Ask git about the repositories that hold the search paths (the current
    /// directory when there are none). With both `tracked` and `modified`,
    /// the files that are both.
    pub(crate) fn new(
        cwd: &Path,
        search: &[PathBuf],
        tracked: bool,
        modified: bool,
    ) -> anyhow::Result<GitFilter> {
        let real_cwd =
            std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        // The folder each search path lives in; standard input is no path.
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut remap: Vec<(PathBuf, PathBuf)> = Vec::new();
        for p in search {
            if p.as_os_str() == "-" {
                continue;
            }
            let abs = if p.is_absolute() { p.clone() } else { cwd.join(p) };
            let dir = if abs.is_dir() {
                abs.clone()
            } else {
                abs.parent().map(Path::to_path_buf).unwrap_or(abs.clone())
            };
            if let Ok(real) = std::fs::canonicalize(&abs) {
                if real != abs {
                    remap.push((abs.clone(), real));
                }
            }
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        if dirs.is_empty() {
            dirs.push(cwd.to_path_buf());
        }
        let mut files: HashSet<PathBuf> = HashSet::new();
        let mut tops: Vec<PathBuf> = Vec::new();
        for dir in &dirs {
            let top = run_git(dir, &["rev-parse", "--show-toplevel"])?;
            let top = PathBuf::from(
                String::from_utf8_lossy(&top).trim_end().to_string(),
            );
            if tops.contains(&top) {
                continue;
            }
            files.extend(Self::repo_files(&top, tracked, modified)?);
            tops.push(top);
        }
        Ok(GitFilter { files, cwd: real_cwd, remap })
    }

    /// The files of one repository that pass `--git-tracked` /
    /// `--git-modified`.
    fn repo_files(
        root: &Path,
        tracked: bool,
        modified: bool,
    ) -> anyhow::Result<HashSet<PathBuf>> {
        let mut tracked_set: Option<HashSet<PathBuf>> = None;
        if tracked {
            let out = run_git(root, &["ls-files", "-z", "--full-name"])?;
            tracked_set = Some(
                out.split(|&b| b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| root.join(bytes_to_path(p)))
                    .collect(),
            );
        }
        let mut modified_set: Option<HashSet<PathBuf>> = None;
        if modified {
            let out = run_git(
                root,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )?;
            let mut set = HashSet::new();
            let mut parts = out.split(|&b| b == 0).filter(|p| !p.is_empty());
            while let Some(entry) = parts.next() {
                if entry.len() < 4 {
                    continue;
                }
                let (x, y) = (entry[0], entry[1]);
                if x == b'!' {
                    continue; // ignored
                }
                set.insert(root.join(bytes_to_path(&entry[3..])));
                if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
                    parts.next(); // the old name
                }
            }
            modified_set = Some(set);
        }
        Ok(match (tracked_set, modified_set) {
            (Some(t), Some(m)) => t.intersection(&m).cloned().collect(),
            (Some(t), None) => t,
            (None, Some(m)) => m,
            (None, None) => HashSet::new(),
        })
    }

    /// Whether the file found by the walk is one of the git files.
    pub(crate) fn allows(&self, path: &Path) -> bool {
        let path = path.strip_prefix("./").unwrap_or(path);
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        if self.files.contains(&abs) {
            return true;
        }
        // A search path that goes through a symbolic link: git names the
        // real place.
        self.remap.iter().any(|(given, real)| {
            abs.strip_prefix(given)
                .is_ok_and(|rest| self.files.contains(&real.join(rest)))
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.files.len()
    }
}

#[cfg(unix)]
fn bytes_to_path(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

#[cfg(not(unix))]
fn bytes_to_path(b: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(b).into_owned())
}
