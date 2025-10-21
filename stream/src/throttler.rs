use std::sync::atomic::{AtomicUsize, Ordering};

use sync_utils::waitgroup::{WaitGroup, WaitGroupGuard};

pub struct Throttler {
    wg: WaitGroup,
    thresholds: AtomicUsize,
}

impl Throttler {
    pub fn new(thresholds: usize) -> Self {
        Throttler { wg: WaitGroup::new(), thresholds: AtomicUsize::new(thresholds) }
    }

    #[inline(always)]
    pub fn nearly_full(&self) -> bool {
        self.wg.left() + 1 > self.thresholds.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.wg.left() >= self.thresholds.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub async fn throttle(&self) -> bool {
        let target = self.thresholds.load(Ordering::Relaxed);
        if target > 0 {
            return self.wg.wait_to(target).await;
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn add_task(&self) -> WaitGroupGuard {
        self.wg.add_guard()
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn set_thresholds(&mut self, thresholds: usize) {
        self.thresholds.store(thresholds, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn get_inflight_count(&self) -> usize {
        self.wg.left()
    }
}
