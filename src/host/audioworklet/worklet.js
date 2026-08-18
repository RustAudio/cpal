// Shared plumbing for the cpal processors. Interleaving and deinterleaving are done here rather
// than in Rust because it avoids an extra copy and the JS engine optimizes these loops well.
class CpalProcessorBase extends AudioWorkletProcessor {
    constructor(options, ProcessorClass) {
        super();
        let [module, memory, handle] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = ProcessorClass.unpack(handle);
        this.name = ProcessorClass.name;
        this.memory = memory;
        this.wasm_memory = new Float32Array(memory.buffer);
    }

    // Growing Wasm memory detaches the old view, so it has to be re-taken after any call into
    // Rust that may have allocated. Both accessors below do this for their caller.
    refresh_memory() {
        if (this.wasm_memory.buffer !== this.memory.buffer) {
            this.wasm_memory = new Float32Array(this.memory.buffer);
        }
    }

    // Resolves `ptr` against the current Wasm memory, or returns -1 (having logged) if the range
    // it addresses does not fit.
    sample_offset(ptr, samples) {
        this.refresh_memory();
        const start = ptr / Float32Array.BYTES_PER_ELEMENT;
        if (start + samples > this.wasm_memory.length) {
            console.error(`${this.name}: Audio buffer out of bounds! Ptr:`, ptr, "Len:", samples);
            return -1;
        }
        return start;
    }

    // Reads sequentially from `channels` and writes strided into Wasm at byte pointer `ptr`.
    // Returns false if the buffer does not fit in Wasm memory.
    interleave(channels, ptr, frame_size) {
        const channels_count = channels.length;
        const start = this.sample_offset(ptr, frame_size * channels_count);
        if (start < 0) {
            return false;
        }

        const interleaved = this.wasm_memory;
        for (let ch = 0; ch < channels_count; ch++) {
            const channel = channels[ch];
            let write_pos = start + ch;

            for (let i = 0; i < frame_size; i++) {
                interleaved[write_pos] = channel[i];
                write_pos += channels_count;
            }
        }
        return true;
    }

    // Reads strided from Wasm at byte pointer `ptr` and writes sequentially into `channels`.
    // Returns false if the buffer does not fit in Wasm memory.
    deinterleave(channels, ptr, frame_size) {
        const channels_count = channels.length;
        const start = this.sample_offset(ptr, frame_size * channels_count);
        if (start < 0) {
            return false;
        }

        const interleaved = this.wasm_memory;
        for (let ch = 0; ch < channels_count; ch++) {
            const channel = channels[ch];
            let read_pos = start + ch;

            for (let i = 0; i < frame_size; i++) {
                channel[i] = interleaved[read_pos];
                read_pos += channels_count;
            }
        }
        return true;
    }
}

registerProcessor("CpalProcessor", class WasmProcessor extends CpalProcessorBase {
    constructor(options) {
        super(options, bindgen.WasmAudioProcessor);
    }

    process(inputs, outputs) {
        const channels = outputs[0];
        const frame_size = channels[0].length;

        const interleaved_ptr = this.processor.process(
            channels.length,
            frame_size,
            sampleRate,
            currentTime
        );

        // Safely stop the node if the buffer does not fit.
        return this.deinterleave(channels, interleaved_ptr, frame_size);
    }
});

registerProcessor("CpalCaptureProcessor", class WasmCaptureProcessor extends CpalProcessorBase {
    constructor(options) {
        super(options, bindgen.WasmAudioCaptureProcessor);
    }

    process(inputs) {
        const channels = inputs[0];
        const channels_count = channels.length;
        if (channels_count === 0) {
            // No source connected to the node yet.
            return true;
        }
        const frame_size = channels[0].length;

        const interleaved_ptr = this.processor.capture_buffer_ptr(channels_count, frame_size);
        if (!this.interleave(channels, interleaved_ptr, frame_size)) {
            return false; // Safely stop the node
        }

        this.processor.process_captured(channels_count, frame_size, sampleRate, currentTime);

        return true;
    }
});

registerProcessor("CpalDuplexProcessor", class WasmDuplexProcessor extends CpalProcessorBase {
    constructor(options) {
        super(options, bindgen.WasmAudioDuplexProcessor);
    }

    process(inputs, outputs) {
        const output_channels = outputs[0];
        const output_channels_count = output_channels.length;
        const frame_size = output_channels[0].length;

        // inputs[0] is empty until the microphone source is connected. Keep rendering output
        // with a zero-channel input rather than stalling the graph waiting for it.
        const input_channels = inputs[0];
        const input_channels_count = input_channels.length;

        const input_ptr = this.processor.prepare(
            input_channels_count,
            output_channels_count,
            frame_size
        );
        if (!this.interleave(input_channels, input_ptr, frame_size)) {
            return false; // Safely stop the node
        }

        // prepare() sized both buffers, so this cannot grow Wasm memory again. Read it before
        // process() runs the user callback, which can.
        const output_ptr = this.processor.output_buffer_ptr();

        this.processor.process(
            input_channels_count,
            output_channels_count,
            frame_size,
            sampleRate,
            currentTime
        );

        return this.deinterleave(output_channels, output_ptr, frame_size);
    }
});
