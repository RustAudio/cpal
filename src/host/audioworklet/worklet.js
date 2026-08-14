registerProcessor("CpalProcessor", class WasmProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.WasmAudioProcessor.unpack(handle);
        this.memory = memory;
        this.wasm_memory = new Float32Array(memory.buffer);
    }

    process(inputs, outputs) {
        // Check if memory grew and update view
        if (this.wasm_memory.buffer !== this.memory.buffer) {
            this.wasm_memory = new Float32Array(this.memory.buffer);
        }

        const channels = outputs[0];
        const channels_count = channels.length;
        const frame_size = channels[0].length;
        const interleaved_ptr = this.processor.process(
            channels_count,
            frame_size,
            sampleRate,
            currentTime
        );

        const interleaved_start = interleaved_ptr / Float32Array.BYTES_PER_ELEMENT;
        const interleaved = this.wasm_memory;

        const total_samples = frame_size * channels_count;
        if (interleaved_start + total_samples > this.wasm_memory.length) {
            console.error("CpalProcessor: Audio buffer out of bounds! Ptr:", interleaved_ptr, "Len:", total_samples);
            return false; // Safely stop the node
        }

        // Deinterleave: read strided from Wasm, write sequential to output
        for (let ch = 0; ch < channels_count; ch++) {
            const channel = channels[ch];
            let read_pos = interleaved_start + ch;

            for (let i = 0; i < frame_size; i++) {
                channel[i] = interleaved[read_pos];
                read_pos += channels_count;
            }
        }

        return true;
    }
});

registerProcessor("CpalCaptureProcessor", class WasmCaptureProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.WasmAudioCaptureProcessor.unpack(handle);
        this.memory = memory;
        this.wasm_memory = new Float32Array(memory.buffer);
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

        // capture_buffer_ptr() may have grown Wasm memory; refresh the view before writing.
        if (this.wasm_memory.buffer !== this.memory.buffer) {
            this.wasm_memory = new Float32Array(this.memory.buffer);
        }

        const interleaved_start = interleaved_ptr / Float32Array.BYTES_PER_ELEMENT;
        const total_samples = frame_size * channels_count;
        if (interleaved_start + total_samples > this.wasm_memory.length) {
            console.error("CpalCaptureProcessor: Audio buffer out of bounds! Ptr:", interleaved_ptr, "Len:", total_samples);
            return false; // Safely stop the node
        }

        // Interleave: read sequential from the input channels, write strided into Wasm
        for (let ch = 0; ch < channels_count; ch++) {
            const channel = channels[ch];
            let write_pos = interleaved_start + ch;

            for (let i = 0; i < frame_size; i++) {
                this.wasm_memory[write_pos] = channel[i];
                write_pos += channels_count;
            }
        }

        this.processor.process_captured(channels_count, frame_size, sampleRate, currentTime);

        return true;
    }
});
