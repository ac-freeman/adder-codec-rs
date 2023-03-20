use crate::codec::header::{Magic, MAGIC_RAW};
use crate::codec::{CodecError, CodecMetadata, WriteCompression};
use crate::Event;
use std::io::{Sink, Write};

/// Filler for when generated ADΔER events need not be captured
pub struct EmptyOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Write> EmptyOutput<W> {
    /// Create a new empty output stream.
    pub fn new(meta: CodecMetadata, _writer: W) -> Self {
        Self {
            meta,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<W: std::io::Write> WriteCompression<W> for EmptyOutput<Sink> {
    fn magic(&self) -> Magic {
        MAGIC_RAW
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, _bytes: &[u8]) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn into_writer(&mut self) -> Option<W> {
        None
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn compress(&self, _data: &[u8]) -> Vec<u8> {
        vec![]
    }

    fn ingest_event(&mut self, _event: Event) -> Result<(), CodecError> {
        Ok(())
    }
}
