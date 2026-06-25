use num_traits::FromPrimitive;

use super::{
    android_device_flags,
    utils::{
        call_method_no_args_ret_bool, call_method_no_args_ret_char_sequence,
        call_method_no_args_ret_int, call_method_no_args_ret_int_array,
        call_method_no_args_ret_string, get_context, get_devices, get_system_service,
        with_attached, Env, JObject, JResult,
    },
    AudioDeviceInfo, AudioDeviceType, Context,
};
use crate::{DeviceDirection, SampleFormat};

impl AudioDeviceInfo {
    /**
     * Request audio devices using Android Java API
     */
    pub fn request(direction: DeviceDirection) -> Result<Vec<AudioDeviceInfo>, String> {
        let context = get_context();

        with_attached(context, |env, context| {
            let sdk_version = env
                .get_static_field(
                    jni::jni_str!("android/os/Build$VERSION"),
                    jni::jni_str!("SDK_INT"),
                    jni::jni_sig!("I"),
                )?
                .i()?;

            if sdk_version >= 23 {
                try_request_devices_info(env, &context, direction)
            } else {
                Err(jni::errors::Error::MethodNotFound {
                    name: "".into(),
                    sig: "".into(),
                })
            }
        })
        .map_err(|error| error.to_string())
    }
}

fn try_request_devices_info<'j>(
    env: &mut Env<'j>,
    context: &JObject<'j>,
    direction: DeviceDirection,
) -> JResult<Vec<AudioDeviceInfo>> {
    let audio_manager = get_system_service(env, context, Context::AUDIO_SERVICE)?;

    let devices = get_devices(env, &audio_manager, android_device_flags(direction))?;

    let length = devices.len(env)?;

    (0..length)
        .map(|index| {
            let device = devices.get_element(env, index)?;
            let id = call_method_no_args_ret_int(env, &device, "getId")?;
            let address = call_method_no_args_ret_string(env, &device, "getAddress")?;
            let address = String::from(address.mutf8_chars(env)?);
            let product_name =
                call_method_no_args_ret_char_sequence(env, &device, "getProductName")?;
            let product_name = String::from(product_name.mutf8_chars(env)?);
            let device_type =
                FromPrimitive::from_i32(call_method_no_args_ret_int(env, &device, "getType")?)
                    .unwrap_or(AudioDeviceType::Unsupported);

            let is_source = call_method_no_args_ret_bool(env, &device, "isSource")?;
            let is_sink = call_method_no_args_ret_bool(env, &device, "isSink")?;
            let direction = crate::device_description::direction_from_caps(is_source, is_sink);
            let channel_counts =
                call_method_no_args_ret_int_array(env, &device, "getChannelCounts")?
                    .into_boxed_slice();
            let sample_rates = call_method_no_args_ret_int_array(env, &device, "getSampleRates")?
                .into_boxed_slice();
            let formats = call_method_no_args_ret_int_array(env, &device, "getEncodings")?
                .into_iter()
                .filter_map(SampleFormat::from_encoding)
                .collect::<Box<[_]>>();

            Ok(AudioDeviceInfo {
                id,
                address,
                product_name,
                device_type,
                direction,
                channel_counts,
                sample_rates,
                formats,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}
