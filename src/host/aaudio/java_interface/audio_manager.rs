use super::{
    utils::{get_context, get_system_property, with_attached, Env, JResult},
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

fn get_mixer_bursts<'j>(env: &mut Env<'j>) -> JResult<i32> {
    let mixer_bursts = get_system_property(env, "aaudio.mixer_bursts", "2")?;

    let mixer_bursts_string = String::from(mixer_bursts.mutf8_chars(env)?);

    mixer_bursts_string
        .parse::<i32>()
        .map_err(|e| jni::errors::Error::ParseFailed(e.to_string()))
}
