use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock<T> {
    state: AtomicBool,
    data: UnsafeCell<T>,
    is_acquired: AtomicBool,
}
impl<T> SpinLock<T> {
    pub fn new(val: T) -> Self {
        Self {
            state: AtomicBool::new(false),
            data: UnsafeCell::new(val),
            is_acquired: AtomicBool::new(false),
        }
    }
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        while self
            .state
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        self.is_acquired.store(true, Ordering::Relaxed);
        SpinLockGuard {
            data_mut_borrow: unsafe { &mut *self.data.get() },
            state: self,
        }
    }

    pub fn try_lock(&self) -> std::io::Result<SpinLockGuard<'_, T>> {
        if self.is_acquired.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Re-entry the lock that has been acquired can cause a deadlock",
            ));
        }
        Ok(self.lock())
    }

    pub(self) fn unlock(&self) {
        self.is_acquired.store(false, Ordering::Relaxed);
        self.state.store(false, Ordering::Release);
    }
}

unsafe impl<T> Send for SpinLock<T> {}
unsafe impl<T> Sync for SpinLock<T> {}

pub struct SpinLockGuard<'a, T> {
    data_mut_borrow: &'a mut T,
    state: &'a SpinLock<T>,
}
impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.state.unlock()
    }
}
impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data_mut_borrow
    }
}
impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data_mut_borrow
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn synchronization() {
        use std::sync::Arc;
        let mut result = 1;
        for i in 1..=20000 {
            let s_lock = Arc::new(crate::SpinLock::new(result));
            let s_lock_sub = s_lock.clone();
            let spin = s_lock.clone();
            let t = std::thread::spawn(move || {
                let mut guard = s_lock.lock();
                *guard += 2;
            });
            let t2 = std::thread::spawn(move || {
                let mut guard = s_lock_sub.lock();
                *guard += 3;
            });
            t.join().unwrap();
            t2.join().unwrap();
            result = *spin.lock();
            assert_eq!(result, (i * 5) + 1);
        }
    }
    #[test]
    fn sync_ptr() {
        use std::sync::Arc;
        let mut result = 1;
        let b = Box::new(result);
        let mut_ptr = Box::into_raw(b);
        for i in 1..=20000 {
            let s_lock = Arc::new(crate::SpinLock::new(mut_ptr));
            let s_lock_sub = s_lock.clone();
            let spin = s_lock.clone();
            let t = std::thread::spawn(move || {
                let guard = s_lock.lock();
                let ptr = *guard;
                unsafe {
                    *ptr += 2;
                };
            });
            let t2 = std::thread::spawn(move || {
                let guard = s_lock_sub.lock();
                let ptr = *guard;
                unsafe {
                    *ptr += 3;
                };
            });
            t.join().unwrap();
            t2.join().unwrap();
            result = unsafe { **spin.lock() };
            assert_eq!(result, (i * 5) + 1);
        }
        unsafe { drop(Box::from_raw(mut_ptr)) };
    }
}
