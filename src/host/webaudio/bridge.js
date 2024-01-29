const WORKLET = `
registerProcessor("cpal-worklet", class CpalWorklet extends AudioWorkletProcessor {
  
  outputProducerArrays = null;
  outputProducerViews = null;
  outputProducerIntArrays = null;

  constructor() {
      super();
      this.port.onmessage = this.onMessage.bind(this);
  }

  onMessage(msg) {
      switch (msg.data.type) {
          case "output": {
              this.outputProducerArrays = msg.data.value;
              this.outputProducerIntArrays = this.outputProducerArrays.map(d => new Int32Array(d));
              this.outputProducerViews = this.outputProducerArrays.map(d => new DataView(d));
              console.log("registered output sources")
              break;
          }
          default: {
              console.error("unknown message", msg);
          }
      }
  }

  // destructure inputs and outputs, support only one stream
  process([input], [output], _parameters) {
      

      if (this.outputProducerViews) {
          console.time("worklet tick")
          for (let ch = 0; ch < output.length; ch++) { 
              const channelView = this.outputProducerViews[ch];
              for (let fr = 0; fr < output[ch].length; fr++) {
                  output[ch][fr] = channelView.getFloat32(Float32Array.BYTES_PER_ELEMENT * fr)
              }
              Atomics.store(this.outputProducerIntArrays[ch], output[ch].length, 0);
              Atomics.notify(this.outputProducerIntArrays[ch], output[ch].length);
          }
          console.timeEnd("worklet tick")
      }

      // keep going
      // https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletProcessor/process#return_value
      return true;
  }
});
`;

export class CpalBridge {
  /**
   * @type {Promise<void> | null}
   */
  registerModulePromise = null;

  /**
   * @type {AudioWorkletNode | null}
   */
  workletNode = null;

  /**
   * @type {(()=>void)[]}
   * queuing calls to self until the module is registered
   */
  q = [];

  onResume = null;

  /**
   * @param {BaseAudioContext} ctx
   * @param {number} channels
   * @param {number} frames
   */
  constructor(ctx, channels, frames) {
    this.context = ctx;

    this.abortController = new AbortController();

    // ensure the bridge assumption holds true
    if (Float32Array.BYTES_PER_ELEMENT !== Int32Array.BYTES_PER_ELEMENT) {
      throw new Error(
        "expected Float32Array.BYTES_PER_ELEMENT to equal Int32Array.BYTES_PER_ELEMENT"
      );
    }

    /**
     * @type {SharedArrayBuffer[]}
     * share frames with audio worklet
     */
    this._outputBuffers = Array.from({ length: channels }).map(
      () => new SharedArrayBuffer(Float32Array.BYTES_PER_ELEMENT * (frames + 1))
    );

    /**
     * @type {Int32Array[]}
     * stores the frames in Int32 representation
     */
    this.outputBuffersInt = this._outputBuffers.map((b) => new Int32Array(b));

    /**
     * @type {Float32Array[]}
     * bridge owned frames
     */
    this._outputValues = Array.from({ length: channels }).map(
      () => new Float32Array(frames)
    );

    /**
     * @type {DataView[]}
     * view over frames used for conversion
     */
    this.outputViews = this._outputValues.map((b) => new DataView(b.buffer));

    try {
      this.configure();
    } catch {
      this.registerModulePromise = new Promise(async (resolve, reject) => {
        try {
          await this.context.audioWorklet.addModule(
            `data:application/javascript,${encodeURIComponent(WORKLET)}`
          );
          this.configure();
          console.log("created worklet node");
          resolve();
        } catch (e) {
          console.error("Failed to add worklet module", e);
          reject(e);
        } finally {
          this.q.forEach((cb) => cb());
          this.registerModulePromise = null;
        }
      });
    }
  }

  configure() {
    this.workletNode = new AudioWorkletNode(this.context, "cpal-worklet");
    this.numberOfInputs = this.workletNode.numberOfInputs;
    this.numberOfOutputs = this.workletNode.numberOfOutputs;
    this.channelCount = this.workletNode.channelCount;
    this.channelCountMode = this.workletNode.channelCountMode;
    this.channelInterpretation = this.workletNode.channelInterpretation;
  }

  connect(...params) {
    if (this.workletNode) {
      this.workletNode.connect(...params);
      console.log("connected worklet node");
    } else {
      this.q.push(() => {
        this.connect(...params);
      });
    }
  }

  disconnect() {
    if (this.workletNode) {
      this.workletNode.disconnect();
    } else {
      this.q.push(() => {
        this.disconnect();
      });
    }
  }

  registerInputCallback(cb) {
    if (this.workletNode) {
      this.workletNode.port.postMessage({ type: "input", value: cb });
    } else {
      this.q.push(() => {
        this.registerInputCallback(cb);
      });
    }
  }

  /**
   * @param {() => Float32Array[]} cb
   * @param {DataView[]} viewsSrc
   * @param {Int32Array[]} sharedIntsSrc
   *
   * @returns {Promise<void>}
   */
  createTick(cb, viewsSrc, sharedIntsSrc) {
    const signal = this.abortController.signal;
    return new Promise(async (resolve, reject) => {
      console.time("bridge tick")
      try {
        /**
         * @type {[Float32Array, DataView, Int32Array][]}
         */
        const data = cb().map((d, i) => [d, viewsSrc[i], sharedIntsSrc[i]]);

        for (let [mainOwnedData, view, sharedInts] of data) {
          for (let i = 0; i < mainOwnedData.length; i++) {
            const offset = Float32Array.BYTES_PER_ELEMENT * i;
            view.setFloat32(offset, mainOwnedData[i]);
            const int = view.getInt32(offset);
            Atomics.store(sharedInts, i, int);
          }
          Atomics.store(sharedInts, mainOwnedData.length, 1);
        }
      } catch (e) {
        console.error(e);
        reject(e);
      }
      console.timeEnd("bridge tick")
      console.time("wait");
      sharedIntsSrc.forEach((sh) => {
        Atomics.wait(sh, sh.length - 1, 1);
      });
      console.timeEnd("wait");
      if (!signal.aborted) {
        return this.createTick(cb, viewsSrc, sharedIntsSrc);
      } else {
        resolve();
      }
    });
  }

  /**
   * @param {() => Float32Array[]} cb
   * @param {number} interval
   */
  registerOutputCallback(cb, interval) {
    if (this.workletNode) {
      this.workletNode.port.postMessage({
        type: "output",
        value: this._outputBuffers,
      });
      this.outputStream = this.createTick(
        cb,
        this.outputViews,
        this.outputBuffersInt,
        interval
      );
      this.onResume = () => {
        this.outputStream = this.createTick(
          cb,
          this.outputViews,
          this.outputBuffersInt,
          interval
        );
      };
    } else {
      this.q.push(() => {
        this.registerOutputCallback(cb);
      });
    }
  }

  stop() {
    this.abortController.abort();
    this.abortController = new AbortController();
  }

  resume() {
    // this.onResume();
  }
}
