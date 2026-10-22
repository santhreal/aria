use std::mem::ManuallyDrop;

#[cfg(doc)]
use super::ARIA_INSTANCE_OK;
use super::{
    ARIA_BYTES_ERR, ARIA_BYTES_FOUND, ARIA_BYTES_NOT_FOUND, ARIA_BYTES_PANIC,
    ARIA_INSTANCE_UNSUPPORTED, ARIA_STATUS_ERR, ARIA_STATUS_OK, ARIA_STATUS_PANIC,
    ARIA_STATUS_UNSUPPORTED,
};

/// Borrowed UTF-8 text, valid until the call it is passed to returns. It holds no
/// terminating NUL.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaStr {
    /// First byte of the text, or null when `len` is 0.
    pub ptr: *const u8,
    /// Length of the text in bytes.
    pub len: usize,
}

impl AriaStr {
    /// Borrow a Rust string slice as an [`AriaStr`].
    pub const fn from_str(text: &str) -> Self {
        Self {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// View the borrowed bytes as a UTF-8 string slice.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to `self.len` valid initialized bytes that remain
    /// alive for `'a` (or `self.len == 0`).
    #[allow(unsafe_code)]
    pub unsafe fn as_str<'a>(self) -> Result<&'a str, std::str::Utf8Error> {
        if self.len == 0 {
            return Ok("");
        }
        // SAFETY: The caller guarantees `ptr` is valid for `len` bytes for `'a`.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes)
    }
}

/// Borrowed bytes, valid until the call they are passed to returns.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytes {
    /// First byte, or null when `len` is 0.
    pub ptr: *const u8,
    /// Number of bytes.
    pub len: usize,
}

impl AriaBytes {
    /// Borrow a Rust byte slice as an [`AriaBytes`].
    pub const fn from_slice(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// View the borrowed bytes as a slice.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to `self.len` valid initialized bytes that remain
    /// alive for `'a` (or `self.len == 0`).
    #[allow(unsafe_code)]
    pub unsafe fn as_slice<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: The caller guarantees `ptr` is valid for `len` bytes for `'a`.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// UTF-8 text returned across the boundary. The receiver releases it with the
/// `free_string` slot of the table of the side that allocated it. A `free_string`
/// accepts a null `ptr` and does nothing for it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedStr {
    /// First byte of the text, or null for an empty string that allocates nothing.
    pub ptr: *mut u8,
    /// Length of the text in bytes.
    pub len: usize,
    /// Allocation size in bytes, for the allocating side's `free_string`. The receiver
    /// does not read it.
    pub cap: usize,
}

impl AriaOwnedStr {
    /// An empty owned string that allocates nothing.
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Transfer ownership of a Rust [`String`] into an [`AriaOwnedStr`].
    pub fn from_string(text: String) -> Self {
        if text.capacity() == 0 {
            return Self::empty();
        }
        let mut bytes = ManuallyDrop::new(text.into_bytes());
        Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }

    /// Whether the string has no pointer (`None` in option positions).
    pub fn is_null(&self) -> bool {
        self.ptr.is_null() && self.len == 0
    }

    /// Copy the string into a Rust [`String`] and release the buffer with `free`.
    ///
    /// # Safety
    ///
    /// `self` must have been produced by the side whose `free` function is passed.
    #[allow(unsafe_code)]
    pub unsafe fn into_string_with(self, free: unsafe extern "C" fn(AriaOwnedStr)) -> String {
        let out = if self.len == 0 || self.ptr.is_null() {
            String::new()
        } else {
            // SAFETY: `self.ptr` points to `self.len` valid bytes until `free` runs.
            let slice = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
            String::from_utf8_lossy(slice).into_owned()
        };
        // SAFETY: `self` was allocated by the side providing `free`.
        unsafe { free(self) };
        out
    }
}

/// Bytes returned across the boundary. The receiver releases them with the
/// `free_bytes` slot of the table of the side that allocated them. A `free_bytes`
/// accepts a null `ptr` and does nothing for it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedBytes {
    /// First byte, or null for an empty buffer that allocates nothing.
    pub ptr: *mut u8,
    /// Number of bytes.
    pub len: usize,
    /// Allocation size in bytes, for the allocating side's `free_bytes`. The receiver
    /// does not read it.
    pub cap: usize,
}

impl AriaOwnedBytes {
    /// An empty byte buffer that allocates nothing.
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Transfer ownership of a Rust [`Vec<u8>`] into an [`AriaOwnedBytes`].
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.capacity() == 0 {
            return Self::empty();
        }
        let mut bytes = ManuallyDrop::new(bytes);
        Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }

    /// Copy the buffer into a [`Vec<u8>`] and release it with `free`.
    ///
    /// # Safety
    ///
    /// `self` must have been produced by the side whose `free` function is passed.
    #[allow(unsafe_code)]
    pub unsafe fn into_vec_with(self, free: unsafe extern "C" fn(AriaOwnedBytes)) -> Vec<u8> {
        let out = if self.len == 0 || self.ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: `self.ptr` points to `self.len` valid bytes until `free` runs.
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }.to_vec()
        };
        // SAFETY: `self` was allocated by the side providing `free`.
        unsafe { free(self) };
        out
    }
}

/// Free an [`AriaOwnedStr`] allocated by Rust's [`AriaOwnedStr::from_string`].
///
/// # Safety
///
/// `text` must have been created by [`AriaOwnedStr::from_string`] in the same
/// dynamic object, or be [`AriaOwnedStr::empty`].
#[allow(unsafe_code)]
pub unsafe extern "C" fn free_rust_string(text: AriaOwnedStr) {
    if !text.ptr.is_null() && text.cap > 0 {
        // SAFETY: `ptr`, `len`, and `cap` came from `String::into_bytes` in this module.
        drop(unsafe { Vec::from_raw_parts(text.ptr, text.len, text.cap) });
    }
}

/// Free an [`AriaOwnedBytes`] allocated by Rust's [`AriaOwnedBytes::from_vec`].
///
/// # Safety
///
/// `bytes` must have been created by [`AriaOwnedBytes::from_vec`] in the same
/// dynamic object, or be [`AriaOwnedBytes::empty`].
#[allow(unsafe_code)]
pub unsafe extern "C" fn free_rust_bytes(bytes: AriaOwnedBytes) {
    if !bytes.ptr.is_null() && bytes.cap > 0 {
        // SAFETY: `ptr`, `len`, and `cap` came from `Vec<u8>` in this module.
        drop(unsafe { Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap) });
    }
}

/// Result of a call that returns text: a JSON document, a message, or nothing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaResult {
    /// An `ARIA_STATUS_*` value.
    pub status: u32,
    /// The result on [`ARIA_STATUS_OK`], empty when the call returns nothing, and the
    /// message on any other status.
    pub value: AriaOwnedStr,
}

impl AriaResult {
    /// Successful unit result with no allocated payload.
    pub const fn ok_empty() -> Self {
        Self {
            status: ARIA_STATUS_OK,
            value: AriaOwnedStr::empty(),
        }
    }

    /// Successful result carrying `value`.
    pub fn ok(value: String) -> Self {
        Self {
            status: ARIA_STATUS_OK,
            value: AriaOwnedStr::from_string(value),
        }
    }

    /// Normal error result carrying `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_ERR,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic result carrying `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_PANIC,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// A decoder call on a file the decoder does not read.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_UNSUPPORTED,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// Result of a call that returns bytes: `storage_get`, `panel_view` and
/// `overlay_scene`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytesResult {
    /// An `ARIA_BYTES_*` value.
    pub status: u32,
    /// The result on [`ARIA_BYTES_FOUND`], otherwise empty.
    pub bytes: AriaOwnedBytes,
    /// The message on [`ARIA_BYTES_ERR`] and [`ARIA_BYTES_PANIC`], otherwise empty.
    pub error: AriaOwnedStr,
}

impl AriaBytesResult {
    /// Success with `bytes`.
    pub fn found(bytes: Vec<u8>) -> Self {
        Self {
            status: ARIA_BYTES_FOUND,
            bytes: AriaOwnedBytes::from_vec(bytes),
            error: AriaOwnedStr::empty(),
        }
    }

    /// Key was not found (`Ok(None)`).
    pub const fn not_found() -> Self {
        Self {
            status: ARIA_BYTES_NOT_FOUND,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::empty(),
        }
    }

    /// Failure with `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_ERR,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic with `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_PANIC,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// Result of a call that creates an instance the host addresses in later calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaInstanceResult {
    /// An `ARIA_INSTANCE_*` value.
    pub status: u32,
    /// The new instance's id on [`ARIA_INSTANCE_OK`], otherwise 0. The side that creates
    /// the instance picks the id, and the other side passes it back unchanged.
    pub instance: u64,
    /// The message on any status but [`ARIA_INSTANCE_OK`], otherwise empty.
    pub error: AriaOwnedStr,
}

impl AriaInstanceResult {
    /// Success with the new instance id `instance`.
    pub const fn ok(instance: u64) -> Self {
        Self {
            status: ARIA_STATUS_OK,
            instance,
            error: AriaOwnedStr::empty(),
        }
    }

    /// Failure with `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_ERR,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic with `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_PANIC,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// A decoder open on a file the decoder does not read.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_INSTANCE_UNSUPPORTED,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }
}
