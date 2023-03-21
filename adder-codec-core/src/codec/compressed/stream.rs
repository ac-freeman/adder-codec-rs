use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::cmp::min;
use std::collections::VecDeque;
use std::io::{Read, Write};

use crate::codec::compressed::adu::cube::AduCube;
use crate::codec::compressed::adu::frame::{Adu, AduChannelType};
use crate::codec::compressed::adu::interblock::AduInterBlock;
use crate::codec::compressed::adu::intrablock::AduIntraBlock;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::blocks::block::Frame;
use crate::codec::compressed::blocks::{BLOCK_SIZE, BLOCK_SIZE_AREA};
use crate::codec::header::{Magic, MAGIC_COMPRESSED};
use crate::codec_old::compressed::compression::{
    d_residual_default_weights, dt_residual_default_weights, Contexts,
};
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::codec_old::compressed::fenwick::Weights;
use crate::Mode::{Continuous, FramePerfect};
use crate::{Coord, DeltaT, Event, EventCoordless, SourceCamera};

/// Write compressed ADΔER data to a stream.
pub struct CompressedOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    pub(crate) frame: Frame,
    pub(crate) adu: Adu,
    pub(crate) arithmetic_coder:
        Option<arithmetic_coding::Encoder<FenwickModel, BitWriter<W, BigEndian>>>,
    pub(crate) contexts: Option<Contexts>,
    pub(crate) stream: Option<BitWriter<W, BigEndian>>,
}

/// Read compressed ADΔER data from a stream.
pub struct CompressedInput<R: Read> {
    pub(crate) meta: CodecMetadata,
    pub(crate) arithmetic_coder:
        Option<arithmetic_coding::Decoder<FenwickModel, BitReader<R, BigEndian>>>,
    pub(crate) contexts: Option<Contexts>,

    // Stores the decoded events so they can be read one by one. They're put into reverse order
    // (todo) when the ADU is decoded, so that they can be popped off the end of the vector.
    decoded_event_queue: Vec<Event>,
    _phantom: std::marker::PhantomData<R>,
}

impl<W: Write> CompressedOutput<W> {
    /// Create a new compressed output stream.
    pub fn new(meta: CodecMetadata, writer: W) -> Self {
        let mut source_model = FenwickModel::with_symbols(
            min(meta.delta_t_max as usize * 2, u16::MAX as usize),
            1 << 30,
        );

        let contexts = Contexts::new(&mut source_model, meta);

        let arithmetic_coder = Encoder::new(source_model);

        Self {
            meta,
            frame: Frame::new(
                meta.plane.w_usize(),
                meta.plane.h_usize(),
                meta.plane.c() > 1,
                match meta.source_camera {
                    SourceCamera::FramedU8 => FramePerfect,
                    SourceCamera::FramedU16 => FramePerfect,
                    SourceCamera::FramedU32 => FramePerfect,
                    SourceCamera::FramedU64 => FramePerfect,
                    SourceCamera::FramedF32 => FramePerfect,
                    SourceCamera::FramedF64 => FramePerfect,
                    SourceCamera::Dvs => Continuous,
                    SourceCamera::DavisU8 => Continuous,
                    SourceCamera::Atis => Continuous,
                    SourceCamera::Asint => Continuous,
                },
            ),
            adu: Adu::new(),
            arithmetic_coder: Some(arithmetic_coder),
            contexts: Some(contexts),
            stream: Some(BitWriter::endian(writer, BigEndian)),
        }
    }

    /// Convenience function to get a mutable reference to the underlying stream.
    #[inline(always)]
    pub(crate) fn stream(&mut self) -> &mut BitWriter<W, BigEndian> {
        self.stream.as_mut().unwrap()
    }

    fn organize_adus(&mut self) {
        for (cube_idx, cube) in self.frame.cubes.iter_mut().enumerate() {
            let mut block = &mut cube.blocks_r[0];
            let mut inter_model = &mut cube.inter_model_r;

            let (start_t, start_d, d_residuals, dt_residuals, sparam) = inter_model
                .forward_intra_prediction(
                    0,
                    self.meta.ref_interval,
                    self.meta.delta_t_max,
                    &block.events,
                );

            if cube_idx == 0 {
                self.adu.head_event_t = start_t;
            }

            let intra_block = AduIntraBlock {
                head_event_t: start_t,
                head_event_d: start_d,
                shift_loss_param: sparam,
                d_residuals: *d_residuals,
                dt_residuals: *dt_residuals,
            };
            let mut adu_cube = AduCube::from_intra_block(
                intra_block,
                cube.cube_idx_y as u16,
                cube.cube_idx_x as u16,
            );

            let base_sparam = 7; // TODO: Dynamic control of this parameter

            for block in cube.blocks_r.iter_mut().skip(1) {
                let (d_residuals, t_residuals, sparam) = inter_model.forward_inter_prediction(
                    base_sparam,
                    self.meta.delta_t_max,
                    self.meta.ref_interval,
                    &block.events,
                );

                adu_cube.add_inter_block(AduInterBlock {
                    shift_loss_param: sparam,
                    d_residuals: *d_residuals,
                    t_residuals: *t_residuals,
                });
            }

            self.adu.add_cube(adu_cube, AduChannelType::R);
        }
    }

    pub fn compress_events(&mut self) -> Result<Adu, CodecError> {
        eprintln!("Compressing events...");
        self.organize_adus();
        let adu_debug = self.adu.clone();
        match (
            self.arithmetic_coder.as_mut(),
            self.contexts.as_mut(),
            self.stream.as_mut(),
        ) {
            (Some(encoder), Some(contexts), Some(stream)) => {
                self.adu
                    .compress(encoder, contexts, stream, self.meta.delta_t_max)?;
                self.frame.reset();
                self.adu = Adu::new();

                // TODO: Temporary! Write a function to just reset the probability tables
                // let mut source_model = FenwickModel::with_symbols(
                //     min(self.meta.delta_t_max as usize * 2, u16::MAX as usize),
                //     1 << 30,
                // );
                // *contexts = Contexts::new(&mut source_model, self.meta.clone());
                // *encoder = Encoder::new(source_model);
            }
            (_, _, _) => {
                return Err(CodecError::MalformedEncoder);
            }
        }
        Ok(adu_debug)
    }
}

impl<W: Write> WriteCompression<W> for CompressedOutput<W> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        self.stream().write_bytes(bytes)
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        self.stream().byte_align()
    }

    fn into_writer(&mut self) -> Option<W> {
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(self.contexts.as_ref().unwrap().eof_context);
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .encode(None, self.stream.as_mut().unwrap())
            .unwrap();
        // Must flush the encoder to the bitwriter before flushing the bitwriter itself
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .flush(&mut self.stream.as_mut().unwrap())
            .unwrap();
        self.stream().byte_align().unwrap();
        self.flush_writer().unwrap();
        let tmp = std::mem::replace(&mut self.stream, None);
        tmp.map(|bitwriter| bitwriter.into_writer())
    }

    // fn into_writer(self: Self) -> Option<Box<W>> {
    //     Some(Box::new(self.stream.into_writer()))
    // }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        if let (true, _) = self.frame.add_event(event, self.meta.delta_t_max)? {
            self.compress_events()?;
            self.frame.add_event(event, self.meta.delta_t_max)?;
        };
        Ok(())
    }
    fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
        if let (true, _) = self.frame.add_event(event, self.meta.delta_t_max)? {
            let adu = self.compress_events()?;
            self.frame.add_event(event, self.meta.delta_t_max)?;
            return Ok(Some(adu));
        };
        Ok(None)
    }
}

impl<R: Read> CompressedInput<R> {
    /// Create a new compressed input stream.
    pub fn new(delta_t_max: DeltaT, ref_interval: DeltaT) -> Self
    where
        Self: Sized,
    {
        let mut source_model =
            FenwickModel::with_symbols(min(delta_t_max as usize * 2, u16::MAX as usize), 1 << 30);

        let contexts = Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
            },
        ); // TODO refactor and clean this up

        let arithmetic_coder = Decoder::new(source_model);

        Self {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
            },
            arithmetic_coder: Some(arithmetic_coder),
            contexts: Some(contexts),
            decoded_event_queue: Vec::new(),
            // stream: BitReader::endian(reader, BigEndian),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: Read> ReadCompression<R> for CompressedInput<R> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> std::io::Result<()> {
        reader.read_bytes(bytes)
    }

    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R {
    //     reader.into_reader()
    // }

    #[allow(unused_variables)]
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        if self.decoded_event_queue.is_empty() {
            // Reset the probability tables
            // self.frame.reset();

            // TODO: Temporary! Write a function to just reset the probability tables
            // let mut source_model = FenwickModel::with_symbols(
            //     min(self.meta.delta_t_max as usize * 2, u16::MAX as usize),
            //     1 << 30,
            // );
            // *self.contexts.as_mut().unwrap() = Contexts::new(&mut source_model, self.meta.clone());
            // *self.arithmetic_coder.as_mut().unwrap() = Decoder::new(source_model);

            // Then read and decode the next ADU
            let decoded_adu = Adu::decompress(reader, self);
            for cube in decoded_adu.cubes_r.cubes {
                // intra residual tshifts inverse

                // for each inter block, inter residual tshifts inverse
            }
        }

        // Then return the next event from the queue
        match self.decoded_event_queue.pop() {
            Some(event) => Ok(event),
            None => Err(CodecError::Eof),
        }
    }

    #[allow(unused_variables)]
    fn digest_event_debug(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        adu: &Adu,
    ) -> Result<Event, CodecError> {
        if self.decoded_event_queue.is_empty() {
            // Reset the probability tables
            // self.frame.reset();

            // TODO: Temporary! Write a function to just reset the probability tables
            // let mut source_model = FenwickModel::with_symbols(
            //     min(self.meta.delta_t_max as usize * 2, u16::MAX as usize),
            //     1 << 30,
            // );
            // *self.contexts.as_mut().unwrap() = Contexts::new(&mut source_model, self.meta.clone());
            // *self.arithmetic_coder.as_mut().unwrap() = Decoder::new(source_model);

            // Then read and decode the next ADU
            let decoded_adu = Adu::decompress_debug(reader, self, adu);
            for cube in decoded_adu.cubes_r.cubes {
                // intra residual tshifts inverse

                // for each inter block, inter residual tshifts inverse
            }
        }

        // Then return the next event from the queue
        match self.decoded_event_queue.pop() {
            Some(event) => Ok(event),
            None => Err(CodecError::Eof),
        }
    }

    #[allow(unused_variables)]
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError> {
        todo!()
    }
}
