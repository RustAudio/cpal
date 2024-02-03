registerProcessor(
  "cpal-worklet",
  class CpalWorklet extends AudioWorkletProcessor {
    /**
     * @type {Float32Array | null}
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
          this.ints = new Int32Array(this.shared);
          this.floats = new Float32Array(this.ints.length - 1);
          this.view = new DataView(this.floats.buffer);
          this.isInput = msg.data.isInput;

          console.log("worklet", "registered output sources");
          break;
        }
        default: {
          console.error("worklet", "unknown message", msg);
        }
      }
    }

    /**
     * @param {Float32Array[][]}
     * @param {Float32Array[][]}
     * @param {any}
     *
     */
    process([input], [output], _parameters) {
      if (this.ints && this.view) {
        if (this.isInput) {
          if (input && input.length) {
            const frames = input[0].length;
            const channels = input.length;
            // write last input into buffer
            for (let fr = 0; fr < frames; fr++) {
              for (let ch = 0; ch < channels; ch++) {
                // frame index
                const i = fr * channels + ch;
                // data
                const f = output[ch][fr];
                // set on view
                this.view.setFloat32(i * Float32Array.BYTES_PER_ELEMENT, f);
                // get as int
                const f_int = this.view.getInt32(
                  i * Int32Array.BYTES_PER_ELEMENT
                );
                // store sample
                Atomics.store(this.ints, i, f_int);
              }
            }
          }
        } else {
          if (output && output.length) {
            const frames = output[0].length;
            const channels = output.length;
            // read last output from buffer
            for (let fr = 0; fr < frames; fr++) {
              for (let ch = 0; ch < channels; ch++) {
                // frame index
                const i = fr * channels + ch;
                // load stored frame
                const f_int = Atomics.load(this.ints, i);
                // set on view
                this.view.setInt32(i * Int32Array.BYTES_PER_ELEMENT, f_int);
                // get as float
                const f = this.view.getFloat32(
                  i * Float32Array.BYTES_PER_ELEMENT
                );
                // write sample
                output[ch][fr] = f;
              }
            }
          }
        }

        // change state to BridgePhase::WorkletDone
        Atomics.store(this.ints, this.ints.length - 1, 1);
        Atomics.notify(this.ints, this.ints.length - 1);
        // post message if there's a listener
        this.port.postMessage({ type: "worklet_done" });
      }

      // keep going
      // https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletProcessor/process#return_value
      return true;
    }
  }
);
