//! Persistent per-file extraction cache.
//!
//! Why this exists: extraction — one tree-sitter parse plus one AST walk per
//! file — is ~93% of a graph build's CPU. Extraction is a pure function of
//! (source bytes, language): the [`ImportPlugin`] trait takes only
//! `(&self, source: &[u8], node/tree)`, and the cached value
//! ([`CachedExtraction`]) carries no paths. So a content-addressed cache of
//! extraction results cannot change any output; it only skips re-parsing
//! files whose bytes have not changed.
//!
//! [`ImportPlugin`]: sephera_core::plugins::ImportPlugin
//!
//! Three properties keep the cache honest:
//!
//! * **Content-addressed.** The key is `blake3(language || 0x00 || bytes)`.
//!   The language is part of the key because identical bytes under `.js`
//!   and `.ts` parse with different grammars. A file's bytes changing is
//!   a different key, so there is no invalidation to get wrong — the hash
//!   *is* the invalidation.
//! * **Best-effort.** Every cache operation degrades to a miss or a no-op
//!   on any I/O error. A poisoned, corrupt, or half-written entry fails to
//!   parse and is recomputed; an unwritable cache directory disables the
//!   cache for the run. The cache can never fail a build.
//! * **Bounded per namespace.** Each namespace — one per analyzed repo or
//!   worktree family — is capped at 1 GiB; over the cap, oldest-created
//!   entries are evicted first. mtime orders eviction only — clock skew
//!   cannot affect correctness.
//!
//! Concurrent processes share the store without locking: the same key always
//! holds the same bytes (deterministic extraction), each entry is written via
//! temp-file + atomic rename, and a reader that races a writer or an eviction
//! sees a missing or corrupt file — both of which are just a miss.
//!
//! Trust boundary, stated plainly: a *malformed* cache entry is ignored, but
//! a *well-formed yet wrong* entry is trusted. That matches the threat model
//! — the cache lives in the user's own cache directory, and whoever can
//! write there can already write the analyzed repo. `blake3-256` makes
//! accidental collision infeasible.
//!
//! Escape hatches: `SEPHERA_NO_CACHE=1` disables the cache entirely;
//! `SEPHERA_CACHE_DIR` overrides the root (tests, hermetic runs).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Version of the on-disk entry format. Bumped when the meaning of a cached
/// value changes; old entries are then treated as misses, never misread.
const FORMAT_VERSION: u8 = 1;

/// Revision of the extractor itself: the plugin walk, the tree-sitter
/// grammars, and the meaning of every cached field. Bumped with any of
/// those, because a grammar or extractor fix does not bump the sephera
/// version and would otherwise keep serving stale entries. Kept separate
/// from [`FORMAT_VERSION`], which only tracks the serialization shape.
///
/// Revision history:
/// - 1: initial revision (extraction cache introduced).
/// - 2: the Rust extractor passes `#[path = "..."]` module declarations
///   through to the resolver instead of skipping them.
const EXTRACTOR_REVISION: u16 = 2;

/// The most one namespace's store may hold. Past this, the oldest-created
/// entries are evicted first.
const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Per-write temp-name counter. Two processes may store the same key
/// concurrently; sharing one temp name would let their bytes interleave.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One file's extraction result, as stored on disk.
///
/// Built by [`CachedExtraction::new`], which stamps the format version, the
/// extractor revision, and the sephera version: a sephera upgrade, a grammar
/// or extractor change, or a new serialization shape silently invalidates
/// every entry instead of misreading one.
//
// `Clone` is deliberately absent: the miss and hit paths move the pieces
// instead of copying them (see `attach_cached`), and a stray `.clone()`
// on a declaration map would be the most expensive line in this module.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedExtraction {
    format: u8,
    extractor: u16,
    sephera: String,
    pub imports: Vec<sephera_core::types::ImportStatement>,
    pub declared: Option<sephera_core::declarations::DeclaredNames>,
    pub boundary: Option<sephera_core::boundary::FileBoundary>,
}

impl CachedExtraction {
    /// The extraction result of one file, stamped for the cache.
    #[must_use]
    pub fn new(
        imports: Vec<sephera_core::types::ImportStatement>,
        declared: Option<sephera_core::declarations::DeclaredNames>,
        boundary: Option<sephera_core::boundary::FileBoundary>,
    ) -> Self {
        Self {
            format: FORMAT_VERSION,
            extractor: EXTRACTOR_REVISION,
            sephera: env!("CARGO_PKG_VERSION").to_owned(),
            imports,
            declared,
            boundary,
        }
    }

    /// Whether this entry was written by a sephera that means the same
    /// thing by it as this one does.
    fn is_current(&self) -> bool {
        self.format == FORMAT_VERSION
            && self.extractor == EXTRACTOR_REVISION
            && self.sephera == env!("CARGO_PKG_VERSION")
    }
}

/// Best-effort accounting of the store's size, so the common case needs no
/// directory walk.
#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    bytes: u64,
}

/// The persistent extraction cache for one analysis base.
///
/// Construct with [`ExtractionCache::for_base`]; `None` inside means disabled
/// (see the module docs for why), and every method then becomes a no-op.
#[derive(Debug)]
pub struct ExtractionCache {
    dir: Option<PathBuf>,
    /// Best-effort size accounting, batched: `meta.json` is read once at
    /// construction, additions accumulate in memory, and the total is
    /// flushed back on drop. This keeps the per-store cost at one integer
    /// add instead of a read-modify-write cycle per file; the 1 GiB cap
    /// stays approximate, which is all a best-effort cache needs.
    bytes_base: AtomicU64,
    bytes_added: AtomicU64,
}

impl ExtractionCache {
    /// The cache for analyses rooted at `base`, or a disabled cache when
    /// the environment says so or no cache directory resolves.
    ///
    /// The namespace is the canonical git common directory when `base` sits
    /// in a git repo — linked worktrees share one common dir, so both sides
    /// of a `breaking --from/--to` run share one namespace — else the
    /// canonicalized base path. The namespace only partitions the store;
    /// correctness never depends on it, because entries are content-addressed.
    #[must_use]
    pub fn for_base(base: &Path) -> Self {
        let disabled = Self {
            dir: None,
            bytes_base: AtomicU64::new(0),
            bytes_added: AtomicU64::new(0),
        };
        if std::env::var_os("SEPHERA_NO_CACHE").is_some() {
            return disabled;
        }
        let Some(root) = cache_root() else {
            return disabled;
        };
        let namespace = namespace_for(base);
        let dir = root.join("extract-v1").join(namespace);
        if fs::create_dir_all(&dir).is_err() {
            return disabled;
        }
        let bytes_base = fs::read(dir.join("meta.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CacheMeta>(&bytes).ok())
            .map_or(0, |meta| meta.bytes);
        Self {
            dir: Some(dir),
            bytes_base: AtomicU64::new(bytes_base),
            bytes_added: AtomicU64::new(0),
        }
    }

    /// The cached extraction of `(language, source)`, if one was stored by
    /// a sephera that means the same thing by it.
    ///
    /// Any failure — missing file, unreadable file, unparsable JSON, stale
    /// version — is a miss, never an error.
    #[must_use]
    pub fn load(
        &self,
        language: &str,
        source: &[u8],
    ) -> Option<CachedExtraction> {
        let dir = self.dir.as_ref()?;
        let bytes = fs::read(entry_path(dir, language, source)).ok()?;
        let cached: CachedExtraction = serde_json::from_slice(&bytes).ok()?;
        cached.is_current().then_some(cached)
    }

    /// Store one file's extraction result. Best-effort: any I/O failure is
    /// silently dropped. The value is serialized as given: stamping happens
    /// in [`CachedExtraction::new`], the only constructor, so a re-stamp
    /// here would be a no-op — and the clone it required copied the whole
    /// declaration map on every miss.
    pub fn store(
        &self,
        language: &str,
        source: &[u8],
        value: &CachedExtraction,
    ) {
        let Some(dir) = self.dir.as_ref() else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(value) else {
            return;
        };
        let path = entry_path(dir, language, source);
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        // Same key means same bytes, so even a lost rename race is
        // harmless — but a corrupt temp is a wasted miss.
        let tmp = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if fs::write(&tmp, &bytes).is_err() {
            return;
        }
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
            return;
        }
        let new_bytes = bytes.len() as u64;
        let added = self
            .bytes_added
            .fetch_add(new_bytes, Ordering::Relaxed)
            .saturating_add(new_bytes);
        // Only when the cap is exceeded pay for a full walk to evict
        // oldest-first. Two threads crossing at once both walk; the
        // second finds nothing left to evict — harmless.
        if self
            .bytes_base
            .load(Ordering::Relaxed)
            .saturating_add(added)
            > MAX_CACHE_BYTES
        {
            let total = evict_oldest(dir);
            self.bytes_base.store(total, Ordering::Relaxed);
            self.bytes_added.store(0, Ordering::Relaxed);
        }
    }
}

impl Drop for ExtractionCache {
    /// Flush the batched size accounting. Skipped when nothing was stored,
    /// so a read-only build never clobbers a newer total written by a
    /// concurrent build; the write goes through a temp file + rename so a
    /// concurrent reader never sees a half-written `meta.json`. Best-effort
    /// like everything else here: a killed process leaves a stale (low)
    /// total, and the next over-cap walk corrects the drift.
    fn drop(&mut self) {
        let Some(dir) = self.dir.as_ref() else {
            return;
        };
        let added = self.bytes_added.load(Ordering::Relaxed);
        if added == 0 {
            return;
        }
        let total = self
            .bytes_base
            .load(Ordering::Relaxed)
            .saturating_add(added);
        let meta = CacheMeta { bytes: total };
        let Ok(bytes) = serde_json::to_vec(&meta) else {
            return;
        };
        let tmp = dir.join(format!(
            "meta.json.tmp-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if fs::write(&tmp, &bytes).is_err()
            || fs::rename(&tmp, dir.join("meta.json")).is_err()
        {
            let _ = fs::remove_file(&tmp);
        }
    }
}

/// The cache root: explicit override, else XDG, else `~/.cache`.
/// `None` means "no place to put a cache" — the caller disables itself.
fn cache_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SEPHERA_CACHE_DIR") {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Some(path);
        }
        return None;
    }
    fallback_cache_root()
}

/// The XDG/`~/.cache` fallback. In test builds this is compiled out and
/// the override above is the only way to get a cache: unit tests must
/// never touch the developer's real `~/.cache`, and resolver tests call
/// `build_graph` without the env lock, so an override left set by another
/// test would otherwise leak into them.
#[cfg(not(test))]
fn fallback_cache_root() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        let path = PathBuf::from(xdg);
        if path.is_absolute() {
            return Some(path.join("sephera"));
        }
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache").join("sephera"))
}

/// Test builds have no fallback: see [`fallback_cache_root`].
#[cfg(test)]
const fn fallback_cache_root() -> Option<PathBuf> {
    None
}

/// The namespace for analyses rooted at `base`: the canonical git common
/// directory when inside a git repo, else the canonicalized base path,
/// hashed to a fixed length.
fn namespace_for(base: &Path) -> String {
    let anchor = git_common_dir(base)
        .or_else(|| fs::canonicalize(base).ok())
        .unwrap_or_else(|| base.to_path_buf());
    blake3::hash(anchor.to_string_lossy().as_bytes()).to_hex()[..16].to_owned()
}

/// The `--git-common-dir` of the repo containing `base`, canonicalized.
/// Linked worktrees report their main repo's git dir, which is what lets
/// both sides of a two-ref comparison share one namespace.
fn git_common_dir(base: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(base)
        .arg("rev-parse")
        .arg("--git-common-dir")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    let absolute = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    fs::canonicalize(absolute).ok()
}

/// The content key: `blake3(language || 0x00 || bytes)`, hex.
///
/// The language is in the key because identical bytes under two extensions
/// parse with two grammars. The `0x00` separator keeps a language name that
/// is a prefix of another (`"c"` vs `"c..."`) from colliding with bytes
/// that start where the name ends.
fn cache_key(language: &str, source: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(language.as_bytes());
    hasher.update(&[0]);
    hasher.update(source);
    hasher.finalize().to_hex().to_string()
}

/// `<dir>/<shard>/<key>.json`, sharded by the key's first two hex digits so
/// no single directory grows without bound.
fn entry_path(dir: &Path, language: &str, source: &[u8]) -> PathBuf {
    let key = cache_key(language, source);
    dir.join(&key[..2]).join(format!("{key}.json"))
}

/// Evict down to this fraction of the cap, so the store after the next
/// miss is still under the cap instead of triggering another full walk.
const EVICT_TARGET_BYTES: u64 = MAX_CACHE_BYTES / 4 * 3;

/// Delete oldest-created entries until the store is back under the cap.
/// Returns the true total afterwards; mtime orders eviction only.
///
/// The total is recomputed from the walk itself rather than trusted from
/// the caller's estimate: `meta.json` is best-effort accounting, and a
/// stale-low estimate must not let the store sit over the cap. A directory
/// that cannot be walked counts as empty; the next successful walk
/// corrects the total.
fn evict_oldest(dir: &Path) -> u64 {
    evict_below(dir, MAX_CACHE_BYTES, EVICT_TARGET_BYTES)
}

/// Walk `dir`, and when over `cap`, delete oldest-created entries until at
/// or under `target`. Returns the true total of what remains. Split out so
/// the recompute and the low-water behavior are unit-testable without
/// filling a 1 GiB store.
fn evict_below(dir: &Path, cap: u64, target: u64) -> u64 {
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let Ok(shards) = fs::read_dir(dir) else {
        return 0;
    };
    for shard in shards.flatten() {
        if !shard.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(meta) = file.metadata() else {
                continue;
            };
            entries.push((
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
                path,
            ));
        }
    }
    let mut total: u64 = entries.iter().map(|entry| entry.1).sum();
    if total <= cap {
        return total;
    }
    entries.sort_by_key(|entry| entry.0);
    for (_, size, path) in &entries {
        if total <= target {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total = total.saturating_sub(*size);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, MutexGuard};

    /// The cache reads process-global env vars, and Rust runs tests on
    /// threads: hold this while mutating them so no two tests race.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// The env lock plus restoration. The old `temp_cache` leaked every
    /// override process-wide — a later (or concurrent) test without the
    /// lock, like the resolver tests that call `build_graph`, could then
    /// observe a `SEPHERA_CACHE_DIR` pointing at a deleted tempdir.
    /// Dropping the guard puts both variables back the way they were.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        prev_cache_dir: Option<OsString>,
        prev_no_cache: Option<OsString>,
    }

    impl EnvGuard {
        fn lock() -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Self {
                prev_cache_dir: std::env::var_os("SEPHERA_CACHE_DIR"),
                prev_no_cache: std::env::var_os("SEPHERA_NO_CACHE"),
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: the ENV_LOCK is held until the end of drop, so no
            // other test in this module observes a half-restored pair.
            unsafe {
                restore_var("SEPHERA_CACHE_DIR", self.prev_cache_dir.take());
                restore_var("SEPHERA_NO_CACHE", self.prev_no_cache.take());
            }
        }
    }

    /// Put an env var back the way it was before the guard locked.
    ///
    /// # Safety
    ///
    /// The caller must hold [`ENV_LOCK`].
    unsafe fn restore_var(name: &str, prev: Option<OsString>) {
        // SAFETY: see the contract above; edition 2024 still wants the
        // block spelled out inside an unsafe fn.
        unsafe {
            match prev {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    fn temp_cache() -> (tempfile::TempDir, EnvGuard) {
        let guard = EnvGuard::lock();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: the ENV_LOCK is held, so no other thread observes a
        // half-set pair.
        unsafe {
            std::env::set_var(
                "SEPHERA_CACHE_DIR",
                dir.path().join("cache-root"),
            );
            std::env::remove_var("SEPHERA_NO_CACHE");
        }
        (dir, guard)
    }

    fn sample_value() -> CachedExtraction {
        CachedExtraction::new(
            vec![sephera_core::types::ImportStatement::new("os", 1u64)],
            None,
            None,
        )
    }

    #[test]
    fn store_then_load_roundtrips() {
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));
        assert!(cache.dir.is_some());

        let source = b"import os\n";
        assert_eq!(cache.load("Python", source), None);
        cache.store("Python", source, &sample_value());
        assert_eq!(cache.load("Python", source), Some(sample_value()));
    }

    #[test]
    fn language_is_part_of_the_key() {
        // Identical bytes under two languages must not share an entry:
        // they parse with different grammars.
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        cache.store("Python", source, &sample_value());
        assert_eq!(cache.load("JavaScript", source), None);
        assert_eq!(cache.load("Python", source), Some(sample_value()));
    }

    #[test]
    fn corrupt_entry_is_a_miss_not_an_error() {
        // Teeth: if load propagated the parse error instead of degrading
        // to a miss, this test fails with that error.
        let (temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        cache.store("Python", source, &sample_value());
        let dir = cache.dir.clone().unwrap();
        let path = entry_path(&dir, "Python", source);
        fs::write(&path, b"{ this is not json").unwrap();

        assert_eq!(cache.load("Python", source), None);
        // And the corrupt entry heals on the next store.
        cache.store("Python", source, &sample_value());
        assert_eq!(cache.load("Python", source), Some(sample_value()));
        drop(temp);
    }

    #[test]
    fn stale_version_is_a_miss() {
        // An entry stamped by another sephera version must not be trusted.
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        let mut value = sample_value();
        value.sephera = "0.0.0-test".to_owned();
        // Bypass store's re-stamping by writing the entry directly.
        let dir = cache.dir.clone().unwrap();
        let path = entry_path(&dir, "Python", source);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(cache.load("Python", source), None);
    }

    #[test]
    fn no_cache_env_disables_everything() {
        let (_temp, _guard) = temp_cache();
        // SAFETY: the ENV_LOCK is held.
        unsafe {
            std::env::set_var("SEPHERA_NO_CACHE", "1");
        }
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));
        assert!(cache.dir.is_none());
        // Loads miss and stores are dropped, without touching the disk.
        assert_eq!(cache.load("Python", b"x"), None);
        cache.store("Python", b"x", &sample_value());
        // The guard's drop restores both variables; nothing leaks.
    }

    #[test]
    fn unwritable_cache_dir_degrades_to_disabled() {
        // Teeth: if any cache op unwrapped I/O, this panics instead of
        // degrading. A file where the directory should be makes every
        // create_dir_all fail.
        let _guard = EnvGuard::lock();
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("not-a-dir");
        fs::write(&file, b"nope").unwrap();
        // SAFETY: the ENV_LOCK is held for the whole test.
        unsafe {
            std::env::set_var("SEPHERA_CACHE_DIR", &file);
            std::env::remove_var("SEPHERA_NO_CACHE");
        }

        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));
        assert!(cache.dir.is_none());
        assert_eq!(cache.load("Python", b"x"), None);
        cache.store("Python", b"x", &sample_value());
        // The guard's drop restores both variables; nothing leaks.
    }

    #[test]
    fn fabricated_wellformed_entry_is_trusted() {
        // This pins the documented trust boundary rather than the fix:
        // a well-formed entry IS the answer. If the hit path were
        // bypassed (the bug this guards against), the test would see
        // the sample value's import instead of the fabricated one.
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        let mut fabricated = sample_value();
        fabricated.imports[0].raw_path = "fabricated".to_owned();
        cache.store("Python", source, &fabricated);

        let loaded = cache.load("Python", source).unwrap();
        assert_eq!(loaded.imports[0].raw_path, "fabricated");
    }

    /// The correctness proof that matters: a real graph build served from
    /// a warm cache, a cold cache, and the cache disabled entirely must
    /// produce byte-identical serialized reports. Teeth: if the hit path
    /// reattached anything wrongly (path, language, declarations), the
    /// warm report would diverge from the other two.
    #[test]
    fn cache_hit_and_cache_miss_build_identical_reports() {
        let _guard = EnvGuard::lock();
        let repo = tempfile::tempdir().unwrap();
        let cache_root = tempfile::tempdir().unwrap();
        fs::write(repo.path().join("a.py"), "import os\nfrom b import thing\n")
            .unwrap();
        fs::write(repo.path().join("b.py"), "import sys\nthing = 1\n").unwrap();

        let build = |warm_cache: bool| -> serde_json::Value {
            // SAFETY: the ENV_LOCK is held for the whole test.
            unsafe {
                if warm_cache {
                    std::env::set_var("SEPHERA_CACHE_DIR", cache_root.path());
                    std::env::remove_var("SEPHERA_NO_CACHE");
                } else {
                    std::env::set_var("SEPHERA_NO_CACHE", "1");
                }
            }
            let (report, _declarations) =
                crate::resolver::build_graph_with_declarations(
                    repo.path(),
                    &sephera_ignore::IgnoreMatcher::empty(),
                )
                .expect("graph build must succeed");
            // SAFETY: the ENV_LOCK is held for the whole test.
            unsafe {
                std::env::remove_var("SEPHERA_NO_CACHE");
            }
            serde_json::to_value(&report).expect("report serializes")
        };

        let cold = build(true);
        let warm = build(true);
        let bypassed = build(false);
        assert_eq!(cold, warm, "warm-cache report diverged from cold");
        assert_eq!(
            cold, bypassed,
            "cached report diverged from cache-disabled report"
        );
        // The guard's drop restores both variables; nothing leaks.
    }

    /// Teeth for the whole wiring: a well-formed fabricated entry planted
    /// for a real file's content must be served by the graph build. If the
    /// hit path in `extract_one_file` were bypassed (or the key computed
    /// differently at store and load), the report would show the real
    /// import instead of the fabricated one.
    #[test]
    fn hit_path_serves_planted_entry_in_graph_build() {
        let _guard = EnvGuard::lock();
        let repo = tempfile::tempdir().unwrap();
        let cache_root = tempfile::tempdir().unwrap();
        let source = b"import real_module\n";
        fs::write(repo.path().join("a.py"), source).unwrap();

        // SAFETY: the ENV_LOCK is held for the whole test.
        unsafe {
            std::env::set_var("SEPHERA_CACHE_DIR", cache_root.path());
            std::env::remove_var("SEPHERA_NO_CACHE");
        }
        let cache = ExtractionCache::for_base(repo.path());
        assert!(cache.dir.is_some());
        let mut lie = sample_value();
        lie.imports[0].raw_path = "fabricated_module".to_owned();
        cache.store("Python", source, &lie);

        let (report, _declarations) =
            crate::resolver::build_graph_with_declarations(
                repo.path(),
                &sephera_ignore::IgnoreMatcher::empty(),
            )
            .expect("graph build must succeed");
        let text = serde_json::to_string(&report).expect("report serializes");
        assert!(
            text.contains("fabricated_module"),
            "planted cache entry was not served"
        );
        assert!(
            !text.contains("real_module"),
            "stale parse leaked through the cache"
        );
        // The guard's drop restores both variables; nothing leaks.
    }

    #[test]
    fn stale_extractor_revision_is_a_miss() {
        // Teeth: without the revision in is_current, a grammar or
        // extractor change that keeps the sephera version would keep
        // serving stale entries.
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        cache.store("Python", source, &sample_value());
        let dir = cache.dir.clone().unwrap();
        let path = entry_path(&dir, "Python", source);

        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["extractor"] = serde_json::json!(u16::MAX);
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        assert_eq!(cache.load("Python", source), None);
    }

    #[test]
    fn previous_real_revision_is_a_miss() {
        // Teeth for the bump discipline: an entry stamped with the
        // immediately previous revision must miss, not just a garbage
        // one. Guards against `is_current` ever degrading to `<=`.
        let (_temp, _guard) = temp_cache();
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));

        let source = b"import os\n";
        cache.store("Python", source, &sample_value());
        let dir = cache.dir.clone().unwrap();
        let path = entry_path(&dir, "Python", source);

        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["extractor"] = serde_json::json!(EXTRACTOR_REVISION - 1);
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        assert_eq!(cache.load("Python", source), None);
    }

    #[test]
    fn drop_without_stores_writes_no_meta() {
        // Teeth: Drop used to rewrite meta.json unconditionally, so a
        // read-only build clobbered a concurrent build's newer total.
        let (_temp, _guard) = temp_cache();
        let dir = {
            let cache =
                ExtractionCache::for_base(Path::new("/nonexistent-base"));
            assert!(cache.dir.is_some());
            cache.dir.clone().unwrap()
        }; // dropped without storing anything
        assert!(
            !dir.join("meta.json").exists(),
            "drop wrote meta.json with nothing stored"
        );
    }

    #[test]
    fn evict_below_recomputes_the_true_total() {
        // Teeth: evict_oldest used to return the caller's estimate without
        // summing the walk, so a stale-low meta.json let the store sit
        // over the cap.
        let temp = tempfile::tempdir().unwrap();
        let shard = temp.path().join("ab");
        fs::create_dir_all(&shard).unwrap();
        fs::write(shard.join("k1.json"), vec![0u8; 10]).unwrap();
        fs::write(shard.join("k2.json"), vec![0u8; 20]).unwrap();
        fs::write(shard.join("k3.json"), vec![0u8; 30]).unwrap();

        // Under the cap: nothing evicted, and the returned total is the
        // walked sum, not an estimate.
        assert_eq!(evict_below(temp.path(), u64::MAX, u64::MAX), 60);
        assert_eq!(shard.read_dir().unwrap().count(), 3);
    }

    #[test]
    fn evict_below_evicts_oldest_to_low_water_mark() {
        // Three 100-byte entries, cap 250, target 150: 300 > 250, so
        // eviction removes entries until at most 150 remain — two files
        // gone, one 100-byte file left, true total returned. The count
        // and total hold regardless of mtime granularity.
        let temp = tempfile::tempdir().unwrap();
        let shard = temp.path().join("ab");
        fs::create_dir_all(&shard).unwrap();
        for name in ["k1.json", "k2.json", "k3.json"] {
            fs::write(shard.join(name), vec![0u8; 100]).unwrap();
        }

        assert_eq!(evict_below(temp.path(), 250, 150), 100);
        let remaining: Vec<_> = shard.read_dir().unwrap().collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].as_ref().unwrap().metadata().unwrap().len(),
            100
        );
    }

    #[test]
    fn eviction_target_sits_below_the_cap() {
        // Compile-time pin of the production wiring invariant: evict_oldest
        // passes EVICT_TARGET_BYTES as the target, so a full store is
        // brought down to 75% of the cap instead of stopping exactly at
        // it — otherwise every miss on a full store would pay for another
        // full walk. const, because clippy::assertions_on_constants
        // rightly notes these never change at runtime.
        const _: () = {
            assert!(EVICT_TARGET_BYTES < MAX_CACHE_BYTES);
            assert!(EVICT_TARGET_BYTES == MAX_CACHE_BYTES / 4 * 3);
        };
    }

    #[test]
    fn env_guard_restores_captured_values() {
        // Teeth: the old lock guard never restored, so temp_cache leaked
        // SEPHERA_CACHE_DIR process-wide and a later test could see a
        // cache dir pointing at a deleted tempdir.
        //
        // The pre-value is read under the lock: another test may be
        // mid-mutation when this test starts, and every guard restores on
        // drop, so a quiescent read under the lock is the stable baseline.
        let prev = {
            let _guard = EnvGuard::lock();
            std::env::var_os("SEPHERA_CACHE_DIR")
        };
        {
            let guard = EnvGuard::lock();
            // SAFETY: the ENV_LOCK is held.
            unsafe {
                std::env::set_var(
                    "SEPHERA_CACHE_DIR",
                    "/sentinel-must-not-leak",
                );
            }
            assert_eq!(
                std::env::var_os("SEPHERA_CACHE_DIR").as_deref(),
                Some(OsStr::new("/sentinel-must-not-leak"))
            );
            drop(guard);
        }
        // Re-lock for the final read: without the lock, another test's
        // temp_cache could set the variable between the restore and the
        // assertion.
        let _guard = EnvGuard::lock();
        assert_eq!(std::env::var_os("SEPHERA_CACHE_DIR"), prev);
    }
}
