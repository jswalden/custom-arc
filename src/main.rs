use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::fence;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

struct ArkVault<T>
where
    T: Send + Sync,
{
    rc: AtomicUsize,
    value: T,
}

impl<T> ArkVault<T>
where
    T: Send + Sync,
{
    fn new(value: T) -> ArkVault<T> {
        ArkVault {
            rc: AtomicUsize::new(1),
            value,
        }
    }

    fn value_ref(&self) -> &T {
        &self.value
    }
}

pub struct Ark<T>
where
    T: Send + Sync,
{
    vault: NonNull<ArkVault<T>>,
}

unsafe impl<T> Send for Ark<T> where T: Send + Sync {}
unsafe impl<T> Sync for Ark<T> where T: Send + Sync {}

impl<T> Ark<T>
where
    T: Send + Sync,
{
    pub fn new(value: T) -> Ark<T> {
        let vault = Box::into_raw(Box::new(ArkVault::new(value)));

        Ark {
            vault: NonNull::new(vault).unwrap(),
        }
    }

    fn vault(&self) -> &ArkVault<T> {
        // Safety: ArkVault's value is presently always live.
        unsafe { self.vault.as_ref() }
    }

    pub fn get_mut(ark: &mut Self) -> Option<&mut T> {
        if ark.vault().rc.load(Ordering::Relaxed) == 1 {
            fence(Ordering::Acquire);

            // Safety: This Ark is immutably borrowed (preventing any other
            // borrow of it), and this Ark holds the only reference to the vault
            // and its contained value, so that borrow can be safely lent out as
            // mutable.
            let mut_ref = unsafe { &mut ark.vault.as_mut().value };

            Some(mut_ref)
        } else {
            None
        }
    }
}

impl<T> Clone for Ark<T>
where
    T: Send + Sync,
{
    fn clone(&self) -> Ark<T> {
        let vault = self.vault();
        vault.rc.fetch_add(1, Ordering::Relaxed);
        Ark {
            vault: self.vault.clone(),
        }
    }
}

impl<T> Drop for Ark<T>
where
    T: Send + Sync,
{
    fn drop(&mut self) {
        let old_rc = self.vault().rc.fetch_sub(1, Ordering::Release);
        if old_rc == 1 {
            fence(Ordering::Acquire);
            // Safety: Refcount just dropped to zero, so nothing refers to the
            // vault and its value, so it can be accessed and dropped.
            unsafe {
                drop(Box::from_raw(self.vault.as_ptr()));
            }
        }
    }
}

impl<T> Deref for Ark<T>
where
    T: Send + Sync,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.vault().value_ref()
    }
}

fn main() {
    let v = vec![];
    let v = Ark::new(Mutex::new(v));

    std::thread::scope(|scope| {
        let first = v.clone();

        scope.spawn(move || {
            first.lock().unwrap().push(42);
        });

        let second = v.clone();
        scope.spawn(move || {
            second.lock().unwrap().push(17);
        });
    });

    assert!(v.lock().unwrap().contains(&17));
    assert!(v.lock().unwrap().contains(&42));
}
