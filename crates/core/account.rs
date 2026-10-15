/*!
Accounting for agent mode: which files the filters hid from the search, and
why.

Paths are compared by a 64-bit hash of their bytes, so the walks that find
the hidden files do not have to keep every path of a big tree in memory (the
paths of the hidden files themselves are kept, for the examples). The number
of hidden files is always worked out; the reason for each one (one extra
walk per filter that is switched on) only when it was asked for or the search
was small, because on a big tree each walk costs about as much as a search.
*/

use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasherDefault, Hasher},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::flags::{HiArgs, Relax};

/// A hasher for keys that already are hashes.
#[derive(Default)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type IdSet = HashSet<u64, BuildHasherDefault<IdHasher>>;

/// A 64-bit hash of the bytes of a path (FNV-1a, with a final mix).
pub(crate) fn path_id(p: &Path) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in p.as_os_str().as_encoded_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^ (h >> 29)
}

/// The hash of a folder's path, ignoring a trailing slash (the walk names its
/// root `./`, while `Path::parent` gives `.`).
pub(crate) fn dir_id(p: &Path) -> u64 {
    let b = p.as_os_str().as_encoded_bytes();
    let b = match b {
        [rest @ .., b'/'] if !rest.is_empty() => rest,
        _ => b,
    };
    let mut h: u64 = 0xcbf29ce484222325;
    for &c in b {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^ (h >> 29)
}

/// One group of hidden files, as reported.
pub(crate) struct GroupOut {
    pub(crate) reason: &'static str,
    pub(crate) option: Option<&'static str>,
    pub(crate) advice: &'static str,
    pub(crate) files: u64,
    /// The largest hidden subtrees (a trailing `/`) or files.
    pub(crate) examples: Vec<(String, u64)>,
}

/// What the filters hid.
pub(crate) struct Skipped {
    /// How many files the filters hid (binary files are not counted here).
    pub(crate) total: u64,
    /// The groups; when `classified` is false there is one group whose
    /// reason is `"unclassified"`.
    pub(crate) groups: Vec<GroupOut>,
    pub(crate) classified: bool,
}

impl Skipped {
    fn none() -> Skipped {
        Skipped { total: 0, groups: Vec::new(), classified: true }
    }
}

/// The files the search itself read, and the folders that held them, as
/// hashes. A search that knows them saves the accounting its first walk.
#[derive(Default)]
pub(crate) struct SearchedSet {
    /// Sorted once collection is over (`sort`), then searched by bisection:
    /// merging thousands of hashes into one hash set costs more.
    pub(crate) files: Vec<u64>,
    pub(crate) dirs: IdSet,
}

impl SearchedSet {
    /// Remember a searched file: its hash, and the folders above it.
    pub(crate) fn add(&mut self, path: &Path) {
        self.files.push(path_id(path));
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !self.dirs.insert(dir_id(d)) {
                break;
            }
            dir = d.parent();
        }
    }

    pub(crate) fn merge(&mut self, mut other: SearchedSet) {
        self.files.append(&mut other.files);
        self.dirs.extend(other.dirs);
    }

    pub(crate) fn sort(&mut self) {
        self.files.sort_unstable();
        self.files.dedup();
    }

    fn has(&self, id: u64) -> bool {
        self.files.binary_search(&id).is_ok()
    }
}

/// Hidden files counted by the top of their subtree.
#[derive(Default)]
struct Roots {
    count: u64,
    /// Top-of-subtree hash -> (path, files below it, the top is a file).
    tops: HashMap<u64, (PathBuf, u64, bool)>,
}

impl Roots {
    fn add(&mut self, file: &Path, dirs: &IdSet) {
        let mut top = file;
        while let Some(parent) = top.parent() {
            if dirs.contains(&dir_id(parent)) {
                break;
            }
            top = parent;
        }
        self.count += 1;
        let is_file = top == file;
        self.tops
            .entry(path_id(top))
            .or_insert_with(|| (top.to_path_buf(), 0, is_file))
            .1 += 1;
    }

    fn merge(&mut self, other: Roots) {
        self.count += other.count;
        for (k, v) in other.tops {
            self.tops.entry(k).or_insert((v.0, 0, v.2)).1 += v.1;
        }
    }

    fn describe(
        self,
        args: &HiArgs,
        reason: &'static str,
        option: Option<&'static str>,
        advice: &'static str,
    ) -> GroupOut {
        let mut tops: Vec<(PathBuf, u64, bool)> =
            self.tops.into_values().collect();
        tops.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        tops.truncate(crate::agent::EXAMPLES);
        let examples = tops
            .into_iter()
            .map(|(p, n, is_file)| {
                let mut shown = display(args, &p);
                if !is_file {
                    shown.push('/');
                }
                (shown, n)
            })
            .collect();
        GroupOut { reason, option, advice, files: self.count, examples }
    }
}

enum Want<'a> {
    /// Every file and directory, as hashes.
    Base,
    /// Count the files whose hash is not in the first set, by the top of
    /// their subtree (the second set holds the folders the search visited).
    Cheap(&'a SearchedSet),
    /// The paths of the files whose hash is in the set.
    Select(&'a IdSet),
    /// The paths of the files whose hash is not in the set.
    NotIn(&'a SearchedSet),
}

#[derive(Default)]
struct Walked {
    files: Vec<u64>,
    dirs: IdSet,
    paths: Vec<PathBuf>,
    roots: Roots,
    /// An ignore file (or a `.git` folder) turned up in the walk.
    saw_ignore: bool,
}

/// Walks of fewer files than this run on the calling thread: starting a pool
/// of workers costs more than a small tree.
const SERIAL_BELOW: usize = 3000;

/// What one walk thread collects, handed over when the thread is done.
struct Local<'a> {
    files: Vec<u64>,
    dirs: Vec<u64>,
    paths: Vec<PathBuf>,
    roots: Roots,
    saw_ignore: bool,
    out: &'a Mutex<Walked2>,
}

#[derive(Default)]
struct Walked2 {
    files: Vec<u64>,
    dirs: Vec<u64>,
    paths: Vec<PathBuf>,
    roots: Roots,
    saw_ignore: bool,
}

impl Drop for Local<'_> {
    fn drop(&mut self) {
        let mut out = self.out.lock().unwrap();
        out.files.append(&mut self.files);
        out.dirs.append(&mut self.dirs);
        out.paths.append(&mut self.paths);
        out.roots.merge(std::mem::take(&mut self.roots));
        out.saw_ignore |= self.saw_ignore;
    }
}

impl Local<'_> {
    fn visit(
        &mut self,
        args: &HiArgs,
        use_git: bool,
        want: &Want<'_>,
        ent: &ignore::DirEntry,
    ) {
        if matches!(want, Want::NotIn(_)) && !self.saw_ignore {
            self.saw_ignore = matches!(
                ent.file_name().as_encoded_bytes(),
                b".gitignore" | b".ignore" | b".rgignore" | b".git"
            );
        }
        match ent.file_type() {
            Some(t) if t.is_file() => {
                let keep = !use_git
                    || ent.depth() == 0
                    || args
                        .git_filter()
                        .map_or(true, |g| g.allows(ent.path()));
                if !keep {
                    return;
                }
                let id = path_id(ent.path());
                match want {
                    Want::Base => self.files.push(id),
                    Want::Cheap(seen) => {
                        if !seen.has(id) {
                            self.roots.add(ent.path(), &seen.dirs);
                        }
                    }
                    Want::Select(set) => {
                        if set.contains(&id) {
                            self.paths.push(ent.path().to_path_buf());
                        }
                    }
                    Want::NotIn(set) => {
                        if !set.has(id) {
                            self.paths.push(ent.path().to_path_buf());
                        }
                    }
                }
            }
            Some(t) if t.is_dir() => {
                if matches!(want, Want::Base) {
                    self.dirs.push(dir_id(ent.path()));
                }
            }
            _ => {}
        }
    }
}

/// Walk the tree like the search does, with the filters in `relax` off.
/// `small` walks on this thread.
fn walk(
    args: &HiArgs,
    relax: &Relax,
    use_git: bool,
    want: Want<'_>,
    small: bool,
) -> anyhow::Result<Walked> {
    let out = Mutex::new(Walked2::default());
    let new_local = || Local {
        files: Vec::new(),
        dirs: Vec::new(),
        paths: Vec::new(),
        roots: Roots::default(),
        saw_ignore: false,
        out: &out,
    };
    let mut builder = args.walk_builder_relaxed(relax)?;
    if small {
        let mut local = new_local();
        for result in builder.threads(1).build() {
            if let Ok(ent) = result {
                local.visit(args, use_git, &want, &ent);
            }
        }
        drop(local);
    } else {
        let want = &want;
        builder.build_parallel().run(|| {
            let mut local = new_local();
            Box::new(move |result| {
                if let Ok(ent) = result {
                    local.visit(args, use_git, want, &ent);
                }
                ignore::WalkState::Continue
            })
        });
    }
    let out = out.into_inner().unwrap();
    let mut walked = Walked::default();
    walked.files = out.files;
    walked.dirs.extend(out.dirs);
    walked.paths = out.paths;
    walked.roots = out.roots;
    walked.saw_ignore = out.saw_ignore;
    Ok(walked)
}

/// A path the way ripgrep prints it.
pub(crate) fn display(args: &HiArgs, p: &Path) -> String {
    let p = if args.has_implicit_path() {
        p.strip_prefix("./").unwrap_or(p)
    } else {
        p
    };
    p.to_string_lossy().into_owned()
}

/// A hidden file group while it is being worked out.
struct Group {
    reason: &'static str,
    option: Option<&'static str>,
    advice: &'static str,
    files: Vec<PathBuf>,
}

/// Work out what the filters hid. With `thorough` the reasons are found (one
/// walk per filter that is on, until every hidden file has one); without it
/// the hidden files are only counted and their biggest subtrees named.
pub(crate) fn skipped(
    args: &HiArgs,
    thorough: bool,
    known: Option<SearchedSet>,
) -> anyhow::Result<Skipped> {
    let active = args.active_filters();
    let git = args.git_filter().is_some();
    if !active.any() && !git {
        return Ok(Skipped::none());
    }
    let mut base = match known {
        Some(k) => k,
        None => {
            let w = walk(args, &Relax::default(), true, Want::Base, false)?;
            SearchedSet { files: w.files, dirs: w.dirs }
        }
    };
    base.sort();
    let small = base.files.len() < SERIAL_BELOW;
    // The search paths themselves were visited, even if nothing under them
    // was searched.
    for root in args.search_paths() {
        base.dirs.insert(dir_id(root));
    }
    let git_advice = "files that git does not list as modified or tracked \
                      (drop --git-modified / --git-tracked)";
    if !thorough {
        let mut groups = Vec::new();
        let mut total = 0;
        if git {
            // Files the plain walk finds but the git list leaves out.
            let w = walk(
                args,
                &Relax::default(),
                false,
                Want::Cheap(&base),
                small,
            )?;
            if w.roots.count > 0 {
                total += w.roots.count;
                groups.push(w.roots.describe(args, "git", None, git_advice));
            }
        }
        if active.any() {
            let w = walk(args, &active, true, Want::Cheap(&base), small)?;
            if w.roots.count > 0 {
                total += w.roots.count;
                groups.push(w.roots.describe(
                    args,
                    "unclassified",
                    None,
                    "files hidden by the filters",
                ));
            }
        }
        groups.sort_by(|a, b| b.files.cmp(&a.files));
        return Ok(Skipped { total, groups, classified: false });
    }
    let full = walk(args, &active, true, Want::NotIn(&base), small)?;
    // Ignore rules can only have hidden something if an ignore file or a
    // repository turned up in the tree, or one lives outside it.
    let ignore_possible = full.saw_ignore || ignore_source_outside(args);
    let hidden = full.paths;
    let mut groups: Vec<Group> = Vec::new();
    if git {
        let files =
            walk(args, &Relax::default(), false, Want::NotIn(&base), small)?
                .paths;
        if !files.is_empty() {
            groups.push(Group {
                reason: "git",
                option: None,
                advice: git_advice,
                files,
            });
        }
    }
    let mut left: HashSet<PathBuf> = hidden.into_iter().collect();
    let single: [(&str, Option<&str>, &str, Relax, bool); 10] = [
        (
            "hidden",
            Some("--hidden"),
            "hidden files and directories (names starting with a dot)",
            Relax { hidden: true, ..Relax::default() },
            active.hidden,
        ),
        (
            "ignored-vcs",
            Some("--no-ignore-vcs"),
            "files matched by .gitignore or git's exclude files",
            Relax { vcs: true, ..Relax::default() },
            active.vcs,
        ),
        (
            "ignored-parent",
            Some("--no-ignore-parent"),
            "files matched by ignore files in parent directories",
            Relax { parent: true, ..Relax::default() },
            active.parent,
        ),
        (
            "ignored-dot",
            Some("--no-ignore-dot"),
            "files matched by .ignore or .rgignore files",
            Relax { dot: true, ..Relax::default() },
            active.dot,
        ),
        (
            "ignored-files",
            Some("--no-ignore-files"),
            "files matched by --ignore-file files",
            Relax { ignore_files: true, ..Relax::default() },
            active.ignore_files,
        ),
        (
            "size",
            None,
            "files larger than --max-filesize (raise or remove it)",
            Relax { size: true, ..Relax::default() },
            active.size,
        ),
        (
            "glob",
            None,
            "files excluded by -g/--glob (change or remove the globs)",
            Relax { globs: true, ..Relax::default() },
            active.globs,
        ),
        (
            "type",
            None,
            "files excluded by -t/-T/--type options (change or remove them)",
            Relax { types: true, ..Relax::default() },
            active.types,
        ),
        (
            "depth",
            None,
            "files deeper than --max-depth (raise or remove it)",
            Relax { depth: true, ..Relax::default() },
            active.depth,
        ),
        (
            "symlink",
            Some("--follow"),
            "symbolic links that were not followed",
            Relax { symlinks: true, ..Relax::default() },
            active.symlinks,
        ),
    ];
    let mut ids: IdSet = left.iter().map(|p| path_id(p)).collect();
    for (reason, option, advice, relax, is_active) in single {
        if left.is_empty() {
            break;
        }
        if !is_active || (reason.starts_with("ignored-") && !ignore_possible) {
            continue;
        }
        let w = walk(args, &relax, true, Want::Select(&ids), small)?;
        let mut files: Vec<PathBuf> = Vec::new();
        for p in w.paths {
            if left.remove(&p) {
                ids.remove(&path_id(&p));
                files.push(p);
            }
        }
        if !files.is_empty() {
            groups.push(Group { reason, option, advice, files });
        }
    }
    // Combined ignore rules: all the ignore kinds off together.
    let ignore = Relax {
        vcs: active.vcs,
        parent: active.parent,
        dot: active.dot,
        ignore_files: active.ignore_files,
        ..Relax::default()
    };
    if !left.is_empty() && ignore.any() && ignore_possible {
        let w = walk(args, &ignore, true, Want::Select(&ids), small)?;
        let files: Vec<PathBuf> =
            w.paths.into_iter().filter(|p| left.remove(p)).collect();
        if !files.is_empty() {
            groups.push(Group {
                reason: "ignored",
                option: Some("--no-ignore"),
                advice: "files matched by several kinds of ignore rule",
                files,
            });
        }
    }
    if !left.is_empty() {
        let files: Vec<PathBuf> = left.into_iter().collect();
        // Only the options that can bring these files back: --hidden when a
        // name on the way starts with a dot, --follow when a symbolic link
        // is on the way, --no-ignore when ignore rules were on (a rule that
        // hid a file cannot be told from here).
        let roots = args.search_paths();
        let mut parts = Vec::new();
        if active.hidden && files.iter().any(|p| has_hidden_part(p, roots)) {
            parts.push("--hidden");
        }
        if ignore.any() && ignore_possible {
            parts.push("--no-ignore");
        }
        if active.symlinks
            && files.iter().take(200).any(|p| has_symlink_part(p, roots))
        {
            parts.push("--follow");
        }
        let option: Option<&'static str> = match parts.as_slice() {
            [] => None,
            ["--hidden"] => Some("--hidden"),
            ["--no-ignore"] => Some("--no-ignore"),
            ["--follow"] => Some("--follow"),
            ["--hidden", "--no-ignore"] => Some("--hidden --no-ignore"),
            ["--hidden", "--follow"] => Some("--hidden --follow"),
            ["--no-ignore", "--follow"] => Some("--no-ignore --follow"),
            _ => Some("--hidden --no-ignore --follow"),
        };
        groups.push(Group {
            reason: "several",
            option,
            advice: "files hidden by a combination of filters (hidden, \
                     ignore rules, size, globs, types, depth)",
            files,
        });
    }
    let mut out: Vec<GroupOut> =
        groups.into_iter().map(|g| describe(args, g, &base.dirs)).collect();
    // The biggest group first; equal sizes keep the order of the rules.
    out.sort_by(|a, b| b.files.cmp(&a.files));
    let total = out.iter().map(|g| g.files).sum();
    Ok(Skipped { total, groups: out, classified: true })
}

/// Whether an ignore rule could come from outside the tree walked: ignore
/// files in the folders above a search path, `--ignore-file`, or git's global
/// ignore file.
fn ignore_source_outside(args: &HiArgs) -> bool {
    if args.has_ignore_files() {
        return true;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let global = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))
        .map(|c| c.join("git").join("ignore"));
    if global.is_some_and(|g| g.exists()) {
        return true;
    }
    // A global ignore file named in git's own settings.
    let configs = [
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gitconfig")),
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h).join(".config").join("git").join("config")
        }),
    ];
    if configs.iter().flatten().any(|c| {
        std::fs::read_to_string(c)
            .is_ok_and(|t| t.to_ascii_lowercase().contains("excludesfile"))
    }) {
        return true;
    }
    args.search_paths().iter().any(|root| {
        let abs = std::fs::canonicalize(root).unwrap_or(root.clone());
        abs.ancestors().any(|dir| {
            [".gitignore", ".ignore", ".rgignore"]
                .iter()
                .any(|n| dir.join(n).exists())
                || dir.join(".git").exists()
        })
    })
}

/// The part of the path below the search path it was found under.
fn below_root<'a>(p: &'a Path, roots: &[PathBuf]) -> &'a Path {
    roots.iter().find_map(|r| p.strip_prefix(r).ok()).unwrap_or(p)
}

/// Whether a name below the search path starts with a dot.
fn has_hidden_part(p: &Path, roots: &[PathBuf]) -> bool {
    below_root(p, roots).components().any(|c| {
        c.as_os_str().as_encoded_bytes().first() == Some(&b'.')
            && !matches!(c, std::path::Component::CurDir)
            && c.as_os_str().len() > 1
            && c.as_os_str() != ".."
    })
}

/// Whether the file, or a folder on the way to it below the search path, is
/// a symbolic link.
fn has_symlink_part(p: &Path, roots: &[PathBuf]) -> bool {
    let rest = below_root(p, roots);
    let base = roots.iter().find(|r| p.starts_with(r)).map(|r| r.as_path());
    let mut cur = base.unwrap_or(Path::new("")).to_path_buf();
    for c in rest.components() {
        cur.push(c);
        if std::fs::symlink_metadata(&cur)
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

/// Turn a group into its report: the count and the biggest hidden subtrees.
fn describe(args: &HiArgs, g: Group, dirs: &IdSet) -> GroupOut {
    // The top of each hidden subtree: the outermost ancestor whose parent
    // the plain walk did visit.
    let mut roots: HashMap<PathBuf, u64> = HashMap::new();
    for p in &g.files {
        let mut top = p.as_path();
        while let Some(parent) = top.parent() {
            if dirs.contains(&dir_id(parent)) {
                break;
            }
            top = parent;
        }
        *roots.entry(top.to_path_buf()).or_insert(0) += 1;
    }
    let mut roots: Vec<(PathBuf, u64)> = roots.into_iter().collect();
    roots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    roots.truncate(crate::agent::EXAMPLES);
    let file_ids: IdSet = g.files.iter().map(|p| path_id(p)).collect();
    let examples = roots
        .iter()
        .map(|(p, n)| {
            let mut shown = display(args, p);
            if !file_ids.contains(&path_id(p)) {
                shown.push('/');
            }
            (shown, *n)
        })
        .collect();
    GroupOut {
        reason: g.reason,
        option: g.option,
        advice: g.advice,
        files: g.files.len() as u64,
        examples,
    }
}
