use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering the value if a previous holder panicked.
///
/// The state behind these mutexes is never left half-updated by a panic, and
/// permanently failing every later request would be worse than carrying on.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
