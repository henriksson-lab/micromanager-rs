//! Bus abstraction: carries an encoded host-command frame to the camera and back,
//! and streams frame data. PCIe and USB implement this identically above the
//! `pd_cam_write_read` boundary (RE notes §4bis / §4quinquies).

use crate::error::MmResult;

/// A frame-ready notification from the device (mirrors `pvcam_notif`, RE §3).
#[derive(Clone, Copy, Debug, Default)]
pub struct Notification {
    pub bof_count: u32,
    pub eof_count: u32,
    pub timestamp: u64,
}

/// Transport for one camera, agnostic to PCIe vs USB.
pub trait PvcamBus: Send {
    /// Send an encoded host-command frame and read the response back into `resp`
    /// (the device overwrites the shared request/response buffer). Returns the
    /// number of response bytes available.
    fn write_read(&mut self, frame: &[u8], resp: &mut [u8]) -> MmResult<usize>;

    /// Arm acquisition: hand the carrier a host buffer to receive `frame_bytes`
    /// per frame, `total_bytes` overall, `circular` for live mode.
    fn arm_acquisition(
        &mut self,
        frame_bytes: u32,
        total_bytes: u64,
        circular: bool,
    ) -> MmResult<()>;

    /// Non-blocking poll for the next frame notification.
    fn poll_notification(&mut self) -> MmResult<Option<Notification>>;

    /// Copy the most recent completed frame into `dst`.
    fn read_frame(&mut self, dst: &mut [u8]) -> MmResult<()>;

    /// Stop/abort an active acquisition.
    fn stop_acquisition(&mut self) -> MmResult<()>;
}

/// In-memory bus for unit tests: scripts `write_read` responses by command code and
/// records sent frames. Lets the camera logic be tested with no hardware.
#[cfg(test)]
pub struct MockBus {
    /// (code, response-bytes) keyed by the host-command code at frame[4].
    pub responses: std::collections::HashMap<u8, Vec<u8>>,
    pub sent: Vec<Vec<u8>>,
    pub armed: Option<(u32, u64, bool)>,
}

#[cfg(test)]
impl MockBus {
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
            sent: Vec::new(),
            armed: None,
        }
    }
    pub fn on(mut self, code: u8, resp: &[u8]) -> Self {
        self.responses.insert(code, resp.to_vec());
        self
    }
}

#[cfg(test)]
impl PvcamBus for MockBus {
    fn write_read(&mut self, frame: &[u8], resp: &mut [u8]) -> MmResult<usize> {
        self.sent.push(frame.to_vec());
        // frame[4] is the host-command code (after class+len16+ctrl-begin).
        let code = frame.get(4).copied().unwrap_or(0);
        if let Some(r) = self.responses.get(&code) {
            let n = r.len().min(resp.len());
            resp[..n].copy_from_slice(&r[..n]);
            Ok(n)
        } else {
            Ok(0)
        }
    }
    fn arm_acquisition(&mut self, fb: u32, tb: u64, c: bool) -> MmResult<()> {
        self.armed = Some((fb, tb, c));
        Ok(())
    }
    fn poll_notification(&mut self) -> MmResult<Option<Notification>> {
        // Simulate an immediately-ready frame so snap/sequence flows complete fast.
        Ok(Some(Notification { eof_count: 1, ..Default::default() }))
    }
    fn read_frame(&mut self, dst: &mut [u8]) -> MmResult<()> {
        // Fill with a recognizable pattern.
        for (i, b) in dst.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        Ok(())
    }
    fn stop_acquisition(&mut self) -> MmResult<()> {
        self.armed = None;
        Ok(())
    }
}

#[cfg(test)]
impl MockBus {
    /// Return the host-command code of each sent frame (frame[4]).
    pub fn sent_codes(&self) -> Vec<u8> {
        self.sent.iter().filter_map(|f| f.get(4).copied()).collect()
    }
}
