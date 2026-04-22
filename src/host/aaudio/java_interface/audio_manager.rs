use super::{
    utils::{get_context, get_system_property, with_attached, JNIEnv, JResult},
    AudioManager,
};

impl AudioManager {
    /// Get the AAudio mixer burst count from system property
    pub fn get_mixer_bursts() -> Result<i32, String> {
        let context = get_context();

        with_attached(context, |env, _context| get_mixer_bursts(env))
            .map_err(|error| error.to_string())
    }
}

fn get_mixer_bursts<'j>(env: &mut JNIEnv<'j>) -> JResult<i32> {
    let mixer_bursts = get_system_property(env, "aaudio.mixer_bursts", "2")?;

    let mixer_bursts_string = String::from(env.get_string(&mixer_bursts)?);

    // TODO: Use jni::errors::Error::ParseFailed instead of jni::errors::Error::JniCall once jni > v0.21.1 is released
    mixer_bursts_string
        .parse::<i32>()
        .map_err(|_| jni::errors::Error::JniCall(jni::errors::JniError::Unknown))
}
