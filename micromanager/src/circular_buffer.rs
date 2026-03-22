use std::collections::VecDeque;
use parking_lot::Mutex;
use std::collections::HashMap;

/// Metadata attached to each frame in the circular buffer.
#[derive(Debug, Clone, Default)]
pub struct ImageMetadata {
    pub tags: HashMap<String, String>,
}

impl ImageMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.tags.insert(key.into(), value.into());
    }
}

/// A single captured image frame.
#[derive(Debug, Clone)]
pub struct ImageFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bytes_per_pixel: u32,
    pub metadata: ImageMetadata,
}

impl ImageFrame {
    pub fn new(data: Vec<u8>, width: u32, height: u32, bytes_per_pixel: u32) -> Self {
        Self {
            data,
            width,
            height,
            bytes_per_pixel,
            metadata: ImageMetadata::new(),
        }
    }
}

/// Fixed-capacity ring buffer of `ImageFrame`s.
/// Thread-safe via internal `Mutex`.
pub struct CircularBuffer {
    inner: Mutex<CircularBufferInner>,
}

struct CircularBufferInner {
    buf: VecDeque<ImageFrame>,
    capacity: usize,
    overflow_count: u64,
}

impl CircularBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(CircularBufferInner {
                buf: VecDeque::with_capacity(capacity),
                capacity,
                overflow_count: 0,
            }),
        }
    }

    /// Push a frame; drops oldest frame if at capacity.
    pub fn push(&self, frame: ImageFrame) {
        let mut g = self.inner.lock();
        if g.buf.len() == g.capacity {
            g.buf.pop_front();
            g.overflow_count += 1;
        }
        g.buf.push_back(frame);
    }

    /// Pop the oldest frame, or None if empty.
    pub fn pop(&self) -> Option<ImageFrame> {
        self.inner.lock().buf.pop_front()
    }

    /// Peek at the oldest frame without consuming it.
    pub fn peek(&self) -> Option<ImageFrame> {
        self.inner.lock().buf.front().cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock();
        g.buf.clear();
        g.overflow_count = 0;
    }

    pub fn overflow_count(&self) -> u64 {
        self.inner.lock().overflow_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop() {
        let buf = CircularBuffer::new(3);
        let frame = ImageFrame::new(vec![0u8; 4], 2, 2, 1);
        buf.push(frame.clone());
        assert_eq!(buf.len(), 1);
        let out = buf.pop().unwrap();
        assert_eq!(out.data, frame.data);
        assert!(buf.is_empty());
    }

    #[test]
    fn overflow() {
        let buf = CircularBuffer::new(2);
        for i in 0..3u8 {
            buf.push(ImageFrame::new(vec![i], 1, 1, 1));
        }
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.overflow_count(), 1);
        // Oldest pushed is gone; first remaining is frame with data [1]
        assert_eq!(buf.pop().unwrap().data, vec![1u8]);
    }
}
