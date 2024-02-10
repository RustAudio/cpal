registerProcessor(
  "cpal-worklet",
  class CpalWorklet extends AudioWorkletProcessor {
    /**
     * @type {number}
     *
     * size of one chunk (frames * samples)
     */

    chunkSize = 0;

    /**
     * @type {number}
     *
     * number of chunks in buffer
     */
    chunks = 0;

    /**
     * @type {Float32Array | null}
     *
     * float for converting one sample
     */
    floats = null;

    /**
     * @type {DataView | null}
     *
     * view over floats
     */
    view = null;

    /**
     * @type {SharedArrayBuffer | null}
     *
     * shared memory
     */
    shared = null;

    /**
     * @type {Int32Array | null}
     *
     * shared as ints
     */
    ints = null;

    /**
     * @type {bool}
     *
     * worklet behavior
     */
    isInput = false;

    constructor() {
      super();
      this.port.onmessage = this.onMessage.bind(this);
    }

    onMessage(msg) {
      switch (msg.data.type) {
        case "buffer": {
          this.shared = msg.data.buffer;
          this.chunkSize = msg.data.chunk_size;
          this.chunks = msg.data.chunks;
          this.ints = new Int32Array(this.shared);
          this.floats = new Float32Array(1);
          this.view = new DataView(this.floats.buffer);
          this.isInput = msg.data.isInput;

          console.log("worklet", "registered shared buffer");
          break;
        }
        default: {
          console.error("worklet", "unknown message", msg);
        }
      }
    }

    /**
     * @param {Float32Array[]}
     */

    writeInput(input) {
      // const frames = input[0].length;
      // const channels = input.length;
      // // write last input into buffer
      // for (let fr = 0; fr < frames; fr++) {
      //   for (let ch = 0; ch < channels; ch++) {
      //     // frame index
      //     const i = fr * channels + ch;
      //     // data
      //     const f = output[ch][fr];
      //     // set on view
      //     this.view.setFloat32(i * Float32Array.BYTES_PER_ELEMENT, f);
      //     // get as int
      //     const f_int = this.view.getInt32(
      //       i * Int32Array.BYTES_PER_ELEMENT
      //     );
      //     // store sample
      //     Atomics.store(this.ints, i, f_int);
      //   }
      // }
      // // change state to BridgePhase::WorkletDone
      // Atomics.store(this.ints, this.ints.length - 1, 1);
      // Atomics.notify(this.ints, this.ints.length - 1);
      // // post message if there's a listener
      // this.port.postMessage({ type: "worklet_done" });
    }

    /**
     * @param {Float32Array[][]}
     */
    readOutput(outputs) {
      const channels = outputs[0].length;
      const i = Atomics.load(this.ints, this.ints.length - this.chunks);

      if (i >= 0) {
        const start = i * this.chunkSize;
        const end = start + this.chunkSize - 1;

        for (let s = start; s <= end; s++) {
          const int = Atomics.load(this.ints, s);
          this.view.setInt32(0, int, true);
          const float = this.view.getFloat32(0, true);
          const s_i = s - start;
          const ch = channels - ((s_i + 1) % channels);
          const fr = (s_i - ch + 1) / channels;
          outputs.forEach(output => {
            output[ch - 1][fr] = float;
          })
        }

        Atomics.store(
          this.ints,
          this.ints.length - 1,
          -1
        );

        for (let chunk = 0; chunk < this.chunks - 1; chunk++) {
          const next = Atomics.load(
            this.ints,
            this.ints.length - this.chunks + chunk + 1
          );

          Atomics.store(
            this.ints,
            this.ints.length - this.chunks + chunk,
            next
          );
        }

        Atomics.notify(this.ints, this.ints.length - this.chunks);
      }
      else {
        console.log("empty buffer")
      }

      this.port.postMessage({ type: "worklet_done" });
    }

    /**
     * @param {Float32Array[][]}
     * @param {Float32Array[][]}
     * @param {any}
     *
     */
    process([input], outputs, _parameters) {
      if (this.ints && this.view) {
        if (this.isInput) {
          if (input && input.length) {
            this.writeInput(input);
          }
        } else {
          if (outputs.length && outputs[0].length) {
            this.readOutput(outputs);
          }
        }
      }

      // keep going
      // https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletProcessor/process#return_value
      return true;
    }
  }
);
