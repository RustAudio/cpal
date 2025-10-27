use std::sync::Arc;

extern crate jni;

use self::jni::Executor;
use self::jni::{errors::Result as JResult, JNIEnv, JavaVM};

// constants from android.media.AudioFormat
pub const ENCODING_PCM_16BIT: i32 = 2;
pub const ENCODING_PCM_FLOAT: i32 = 4;
pub const CHANNEL_OUT_MONO: i32 = 4;
pub const CHANNEL_OUT_STEREO: i32 = 12;

fn with_attached<F, R>(closure: F) -> JResult<R>
where
    F: FnOnce(&mut JNIEnv) -> JResult<R>,
{
    let android_context = ndk_context::android_context();
    let vm = Arc::new(unsafe { JavaVM::from_raw(android_context.vm().cast())? });
    Executor::new(vm).with_attached(|env| closure(env))
}
