use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;

use veena_net::{agent, ureq};

/// Bytes requested per request. Bounded ranges are required by some
/// servers (YouTube refuses open-ended ones) and keep seeks cheap.
const CHUNK_SIZE: u64 = 4 * 1024 * 1024;

/// Forward seeks up to this distance read through the open connection instead
/// of starting a new request.
const SKIP_THRESHOLD: u64 = 256 * 1024;

/// A seekable reader over an HTTP resource.
///
/// Fetches the file in bounded `Range` requests, so playback can start
/// before the file is downloaded and seeking only fetches what is needed.
pub struct HttpReader {
    url: String,
    headers: Vec<(String, String)>,
    pos: u64,
    len: Option<u64>,
    content_type: Option<String>,
    ranges: bool,
    chunk_size: u64,
    /// End (exclusive) of the bytes the open response will deliver.
    chunk_end: u64,
    /// The open response body. In a `Mutex` only so the reader is `Sync`,
    /// which the decoder requires. It is only ever accessed via `&mut self`.
    body: Mutex<Option<Box<dyn Read + Send>>>,
}

impl HttpReader {
    /// Open `url`, sending `headers` with every request.
    pub fn open(url: &str, headers: &[(String, String)]) -> io::Result<Self> {
        Self::open_with_chunk_size(url, headers, CHUNK_SIZE)
    }

    pub(crate) fn open_with_chunk_size(
        url: &str,
        headers: &[(String, String)],
        chunk_size: u64,
    ) -> io::Result<Self> {
        let mut reader = Self {
            url: url.to_owned(),
            headers: headers.to_vec(),
            pos: 0,
            len: None,
            content_type: None,
            ranges: false,
            chunk_size: chunk_size.max(1),
            chunk_end: 0,
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

    /// Request the next chunk, starting at the current position.
    fn connect(&mut self) -> io::Result<()> {
        let mut last = self.pos + self.chunk_size - 1;
        if let Some(len) = self.len {
            last = last.min(len.saturating_sub(1));
        }
        let mut request = agent()
            .get(&self.url)
            .header("Range", format!("bytes={}-{last}", self.pos));
        for (name, value) in &self.headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = match request.call() {
            Ok(response) => response,
            // Asking for a range that starts at the end: nothing left to read.
            Err(ureq::Error::StatusCode(416)) => {
                *self.body.get_mut().unwrap() = Some(Box::new(io::empty()));
                self.chunk_end = u64::MAX;
                return Ok(());
            }
            Err(e) => return Err(io::Error::other(e)),
        };

        let headers = response.headers();
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let partial = response.status().as_u16() == 206;
        let content_length = header("content-length").and_then(|v| v.parse::<u64>().ok());

        if self.content_type.is_none() {
            self.content_type =
                header("content-type").map(|v| v.split(';').next().unwrap_or(v).trim().to_owned());
        }
        if partial {
            self.ranges = true;
            // "bytes 0-999/12345": the range, then the total after the slash.
            let content_range = header("content-range").unwrap_or_default();
            if let Some(total) = content_range
                .rsplit('/')
                .next()
                .and_then(|v| v.parse().ok())
            {
                self.len = Some(total);
            }
            let range_end = content_range
                .split(['-', '/'])
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
                .map(|last| last + 1);
            self.chunk_end = range_end
                .or(content_length.map(|n| self.pos + n))
                .unwrap_or(u64::MAX);
        } else {
            if content_length.is_some() {
                self.len = content_length;
            }
            self.chunk_end = u64::MAX;
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
        let mut retried = false;
        loop {
            if buf.is_empty() || self.len.is_some_and(|len| self.pos >= len) {
                return Ok(0);
            }
            if self.body.get_mut().unwrap().is_none() || self.pos >= self.chunk_end {
                self.connect()?;
            }
            let body = self.body.get_mut().unwrap().as_mut().expect("connected");
            match body.read(buf) {
                Ok(0) if self.ranges && self.len.is_some() => {
                    if self.pos >= self.chunk_end {
                        // Chunk finished: the loop requests the next one.
                        *self.body.get_mut().unwrap() = None;
                    } else if !retried {
                        // The connection ended early: request the rest.
                        retried = true;
                        *self.body.get_mut().unwrap() = None;
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "server closed the connection early",
                        ));
                    }
                }
                Ok(n) => {
                    self.pos += n as u64;
                    return Ok(n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(e) if retried => return Err(e),
                Err(_) => {
                    // A dropped connection mid-stream: reconnect once and resume.
                    retried = true;
                    *self.body.get_mut().unwrap() = None;
                }
            }
        }
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
        let chunk_end = self.chunk_end;
        let body = self.body.get_mut().unwrap();
        match body {
            // Close ahead within the open chunk: read through to it.
            Some(open)
                if target > self.pos
                    && target - self.pos <= SKIP_THRESHOLD
                    && target < chunk_end =>
            {
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
                let mut end = data.len();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (first, last) = v.trim().split_once('-').unwrap();
                        start = first.parse().unwrap();
                        end = last
                            .parse::<usize>()
                            .map_or(data.len(), |l| (l + 1).min(data.len()));
                    }
                }
                let head = if ranges {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n",
                        end - 1,
                        data.len(),
                        end - start
                    )
                } else {
                    start = 0;
                    end = data.len();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len()
                    )
                };
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&data[start..end]);
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
    fn small_chunks_read_the_whole_file() {
        let data = data();
        let mut reader =
            HttpReader::open_with_chunk_size(&serve(data.clone(), true), &[], 7_919).unwrap();
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, data);
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
