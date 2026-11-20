use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;

use veena_net::{agent, ureq};

/// Forward seeks up to this distance read through the open connection instead
/// of starting a new request.
const SKIP_THRESHOLD: u64 = 256 * 1024;

/// A seekable reader over an HTTP resource.
///
/// Uses `Range` requests, so playback can start before the file is fully
/// downloaded and seeking only fetches what is needed.
pub struct HttpReader {
    url: String,
    headers: Vec<(String, String)>,
    pos: u64,
    len: Option<u64>,
    content_type: Option<String>,
    ranges: bool,
    /// The open response body. In a `Mutex` only so the reader is `Sync`,
    /// which the decoder requires. It is only ever accessed via `&mut self`.
    body: Mutex<Option<Box<dyn Read + Send>>>,
}

impl HttpReader {
    /// Open `url`, sending `headers` with every request.
    pub fn open(url: &str, headers: &[(String, String)]) -> io::Result<Self> {
        let mut reader = Self {
            url: url.to_owned(),
            headers: headers.to_vec(),
            pos: 0,
            len: None,
            content_type: None,
            ranges: false,
            body: Mutex::new(None),
        };
        reader.connect()?;
        Ok(reader)
    }

    /// Total size in bytes, if the server reported it.
    pub fn byte_len(&self) -> Option<u64> {
        self.len
    }

    /// Whether the server supports range requests, i.e. cheap seeking.
    pub fn is_seekable(&self) -> bool {
        self.ranges && self.len.is_some()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Start a request at the current position.
    fn connect(&mut self) -> io::Result<()> {
        let mut request = agent()
            .get(&self.url)
            .header("Range", format!("bytes={}-", self.pos));
        for (name, value) in &self.headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = match request.call() {
            Ok(response) => response,
            // Asking for a range that starts at the end: nothing left to read.
            Err(ureq::Error::StatusCode(416)) => {
                *self.body.get_mut().unwrap() = Some(Box::new(io::empty()));
                return Ok(());
            }
            Err(e) => return Err(io::Error::other(e)),
        };

        let headers = response.headers();
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let partial = response.status().as_u16() == 206;

        if self.content_type.is_none() {
            self.content_type =
                header("content-type").map(|v| v.split(';').next().unwrap_or(v).trim().to_owned());
        }
        if partial {
            self.ranges = true;
            // "bytes 0-999/12345": the total follows the slash.
            if let Some(total) = header("content-range")
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse().ok())
            {
                self.len = Some(total);
            }
        } else if let Some(length) = header("content-length").and_then(|v| v.parse::<u64>().ok()) {
            self.len = Some(length);
        }

        let mut body: Box<dyn Read + Send> = Box::new(response.into_body().into_reader());
        if !partial && self.pos > 0 {
            // The server ignored the range and sent the whole file, so skip
            // to where we need to be.
            io::copy(&mut (&mut body).take(self.pos), &mut io::sink())?;
        }
        *self.body.get_mut().unwrap() = Some(body);
        Ok(())
    }
}

impl Read for HttpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.len.is_some_and(|len| self.pos >= len) {
            return Ok(0);
        }
        if self.body.get_mut().unwrap().is_none() {
            self.connect()?;
        }
        let body = self.body.get_mut().unwrap().as_mut().expect("connected");
        let n = match body.read(buf) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
            Err(_) => {
                // A dropped connection mid-stream: reconnect once and resume.
                self.connect()?;
                self.body
                    .get_mut()
                    .unwrap()
                    .as_mut()
                    .expect("connected")
                    .read(buf)?
            }
        };
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => match self.len {
                Some(len) => len.checked_add_signed(delta),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "stream length is unknown",
                    ));
                }
            },
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;

        if target == self.pos {
            return Ok(target);
        }
        let body = self.body.get_mut().unwrap();
        match body {
            Some(open) if target > self.pos && target - self.pos <= SKIP_THRESHOLD => {
                let skip = target - self.pos;
                let skipped = io::copy(&mut open.take(skip), &mut io::sink())?;
                self.pos += skipped;
                if skipped < skip {
                    // Hit the end early: the next read reconnects.
                    *body = None;
                    self.pos = target;
                }
            }
            _ => {
                // Reconnect lazily at the new position on the next read.
                *body = None;
                self.pos = target;
            }
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    /// A tiny HTTP server serving `data`, with or without range support.
    fn serve(data: Vec<u8>, ranges: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut start = 0usize;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        start = v.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let head = if ranges {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n",
                        data.len() - 1,
                        data.len(),
                        data.len() - start
                    )
                } else {
                    start = 0;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len()
                    )
                };
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&data[start..]);
            }
        });
        format!("http://{addr}/file.wav")
    }

    fn data() -> Vec<u8> {
        (0..1_000_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn check_seeks(reader: &mut HttpReader, data: &[u8]) {
        let mut buf = [0u8; 16];
        for &pos in &[0u64, 10, 500_000, 400_000, 999_990, 3] {
            reader.seek(SeekFrom::Start(pos)).unwrap();
            let n = reader.read(&mut buf).unwrap();
            assert!(n > 0);
            assert_eq!(&buf[..n], &data[pos as usize..pos as usize + n], "at {pos}");
        }
        reader.seek(SeekFrom::End(0)).unwrap();
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn range_requests() {
        let data = data();
        let mut reader = HttpReader::open(&serve(data.clone(), true), &[]).unwrap();
        assert!(reader.is_seekable());
        assert_eq!(reader.byte_len(), Some(data.len() as u64));
        assert_eq!(reader.content_type(), Some("audio/wav"));
        check_seeks(&mut reader, &data);
    }

    #[test]
    fn server_without_ranges() {
        let data = data();
        let mut reader = HttpReader::open(&serve(data.clone(), false), &[]).unwrap();
        assert!(!reader.is_seekable());
        check_seeks(&mut reader, &data);
    }
}
