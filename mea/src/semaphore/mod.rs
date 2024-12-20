// Copyright 2024 tison <wander4096@gmail.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! An async counting semaphore for controlling access to a set of resources.
//!
//! A semaphore maintains a set of permits. Permits are used to synchronize access
//! to a pool of resources. Each [`acquire`] call blocks until a permit is available,
//! and then takes one permit. Each [`release`] call adds a new permit, potentially
//! releasing a blocked acquirer.
//!
//! Semaphores are often used to restrict the number of tasks that can access some
//! (physical or logical) resource. For example, here is a class that uses a
//! semaphore to control access to a pool of connections:
//!
//! # Examples
//!
//! ## Basic usage
//!
//! ```
//! # #[tokio::main]
//! # async fn main() {
//! use mea::semaphore::Semaphore;
//!
//! let semaphore = Semaphore::new(3);
//! let a_permit = semaphore.acquire(1).await;
//! let two_permits = semaphore.acquire(2).await;
//!
//! assert_eq!(semaphore.available_permits(), 0);
//!
//! let permit_attempt = semaphore.try_acquire(1);
//! assert!(permit_attempt.is_none());
//! # }
//! ```
//!
//! ## Limit the number of simultaneously opened files in your program
//!
//! Most operating systems have limits on the number of open file
//! handles. Even in systems without explicit limits, resource constraints
//! implicitly set an upper bound on the number of open files. If your
//! program attempts to open a large number of files and exceeds this
//! limit, it will result in an error.
//!
//! This example uses a Semaphore with 100 permits. By acquiring a permit from
//! the Semaphore before accessing a file, you ensure that your program opens
//! no more than 100 files at a time. When trying to open the 101st
//! file, the program will wait until a permit becomes available before
//! proceeding to open another file.
//!
//! ```
//! use std::fs::File;
//! use std::io::Result;
//! use std::io::Write;
//! use std::sync::LazyLock;
//!
//! use mea::semaphore::Semaphore;
//!
//! static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(100));
//!
//! async fn write_to_file(message: &[u8]) -> Result<()> {
//!     let _permit = PERMITS.acquire(1).await;
//!     let mut buffer = File::create("example.txt")?;
//!     buffer.write_all(message)?;
//!     Ok(()) // Permit goes out of scope here, and is available again for acquisition
//! }
//! ```
//!
//! [`acquire`]: Semaphore::acquire
//! [`release`]: Semaphore::release

use std::sync::Arc;

use crate::internal;

#[cfg(test)]
mod tests;

/// An async counting semaphore for controlling access to a set of resources.
///
/// See the [module level documentation](self) for more.
#[derive(Debug)]
pub struct Semaphore {
    s: internal::Semaphore,
}

impl Semaphore {
    /// Creates a new semaphore with the given number of permits.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(5); // Creates a semaphore with 5 permits
    /// ```
    pub fn new(permits: u32) -> Self {
        Self {
            s: internal::Semaphore::new(permits),
        }
    }

    /// Returns the current number of permits available.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(2);
    /// assert_eq!(sem.available_permits(), 2);
    ///
    /// let permit = sem.try_acquire(1).unwrap();
    /// assert_eq!(sem.available_permits(), 1);
    /// ```
    pub fn available_permits(&self) -> u32 {
        self.s.available_permits()
    }

    /// Reduces the semaphore's permits by a maximum of `n`.
    ///
    /// Returns the actual number of permits that were reduced. This may be less
    /// than `n` if there are insufficient permits available.
    ///
    /// This is useful when you want to permanently remove permits from the semaphore.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(5);
    /// assert_eq!(sem.forget(3), 3); // Removes 3 permits
    /// assert_eq!(sem.available_permits(), 2);
    ///
    /// // Trying to forget more permits than available
    /// assert_eq!(sem.forget(3), 2); // Only removes remaining 2 permits
    /// assert_eq!(sem.available_permits(), 0);
    /// ```
    pub fn forget(&self, n: u32) -> u32 {
        self.s.forget(n)
    }

    /// Adds `n` new permits to the semaphore.
    ///
    /// # Panics
    ///
    /// Panics if adding the permits would cause the total number of permits to overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(0);
    /// sem.release(2); // Adds 2 permits
    /// assert_eq!(sem.available_permits(), 2);
    /// ```
    pub fn release(&self, permits: u32) {
        self.s.release(permits);
    }

    /// Attempts to acquire `n` permits from the semaphore without blocking.
    ///
    /// If the permits are successfully acquired, a [`SemaphorePermit`] is returned.
    /// The permits will be automatically returned to the semaphore when the permit
    /// is dropped, unless [`forget`] is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(2);
    ///
    /// // First acquisition succeeds
    /// let permit1 = sem.try_acquire(1).unwrap();
    /// assert_eq!(sem.available_permits(), 1);
    ///
    /// // Second acquisition succeeds
    /// let permit2 = sem.try_acquire(1).unwrap();
    /// assert_eq!(sem.available_permits(), 0);
    ///
    /// // Third acquisition fails
    /// assert!(sem.try_acquire(1).is_none());
    /// ```
    ///
    /// [`forget`]: SemaphorePermit::forget
    pub fn try_acquire(&self, permits: u32) -> Option<SemaphorePermit<'_>> {
        self.s
            .try_acquire(permits)
            .then_some(SemaphorePermit { sem: self, permits })
    }

    /// Acquires `n` permits from the semaphore.
    ///
    /// If the permits are not immediately available, this method will wait until they become
    /// available. Returns a [`SemaphorePermit`] that will release the permits when dropped.
    ///
    /// # Cancel safety
    ///
    /// This method uses a queue to fairly distribute permits in the order they were requested.
    /// Cancelling a call to `acquire` makes you lose your place in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main]
    /// # async fn main() {
    /// use std::sync::Arc;
    ///
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Arc::new(Semaphore::new(2));
    /// let sem2 = sem.clone();
    ///
    /// let handle = tokio::spawn(async move {
    ///     let permit = sem2.acquire(1).await;
    ///     // Do some work with the permit.
    ///     // Permit is automatically released when dropped.
    /// });
    ///
    /// let permit = sem.acquire(1).await;
    /// // Do some work with the permit
    /// drop(permit); // Explicitly release the permit
    ///
    /// handle.await.unwrap();
    /// # }
    /// ```
    pub async fn acquire(&self, permits: u32) -> SemaphorePermit<'_> {
        self.s.acquire(permits).await;
        SemaphorePermit { sem: self, permits }
    }

    /// Attempts to acquire `n` permits from the semaphore without blocking.
    ///
    /// The semaphore must be wrapped in an [`Arc`] to call this method.
    ///
    /// If the permits are successfully acquired, a [`OwnedSemaphorePermit`] is returned.
    /// The permits will be automatically returned to the semaphore when the permit
    /// is dropped, unless [`forget`] is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Arc::new(Semaphore::new(2));
    ///
    /// let p1 = sem.clone().try_acquire_owned(1).unwrap();
    /// assert_eq!(sem.available_permits(), 1);
    ///
    /// let p2 = sem.clone().try_acquire_owned(1).unwrap();
    /// assert_eq!(sem.available_permits(), 0);
    ///
    /// let p3 = sem.try_acquire_owned(1);
    /// assert!(p3.is_none());
    /// ```
    ///
    /// [`forget`]: SemaphorePermit::forget
    pub fn try_acquire_owned(self: Arc<Self>, permits: u32) -> Option<OwnedSemaphorePermit> {
        self.s
            .try_acquire(permits)
            .then_some(OwnedSemaphorePermit { sem: self, permits })
    }

    /// Acquires `n` permits from the semaphore.
    ///
    /// The semaphore must be wrapped in an [`Arc`] to call this method.
    ///
    /// If the permits are not immediately available, this method will wait until they become
    /// available. Returns a [`OwnedSemaphorePermit`] that will release the permits when dropped.
    ///
    /// # Cancel safety
    ///
    /// This method uses a queue to fairly distribute permits in the order they were requested.
    /// Cancelling a call to `acquire_owned` makes you lose your place in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main]
    /// # async fn main() {
    /// use std::sync::Arc;
    ///
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Arc::new(Semaphore::new(3));
    /// let mut join_handles = Vec::new();
    ///
    /// for _ in 0..5 {
    ///     let permit = sem.clone().acquire_owned(1).await;
    ///     join_handles.push(tokio::spawn(async move {
    ///         // perform task...
    ///         // explicitly own `permit` in the task
    ///         drop(permit);
    ///     }));
    /// }
    ///
    /// for handle in join_handles {
    ///     handle.await.unwrap();
    /// }
    /// # }
    /// ```
    pub async fn acquire_owned(self: Arc<Self>, permits: u32) -> OwnedSemaphorePermit {
        self.s.acquire(permits).await;
        OwnedSemaphorePermit { sem: self, permits }
    }
}

/// A permit from the semaphore.
///
/// This type is created by the [`acquire`] and [`try_acquire`] methods on [`Semaphore`].
/// When the permit is dropped, the permits will be returned to the semaphore unless
/// [`forget`] is called.
///
/// [`acquire`]: Semaphore::acquire
/// [`try_acquire`]: Semaphore::try_acquire
/// [`forget`]: SemaphorePermit::forget
#[must_use = "permits are released immediately when dropped"]
#[derive(Debug)]
pub struct SemaphorePermit<'a> {
    sem: &'a Semaphore,
    permits: u32,
}

impl SemaphorePermit<'_> {
    /// Forgets the permit **without** releasing it back to the semaphore.
    ///
    /// This can be used to permanently reduce the number of permits available
    /// from a semaphore.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Arc::new(Semaphore::new(10));
    /// {
    ///     let permit = sem.try_acquire(5).unwrap();
    ///     assert_eq!(sem.available_permits(), 5);
    ///     permit.forget();
    /// }
    ///
    /// // Since we forgot the permit, available permits won't go back to
    /// // its initial value even after the permit is dropped
    /// assert_eq!(sem.available_permits(), 5);
    /// ```
    pub fn forget(mut self) {
        self.permits = 0;
    }

    /// Returns the number of permits this permit holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(5);
    /// let permit = sem.try_acquire(3).unwrap();
    /// assert_eq!(permit.permits(), 3);
    /// ```
    pub fn permits(&self) -> u32 {
        self.permits
    }
}

impl Drop for SemaphorePermit<'_> {
    fn drop(&mut self) {
        self.sem.release(self.permits);
    }
}

/// An owned permit from the semaphore.
///
/// This type is created by the [`acquire_owned`] method.
///
/// [`acquire_owned`]: Semaphore::acquire_owned
#[must_use = "permits are released immediately when dropped"]
#[derive(Debug)]
pub struct OwnedSemaphorePermit {
    sem: Arc<Semaphore>,
    permits: u32,
}

impl OwnedSemaphorePermit {
    /// Forgets the permit **without** releasing it back to the semaphore.
    ///
    /// This can be used to permanently reduce the number of permits available
    /// from a semaphore.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Arc::new(Semaphore::new(10));
    /// {
    ///     let permit = sem.try_acquire(5).unwrap();
    ///     assert_eq!(sem.available_permits(), 5);
    ///     permit.forget();
    /// }
    ///
    /// // Since we forgot the permit, available permits won't go back to
    /// // its initial value even after the permit is dropped
    /// assert_eq!(sem.available_permits(), 5);
    /// ```
    pub fn forget(mut self) {
        self.permits = 0;
    }

    /// Returns the number of permits this permit holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use mea::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(5);
    /// let permit = sem.try_acquire(3).unwrap();
    /// assert_eq!(permit.permits(), 3);
    /// ```
    pub fn permits(&self) -> u32 {
        self.permits
    }
}

impl Drop for OwnedSemaphorePermit {
    fn drop(&mut self) {
        self.sem.release(self.permits);
    }
}