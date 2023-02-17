use crate::codec::header::{Magic, MAGIC_RAW};
use crate::codec::{CodecError, CodecMetadata, WriteCompression};
use crate::Event;
use std::io::Write;

pub struct EmptyOutput {
    pub(crate) meta: CodecMetadata,
}

impl<W: std::io::Write> WriteCompression<W> for EmptyOutput {
    fn new(meta: CodecMetadata, writer: W) -> Self {
        Self { meta }
    }

    fn magic(&self) -> Magic {
        MAGIC_RAW
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn into_writer(self: Box<Self>) -> W {
        unimplemented!()
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn compress(&self, _data: &[u8]) -> Vec<u8> {
        vec![]
    }

    fn ingest_event(&mut self, _event: &Event) -> Result<(), CodecError> {
        Ok(())
    }
}
