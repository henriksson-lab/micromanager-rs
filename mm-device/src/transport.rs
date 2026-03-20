/// Abstraction over a serial (or otherwise byte-stream) transport.
/// Adapters hold a `Box<dyn Transport>`, which can be swapped for a mock in tests.
use crate::error::{MmError, MmResult};
use std::collections::VecDeque;

pub trait Transport: Send {
    /// Send a command string to the device.
    fn send(&mut self, cmd: &str) -> MmResult<()>;
    /// Read one response line (blocking until terminator or timeout).
    fn receive_line(&mut self) -> MmResult<String>;
    /// Discard any buffered input.
    fn purge(&mut self) -> MmResult<()>;

    /// Convenience: send command and read one response line.
    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.send(cmd)?;
        self.receive_line()
    }

    /// Send raw bytes (for binary protocols).
    fn send_bytes(&mut self, bytes: &[u8]) -> MmResult<()> {
        let s: String = bytes.iter().map(|&b| b as char).collect();
        self.send(&s)
    }

    /// Receive exactly `n` raw bytes.
    fn receive_bytes(&mut self, n: usize) -> MmResult<Vec<u8>> {
        let line = self.receive_line()?;
        let bytes: Vec<u8> = line.bytes().collect();
        Ok(bytes[..bytes.len().min(n)].to_vec())
    }
}

// ─── Mock transport for unit tests ───────────────────────────────────────────

/// A scripted mock transport.  Load expected (command → response) pairs; the
/// mock asserts commands arrive in order and returns the scripted responses.
pub struct MockTransport {
    /// Queue of (expected_command, scripted_response).
    /// Use `None` for expected_command to accept any command.
    script: VecDeque<(Option<String>, String)>,
    /// Queue of raw byte responses for binary protocols.
    bytes_script: VecDeque<Vec<u8>>,
    /// Log of commands actually received (for assertions in tests).
    pub received: Vec<String>,
    /// Log of raw bytes sent (for assertions in binary-protocol tests).
    pub received_bytes: Vec<Vec<u8>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            script: VecDeque::new(),
            bytes_script: VecDeque::new(),
            received: Vec::new(),
            received_bytes: Vec::new(),
        }
    }

    /// Add an expected command (exact match) and its scripted response.
    pub fn expect(mut self, cmd: &str, response: &str) -> Self {
        self.script.push_back((Some(cmd.to_string()), response.to_string()));
        self
    }

    /// Add a wildcard entry that accepts any command and returns the response.
    pub fn any(mut self, response: &str) -> Self {
        self.script.push_back((None, response.to_string()));
        self
    }

    /// Add multiple consecutive wildcard responses.
    pub fn drain_any(mut self, responses: impl IntoIterator<Item = &'static str>) -> Self {
        for r in responses {
            self.script.push_back((None, r.to_string()));
        }
        self
    }

    /// Add a scripted binary byte response (for binary-protocol adapters).
    pub fn expect_binary(mut self, response: &[u8]) -> Self {
        self.bytes_script.push_back(response.to_vec());
        self
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockTransport {
    fn send(&mut self, cmd: &str) -> MmResult<()> {
        self.received.push(cmd.to_string());
        Ok(())
    }

    fn receive_line(&mut self) -> MmResult<String> {
        match self.script.pop_front() {
            Some((expected, response)) => {
                if let Some(exp) = expected {
                    let sent = self.received.last().cloned().unwrap_or_default();
                    if sent != exp {
                        return Err(MmError::LocallyDefined(format!(
                            "MockTransport: expected command {:?}, got {:?}",
                            exp, sent
                        )));
                    }
                }
                Ok(response)
            }
            None => Err(MmError::SerialTimeout),
        }
    }

    fn purge(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn send_bytes(&mut self, bytes: &[u8]) -> MmResult<()> {
        self.received_bytes.push(bytes.to_vec());
        Ok(())
    }

    fn receive_bytes(&mut self, n: usize) -> MmResult<Vec<u8>> {
        match self.bytes_script.pop_front() {
            Some(data) => Ok(data[..data.len().min(n)].to_vec()),
            None => Err(MmError::SerialTimeout),
        }
    }
}
