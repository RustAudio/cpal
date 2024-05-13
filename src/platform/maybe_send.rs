//! The following zero-sized type is for applying [`Send`]/[`Sync`]` restrictions to ensure
//! consistent behaviour across different platforms. The verbosely named type is used
//! (rather than using the markers directly) in the hope of making the compile errors
//! slightly more helpful.

// TODO: Remove this in favour of using negative trait bounds if they stabilise.

/// A marker used to remove the `Send` and `Sync` traits.
pub(crate) struct NotSendSyncAcrossAllPlatforms(std::marker::PhantomData<*mut ()>);

impl Default for NotSendSyncAcrossAllPlatforms {
    fn default() -> Self {
        NotSendSyncAcrossAllPlatforms(std::marker::PhantomData)
    }
}

// TODO: Implement Send on platforms which support it.

#[cfg(any(
    // Windows with WASAPI allows for a Send Stream
    all(windows, not(feature = "asio")),
))]
unsafe impl Send for NotSendSyncAcrossAllPlatforms {}
