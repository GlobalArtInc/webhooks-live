use async_zip::tokio::write::{EntryStreamWriter, ZipFileWriter};
use async_zip::{Compression, ZipEntryBuilder};
use futures_util::io::AsyncWriteExt;
use tokio::io::AsyncWrite;

use crate::encode::INDEX_ENTRY_NAME;
use crate::error::ExportError;

pub struct Archive<W: AsyncWrite + Unpin> {
    writer: ZipFileWriter<W>,
}

impl<W: AsyncWrite + Unpin> Archive<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: ZipFileWriter::with_tokio(writer),
        }
    }

    pub async fn put_body(&mut self, name: &str, data: &[u8]) -> Result<(), ExportError> {
        let entry = ZipEntryBuilder::new(name.into(), Compression::Stored);
        let mut stream = self.writer.write_entry_stream(entry).await?;
        stream.write_all(data).await?;
        stream.close().await?;
        Ok(())
    }

    pub async fn open_index(&mut self) -> Result<IndexEntry<'_, W>, ExportError> {
        let entry = ZipEntryBuilder::new(INDEX_ENTRY_NAME.into(), Compression::Deflate);
        let stream = self.writer.write_entry_stream(entry).await?;
        Ok(IndexEntry { stream })
    }

    pub async fn close(self) -> Result<W, ExportError> {
        let compat = self.writer.close().await?;
        Ok(compat.into_inner())
    }
}

pub struct IndexEntry<'a, W: AsyncWrite + Unpin> {
    stream: EntryStreamWriter<'a, W>,
}

impl<W: AsyncWrite + Unpin> IndexEntry<'_, W> {
    pub async fn push(&mut self, line: &[u8]) -> Result<(), ExportError> {
        self.stream.write_all(line).await?;
        Ok(())
    }

    pub async fn finish(self) -> Result<(), ExportError> {
        self.stream.close().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[tokio::test]
    async fn the_archive_holds_named_body_entries_and_the_index_and_ends_with_its_directory() {
        let mut archive = Archive::new(Vec::new());
        archive
            .put_body("bodies/1-capture.bin", b"hello")
            .await
            .expect("the body entry is written");
        let mut index = archive.open_index().await.expect("the index opens");
        index
            .push(b"{}\n")
            .await
            .expect("the index line is written");
        index.finish().await.expect("the index closes");
        let bytes = archive.close().await.expect("the archive closes");
        assert!(bytes.starts_with(b"PK\x03\x04"));
        assert!(contains(&bytes, b"bodies/1-capture.bin"));
        assert!(contains(&bytes, b"captures.ndjson"));
        assert!(contains(&bytes, b"PK\x05\x06"));
    }
}
