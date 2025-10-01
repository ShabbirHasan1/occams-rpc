use io_buffer::Buffer;

pub trait AllocateBuf {
    /// Alloc buffer or reserve space inside the Buffer
    fn reserve<'a>(&'a mut self, _blob_len: i32) -> Option<&'a mut [u8]>;
}

impl AllocateBuf for Option<Vec<u8>> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        let blob_len = blob_len as usize;
        if let Some(buf) = self.as_mut() {
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    buf.reserve(blob_len - buf.capacity());
                }
                unsafe { buf.set_len(blob_len) };
            }
        } else {
            let mut v = Vec::with_capacity(blob_len);
            unsafe { v.set_len(blob_len) };
            self.replace(v);
        }
        return self.as_deref_mut();
    }
}

impl AllocateBuf for Option<Buffer> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        if let Some(buf) = self.as_mut() {
            let blob_len = blob_len as usize;
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    return None;
                }
                buf.set_len(blob_len);
            }
        } else {
            if let Ok(v) = Buffer::alloc(blob_len) {
                self.replace(v);
            } else {
                // alloc failed
                return None;
            }
        }
        return self.as_deref_mut();
    }
}
