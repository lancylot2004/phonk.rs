use core::ops::Index;

/// A simple ring buffer implementation, specifically meant for batch mutations with a fixed size.
pub struct Ring<T, const N: usize> {
    buf: [T; N],
    head: usize,
    size: usize,
}

impl<T: Copy, const N: usize> Ring<T, N> {
    #[inline(always)]
    pub const fn new(initial_value: T) -> Self {
        Self {
            buf: [initial_value; N],
            head: 0,
            size: 0,
        }
    }

    /// Push a slice of samples efficiently.
    #[inline(always)]
    pub fn extend_from_slice(&mut self, data: &[T]) {
        let len = data.len();

        if len >= N {
            let tail = &data[len - N..];
            self.buf.copy_from_slice(tail);
            self.head = 0;
            self.size = N;
            return;
        }

        let start = self.head;
        let end = (start + len) % N;

        if start < end {
            self.buf[start..end].copy_from_slice(data);
        } else {
            let split = N - start;
            self.buf[start..].copy_from_slice(&data[..split]);
            self.buf[..end].copy_from_slice(&data[split..]);
        }

        self.head = end;
        self.size = N.min(self.size + len);
    }

    /// Returns the full logical window as two contiguous slices.
    #[inline(always)]
    pub fn as_slices(&self) -> (&[T], &[T]) {
        let (a, b) = self.buf.split_at(self.head);
        (b, a)
    }

    /// Checks if the buffer is full.
    pub fn is_full(&self) -> bool {
        self.size == N
    }
}

impl<T, const N: usize> Index<usize> for Ring<T, N> {
    type Output = T;

    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        let idx = self.head + index;
        let idx = if idx < N { idx } else { idx - N };

        unsafe { self.buf.get_unchecked(idx) }
    }
}
