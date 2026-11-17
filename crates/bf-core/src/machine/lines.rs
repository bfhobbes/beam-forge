/// Splits controller output into lines. GRBL ends lines with `\r\n`; a line
/// is complete at `\n` and a trailing `\r` is dropped.
#[derive(Debug, Default)]
pub struct LineReader {
    partial: Vec<u8>,
}

impl LineReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds received bytes and returns the lines they complete.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if self.partial.last() == Some(&b'\r') {
                    self.partial.pop();
                }
                lines.push(String::from_utf8_lossy(&self.partial).into_owned());
                self.partial.clear();
            } else {
                self.partial.push(byte);
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_lines_split_across_reads() {
        let mut reader = LineReader::new();
        assert_eq!(reader.push(b"\r\nGrbl 1."), [""]);
        assert_eq!(reader.push(b"1h ['$' for help]\r"), Vec::<String>::new());
        assert_eq!(reader.push(b"\nok\r\n"), ["Grbl 1.1h ['$' for help]", "ok"]);
    }
}
