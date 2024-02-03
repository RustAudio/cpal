/**
 * @type {Int32Array?}
 */
let intArr;

let abort = new AbortController();

/**
 * @param {MessageEvent} message
 *
 * Receives messages from host
 */
self.onmessage = (message) => {
  switch (message.data && message.data["type"]) {
    case "schedule_tick": {
      console.log("waiter", "schedule_tick", message.data);
      schedule_tick(
        new Int32Array(message.data.buffer),
        Boolean(message.data.input),
        Boolean(message.data.output),
        abort.signal
      );
      break;
    }
    case "cancel_tick": {
      abort.abort(message.data);
      abort = new AbortController();
      break;
    }
    default: {
      console.error("waiter", "unknown message", message);
    }
  }
};

/**
 * @param {Int32Array} intArr
 * @param {boolean} withInput
 * @param {boolean} withOutput
 * @param {AbortSignal} signal
 *
 * Waits for the last digit in shared buffer to be updated, to reflect state change
 * Stores next state
 * Waits for input data if any
 * Waits for output data if any
 * Schedules next tick unless aborted
 *
 */

function schedule_tick(intArr, withInput, withOutput, signal) {
  console.log("waiter", "schedule tick");
  const i = intArr.length - 1;

  // BridgePhase::ReadWrite
  Atomics.wait(intArr, i, 2);

  console.log("waiter", "ticking");

  // BridgePhase::Demand
  Atomics.store(intArr, i, 3);

  if (withInput) {
    self.postMessage({ type: "input_data" });

    console.log("waiter", "tick input");
    // BridgePhase::Demand
    Atomics.wait(intArr, i, 3);
  }

  if (withOutput) {
    self.postMessage({ type: "output_data" });

    console.log("waiter", "tick output");
    // BridgePhase::Input or BridgePhase::Demand
    Atomics.wait(intArr, i, withInput ? 0 : 3);
  }

  const waitsOn = Atomics.load(intArr, i);

  console.log("waiter", "wait for worklet");
  Atomics.wait(intArr, i, waitsOn);

  console.log("waiter", "schedule next");

  if (!signal.aborted) {
    schedule_tick(intArr, withInput, withOutput, signal);
  }
}
