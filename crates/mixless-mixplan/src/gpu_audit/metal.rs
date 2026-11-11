use super::{
    metrics::{Metrics, Score},
    Job, Message,
};
use ::metal::{
    Buffer, CommandQueue, ComputePipelineState, Device, MTLCommandBufferStatus, MTLResourceOptions,
    MTLSize,
};
use objc::{msg_send, sel, sel_impl};
use std::{
    collections::HashMap,
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Arc,
    },
    time::{Duration, Instant},
};

#[repr(C)]
struct Query {
    offset: u32,
    count: u32,
    tonality: u32,
    reserved: u32,
    shape: [f32; 16],
}
const _: () = assert!(std::mem::size_of::<Query>() == 80 && std::mem::size_of::<Score>() == 8);

pub(super) struct Engine {
    pub name: String,
    device: Device,
    queue: CommandQueue,
    pipeline: ComputePipelineState,
    buffers: [Option<Buffer>; 3],
}
impl Engine {
    pub fn new() -> Result<Self, String> {
        let device = Device::system_default().ok_or("No Metal device")?;
        let options = ::metal::CompileOptions::new();
        // Available on the macOS 12 deployment target; maps to safe/precise
        // math on newer systems. Still not assumed bit-identical to Rust.
        options.set_fast_math_enabled(false);
        let library = device.new_library_with_source(include_str!("score.metal"), &options)?;
        let function = library.get_function("score_batch", None)?;
        let pipeline = device.new_compute_pipeline_state_with_function(&function)?;
        let queue = device.new_command_queue();
        Ok(Self {
            name: device.name().to_owned(),
            device,
            queue,
            pipeline,
            buffers: [None, None, None],
        })
    }
    fn buffer(&mut self, index: usize, bytes: usize) -> Result<(), String> {
        let size = bytes.max(4).next_power_of_two();
        if size as u64 > self.device.max_buffer_length() {
            return Err("GPU batch exceeds device buffer limit".into());
        }
        if self.buffers[index]
            .as_ref()
            .is_none_or(|b| b.length() < size as u64)
        {
            self.buffers[index] = Some(
                self.device
                    .new_buffer(size as u64, MTLResourceOptions::StorageModeShared),
            );
        }
        if self.buffers[index].as_ref().unwrap().contents().is_null() {
            return Err("GPU shared buffer allocation failed".into());
        }
        Ok(())
    }
    pub(super) fn evaluate(&mut self, jobs: &[Job]) -> Result<(Vec<Score>, f64), String> {
        let mut frames = Vec::<[f32; 40]>::new();
        let mut queries = Vec::<Query>::new();
        let mut offsets = HashMap::new();
        for job in jobs {
            let offset = *offsets
                .entry(Arc::as_ptr(&job.input) as usize)
                .or_insert_with(|| {
                    let offset = frames.len();
                    frames.extend_from_slice(&job.input.frames);
                    offset
                });
            for shape in &job.shapes {
                queries.push(Query {
                    offset: offset as u32,
                    count: job.input.frames.len() as u32,
                    tonality: job.input.tonality,
                    reserved: 0,
                    shape: *shape,
                });
            }
        }
        if queries.is_empty() {
            return Ok((vec![], 0.));
        }
        self.buffer(0, std::mem::size_of_val(frames.as_slice()))?;
        self.buffer(1, std::mem::size_of_val(queries.as_slice()))?;
        self.buffer(2, queries.len() * std::mem::size_of::<Score>())?;
        let [input, query, output] = self.buffers.each_ref().map(|b| b.as_ref().unwrap());
        // SAFETY: buffers have checked capacities, source structs have no
        // padding, and the preceding dispatch completed before reuse. CPU
        // accesses these shared resources only outside GPU command execution.
        unsafe {
            std::ptr::copy_nonoverlapping(frames.as_ptr(), input.contents().cast(), frames.len());
            std::ptr::copy_nonoverlapping(queries.as_ptr(), query.contents().cast(), queries.len());
        }
        let command = self.queue.new_command_buffer();
        let encoder = command.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&self.pipeline);
        encoder.set_buffer(0, Some(input), 0);
        encoder.set_buffer(1, Some(query), 0);
        encoder.set_buffer(2, Some(output), 0);
        let count = queries.len() as u32;
        encoder.set_bytes(3, 4, (&count as *const u32).cast());
        let width = self.pipeline.thread_execution_width();
        encoder.dispatch_thread_groups(
            MTLSize::new((count as u64).div_ceil(width), 1, 1),
            MTLSize::new(width, 1, 1),
        );
        encoder.end_encoding();
        command.commit();
        command.wait_until_completed();
        if command.status() != MTLCommandBufferStatus::Completed {
            return Err(format!("Metal command status {:?}", command.status()));
        }
        let gpu_seconds = gpu_time(command);
        // SAFETY: the completed kernel writes one initialized Score per query;
        // the shared output has enough capacity and is no longer in GPU use.
        let scores =
            unsafe { std::slice::from_raw_parts(output.contents().cast::<Score>(), queries.len()) }
                .to_vec();
        Ok((scores, gpu_seconds))
    }
}

pub(super) fn drain(mut engine: Engine, receiver: Receiver<Message>) -> Metrics {
    let mut metrics = Metrics::default();
    let mut ended = false;
    let mut failed = false;
    while !ended {
        let first = match receiver.recv() {
            Ok(Message::Job(job)) => job,
            _ => break,
        };
        let mut jobs = vec![first];
        let until = Instant::now() + Duration::from_micros(250);
        while jobs.len() < 32 {
            match receiver.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(Message::Job(job)) => jobs.push(job),
                Ok(Message::Finish) | Err(RecvTimeoutError::Disconnected) => {
                    ended = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        let count: usize = jobs.iter().map(|j| j.shapes.len()).sum();
        if failed {
            metrics.uncompared += count as u64;
            continue;
        }
        let start = Instant::now();
        // Release command-buffer/encoder autoreleased objects after each batch.
        let result = objc::rc::autoreleasepool(|| engine.evaluate(&jobs));
        metrics.dispatch_seconds += start.elapsed().as_secs_f64();
        metrics.batches += 1;
        metrics.max_batch = metrics.max_batch.max(count);
        match result {
            Ok((scores, seconds)) => {
                metrics.gpu_seconds += seconds;
                let mut offset = 0;
                for job in jobs {
                    metrics.jobs += 1;
                    let mut cpu_offset = 0;
                    for count in job.groups {
                        let cpu = &job.cpu[cpu_offset..cpu_offset + count];
                        let gpu = &scores[offset..offset + count];
                        if metrics.observe(cpu, gpu) && metrics.examples.len() < 8 {
                            metrics.examples.push(serde_json::json!({
                                "tonality":job.input.tonality,
                                "frame_bits":job.input.frames.iter().map(|f| f.map(f32::to_bits).to_vec()).collect::<Vec<_>>(),
                                "shape_bits":job.shapes[cpu_offset..cpu_offset+count].iter().map(|s|s.map(f32::to_bits)).collect::<Vec<_>>(),
                                "cpu_score_bits":cpu.iter().map(|s|s.map(f32::to_bits)).collect::<Vec<_>>(),
                                "gpu_score_bits":gpu.iter().map(|s|s.value().map(f32::to_bits)).collect::<Vec<_>>(),
                            }));
                        }
                        offset += count;
                        cpu_offset += count;
                    }
                }
            }
            Err(error) => {
                metrics.errors.push(error);
                metrics.uncompared += count as u64;
                failed = true;
            }
        }
    }
    metrics
}

// objc 0.2's selector macro contains its own legacy cargo-clippy cfg. Limit
// the compatibility allowance to these two supported command-buffer selectors.
#[allow(unexpected_cfgs)]
fn gpu_time(command: &::metal::CommandBufferRef) -> f64 {
    // SAFETY: completed command buffers expose double timestamps on macOS 12+.
    unsafe {
        let start: f64 = msg_send![command, GPUStartTime];
        let end: f64 = msg_send![command, GPUEndTime];
        (end - start).max(0.)
    }
}
