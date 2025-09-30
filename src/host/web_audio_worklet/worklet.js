registerProcessor("CpalProcessor", class WasmProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.WasmAudioProcessor.unpack(handle);
        this.wasm_memory = new Float32Array(memory.buffer);
    }
    process(inputs, outputs) {
        const channels = outputs[0];
        const channels_count = channels.length;
        const frame_size = channels[0].length;

        const interleaved_ptr = this.processor.process(channels_count, frame_size, sampleRate, currentTime);

        const FLOAT32_SIZE_BYTES = 4;
        const interleaved_start = interleaved_ptr / FLOAT32_SIZE_BYTES;
        const interleaved = this.wasm_memory.subarray(interleaved_start, interleaved_start + channels_count * frame_size);

        for (let ch = 0; ch < channels_count; ch++) {
            const channel = channels[ch];

            for (let i = 0, j = ch; i < frame_size; i++, j += channels_count) {
                channel[i] = interleaved[j];
            }
        }

        return true;
    }
});