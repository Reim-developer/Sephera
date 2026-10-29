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
//! * **Bounded.** The store is capped at 1 GiB; over the cap, oldest-created
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

/// The most the store may hold. Past this, the oldest-created entries are
/// evicted first.
const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Per-write temp-name counter. Two processes may store the same key
/// concurrently; sharing one temp name would let their bytes interleave.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One file's extraction result, as stored on disk.
///
/// Built by [`CachedExtraction::new`], which stamps the format version and
/// the sephera version: a sephera upgrade (new grammars, new tables)
/// silently invalidates every entry instead of misreading one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedExtraction {
    format: u8,
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
    /// silently dropped, and the entry is stamped with the current format
    /// and sephera versions on the way in.
    pub fn store(
        &self,
        language: &str,
        source: &[u8],
        value: &CachedExtraction,
    ) {
        let Some(dir) = self.dir.as_ref() else {
            return;
        };
        let mut value = value.clone();
        value.format = FORMAT_VERSION;
        env!("CARGO_PKG_VERSION").clone_into(&mut value.sephera);
        let Ok(bytes) = serde_json::to_vec(&value) else {
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
            let total = evict_oldest(
                dir,
                self.bytes_base
                    .load(Ordering::Relaxed)
                    .saturating_add(added),
            );
            self.bytes_base.store(total, Ordering::Relaxed);
            self.bytes_added.store(0, Ordering::Relaxed);
        }
    }
}

impl Drop for ExtractionCache {
    /// Flush the batched size accounting. Best-effort like everything
    /// else here: a killed process leaves a stale (low) total, and the
    /// next over-cap walk corrects the drift.
    fn drop(&mut self) {
        if let Some(dir) = self.dir.as_ref() {
            let total = self
                .bytes_base
                .load(Ordering::Relaxed)
                .saturating_add(self.bytes_added.load(Ordering::Relaxed));
            let meta = CacheMeta { bytes: total };
            if let Ok(bytes) = serde_json::to_vec(&meta) {
                let _ = fs::write(dir.join("meta.json"), bytes);
            }
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
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        let path = PathBuf::from(xdg);
        if path.is_absolute() {
            return Some(path.join("sephera"));
        }
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache").join("sephera"))
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

/// Delete oldest-created entries until the store is back under the cap.
/// Returns the true total afterwards; mtime orders eviction only.
fn evict_oldest(dir: &Path, mut total: u64) -> u64 {
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let Ok(shards) = fs::read_dir(dir) else {
        return total;
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
    entries.sort_by_key(|entry| entry.0);
    for (_, size, path) in entries {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The cache reads process-global env vars, and Rust runs tests on
    /// threads: hold this while mutating them so no two tests race.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn temp_cache() -> (tempfile::TempDir, MutexGuard<'static, ()>) {
        let guard = lock_env();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: the ENV_LOCK serializes every env mutation in this
        // test module, so no other thread observes a half-set pair.
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
        let (_temp, guard) = temp_cache();
        // SAFETY: the ENV_LOCK is held.
        unsafe {
            std::env::set_var("SEPHERA_NO_CACHE", "1");
        }
        let cache = ExtractionCache::for_base(Path::new("/nonexistent-base"));
        assert!(cache.dir.is_none());
        // Loads miss and stores are dropped, without touching the disk.
        assert_eq!(cache.load("Python", b"x"), None);
        cache.store("Python", b"x", &sample_value());
        drop(guard);
        let _guard = lock_env();
        // SAFETY: the ENV_LOCK is held.
        unsafe {
            std::env::remove_var("SEPHERA_NO_CACHE");
        }
    }

    #[test]
    fn unwritable_cache_dir_degrades_to_disabled() {
        // Teeth: if any cache op unwrapped I/O, this panics instead of
        // degrading. A file where the directory should be makes every
        // create_dir_all fail.
        let guard = lock_env();
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
        drop(guard);
        let _guard = lock_env();
        // SAFETY: the ENV_LOCK is held.
        unsafe {
            std::env::remove_var("SEPHERA_CACHE_DIR");
        }
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
        let guard = lock_env();
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
        drop(guard);
    }

    /// Teeth for the whole wiring: a well-formed fabricated entry planted
    /// for a real file's content must be served by the graph build. If the
    /// hit path in `extract_one_file` were bypassed (or the key computed
    /// differently at store and load), the report would show the real
    /// import instead of the fabricated one.
    #[test]
    fn hit_path_serves_planted_entry_in_graph_build() {
        let guard = lock_env();
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
        drop(guard);
    }
}
