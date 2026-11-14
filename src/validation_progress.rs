//! Read-only progress for full consensus replay. Never a validation receipt.
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Entry {
    data_dir: String,
    checked_height: u32,
    target_height: u32,
    started: Instant,
    progress_at: Instant,
}

fn registry() -> &'static Mutex<HashMap<u64, Entry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug, Serialize)]
pub struct ReplayStatus {
    pub active_workers: usize,
    pub checked_height: u32,
    pub target_height: u32,
    pub elapsed_seconds: u64,
    pub progress_age_seconds: u64,
}

/// GUI snapshots retain the same Settings data_dir string used by replay.
/// This diagnostic lookup does no filesystem work and grants no chain trust.
pub fn snapshot(data_dir: &str) -> Option<ReplayStatus> {
    let entries = registry().lock().unwrap_or_else(|e| e.into_inner());
    let matching = entries.iter().filter(|(_, entry)| entry.data_dir == data_dir);
    let active_workers = matching.clone().count();
    let (_, oldest) = matching.min_by_key(|(id, entry)| (entry.started, **id))?;
    Some(ReplayStatus {
        active_workers,
        checked_height: oldest.checked_height,
        target_height: oldest.target_height,
        elapsed_seconds: oldest.started.elapsed().as_secs(),
        progress_age_seconds: oldest.progress_at.elapsed().as_secs(),
    })
}

pub(crate) struct ReplayGuard(u64);

impl ReplayGuard {
    pub(crate) fn begin(data_dir: &str, target_height: u32) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        registry().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Entry {
            data_dir: data_dir.to_string(), checked_height: 0, target_height,
            started: now, progress_at: now,
        });
        Self(id)
    }

    pub(crate) fn checked(&self, height: u32) {
        if let Some(entry) = registry().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.0) {
            entry.checked_height = height.min(entry.target_height);
            entry.progress_at = Instant::now();
        }
    }
}

impl Drop for ReplayGuard {
    fn drop(&mut self) {
        registry().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hf134_replay_progress_is_scoped_and_removed_on_error() {
        let dir = "hf134-progress-scoped-test";
        assert!(snapshot(dir).is_none());
        let result: Result<(), ()> = (|| {
            let replay = ReplayGuard::begin(dir, 200);
            replay.checked(100);
            let view = snapshot(dir).unwrap();
            assert_eq!((view.active_workers, view.checked_height, view.target_height), (1, 100, 200));
            assert!(snapshot("another-data-directory").is_none());
            Err(())
        })();
        assert!(result.is_err());
        assert!(snapshot(dir).is_none(), "an error must not leave a phantom replay");
    }

    #[test]
    fn hf134_replay_progress_counts_overlapping_workers_without_overwrite() {
        let dir = "hf134-progress-overlap-test";
        let first = ReplayGuard::begin(dir, 200);
        first.checked(100);
        let second = ReplayGuard::begin(dir, 300);
        second.checked(20);
        let view = snapshot(dir).unwrap();
        assert_eq!(view.active_workers, 2);
        assert_eq!((view.checked_height, view.target_height), (100, 200));
        drop(second);
        assert_eq!(snapshot(dir).unwrap().active_workers, 1);
        drop(first);
        assert!(snapshot(dir).is_none());
    }

    #[test]
    fn hf134_replay_progress_does_not_accept_invalid_persisted_utxos() {
        let (mut settings, mut chain) = crate::hf134_tests::fixture();
        settings.node.data_dir = "hf134-progress-invalid-utxos-test".to_string();
        chain.validate_all(&settings).unwrap();
        assert!(snapshot(&settings.node.data_dir).is_none());
        std::sync::Arc::make_mut(&mut chain.utxos).clear();
        assert!(chain.validate_all(&settings).is_err());
        assert!(snapshot(&settings.node.data_dir).is_none());
    }
}
