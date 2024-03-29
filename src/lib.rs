use std::cell::Cell;
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread_local;

thread_local! {
    pub(crate) static CURRENT_ACQUIRED: Cell<bool> = const{ Cell::new(false) };
}

#[derive(Debug)]
pub struct SpinLock<T> {
    state: AtomicBool,
    data: UnsafeCell<T>,
}
impl<T> SpinLock<T> {
    pub fn new(val: T) -> Self {
        Self {
            state: AtomicBool::new(false),
            data: UnsafeCell::new(val),
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
        CURRENT_ACQUIRED.set(true);
        SpinLockGuard {
            data_mut_borrow: unsafe { &mut *self.data.get() },
            state: self,
        }
    }

    pub fn try_lock(&self) -> std::io::Result<SpinLockGuard<'_, T>> {
        if CURRENT_ACQUIRED.get() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Re-acquire the lock that has been acquired would cause a deadlock",
            ));
        }
        Ok(self.lock())
    }

    pub(self) fn unlock(&self) {
        CURRENT_ACQUIRED.set(false);
        self.state.store(false, Ordering::Release);
    }
}

unsafe impl<T> Send for SpinLock<T> {}
unsafe impl<T> Sync for SpinLock<T> {}

#[derive(Debug)]
pub struct SpinLockGuard<'a, T: 'a> {
    data_mut_borrow: &'a mut T,
    state: &'a SpinLock<T>,
}
impl<'a, T: 'a> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.state.unlock()
    }
}
impl<'a, T: 'a> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data_mut_borrow
    }
}
impl<'a, T: 'a> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data_mut_borrow
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::SpinLock;

    #[test]
    fn synchronization() {
        let mut result = 1;
        for i in 1..=20000 {
            let s_lock = Arc::new(SpinLock::new(result));
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
        let mut result = 1;
        let b = Box::new(result);
        let mut_ptr = Box::into_raw(b);
        for i in 1..=20000 {
            let s_lock = Arc::new(SpinLock::new(mut_ptr));
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

    #[test]
    fn avoid_deadlock() {
        let spin = Arc::new(SpinLock::new(0));
        let spin2 = spin.clone();
        let t1 = std::thread::spawn(move || {
            let _guard = spin.lock();
            let r = spin.try_lock();
            println!("{:?}", r);
            assert!(r.is_err());
            match r {
                Ok(_) => unreachable!(),
                Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock),
            };
        });
        let t = std::thread::spawn(move || {
            assert!(spin2.try_lock().is_ok());
        });
        t.join().unwrap();
        t1.join().unwrap();
    }
}
