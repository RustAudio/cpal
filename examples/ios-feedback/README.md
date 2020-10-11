# iOS Feedback Example

This example is an Xcode project that exercises both input and output
audio streams. Audio samples are read in from your micrphone and then
routed to your audio output device with a small but noticeable delay
so you can verify it is working correctly.

To build the example you will need to still `cargo-lipo`. While not
necessary for building iOS binaries, it is used to build a universal
binary (x86 for simulator and aarch64 for device.)

```
cargo install cargo-lipo
```

Then open the XCode project and click run. A hook in the iOS application
lifecycle calls into the Rust code to start the input/output feedback
loop and immediately returns back control.

Before calling into Rust, the AVAudioSession category is configured.
This is important for controlling how audio is shared with the rest
of the system when your app is in the foreground. One example is
controlling whether other apps can play music in the background.
More information [here](https://developer.apple.com/library/archive/documentation/Audio/Conceptual/AudioSessionProgrammingGuide/AudioSessionCategoriesandModes/AudioSessionCategoriesandModes.html#//apple_ref/doc/uid/TP40007875-CH10).

