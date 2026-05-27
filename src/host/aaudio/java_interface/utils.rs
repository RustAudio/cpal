use jni::sys::jobject;
pub use jni::{
    errors::Result as JResult,
    objects::{JIntArray, JObject, JObjectArray, JString},
    strings::JNIString,
    Env, JavaVM,
};
use ndk_context::AndroidContext;

pub fn get_context() -> AndroidContext {
    ndk_context::android_context()
}

pub fn with_attached<F, R>(context: AndroidContext, closure: F) -> JResult<R>
where
    for<'j> F: FnOnce(&mut Env<'j>, JObject<'j>) -> JResult<R>,
{
    // jni 0.22: from_raw returns JavaVM directly and asserts non-null,
    // so attach_current_thread and the closure are the only fallible steps.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) };
    let raw_context = context.context() as jobject;
    vm.attach_current_thread(|env: &mut Env<'_>| {
        let context_obj = unsafe { JObject::from_raw(env, raw_context) };
        closure(env, context_obj)
    })
}

pub fn call_method_no_args_ret_int_array<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
) -> JResult<Vec<i32>> {
    let obj = env
        .call_method(subject, JNIString::new(method), jni::jni_sig!("()[I"), &[])?
        .l()?;
    let array: JIntArray<'j> = unsafe { JIntArray::from_raw(env, obj.into_raw()) };
    let length = array.len(env)?;
    let mut values = vec![0i32; length];
    array.get_region(env, 0, &mut values)?;
    Ok(values)
}

pub fn call_method_no_args_ret_int<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
) -> JResult<i32> {
    env.call_method(subject, JNIString::new(method), jni::jni_sig!("()I"), &[])?
        .i()
}

pub fn call_method_no_args_ret_bool<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
) -> JResult<bool> {
    env.call_method(subject, JNIString::new(method), jni::jni_sig!("()Z"), &[])?
        .z()
}

pub fn call_method_no_args_ret_string<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
) -> JResult<JString<'j>> {
    let obj = env
        .call_method(
            subject,
            JNIString::new(method),
            jni::jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    Ok(unsafe { JString::from_raw(env, obj.into_raw()) })
}

pub fn call_method_no_args_ret_char_sequence<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
) -> JResult<JString<'j>> {
    let cseq = env
        .call_method(
            subject,
            JNIString::new(method),
            jni::jni_sig!("()Ljava/lang/CharSequence;"),
            &[],
        )?
        .l()?;

    let s_obj = env
        .call_method(
            &cseq,
            jni::jni_str!("toString"),
            jni::jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    Ok(unsafe { JString::from_raw(env, s_obj.into_raw()) })
}

pub fn call_method_string_arg_ret_bool<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    name: &str,
    arg: impl AsRef<str>,
) -> JResult<bool> {
    let arg_str = env.new_string(arg)?;
    env.call_method(
        subject,
        JNIString::new(name),
        jni::jni_sig!("(Ljava/lang/String;)Z"),
        &[(&arg_str).into()],
    )?
    .z()
}

pub fn call_method_string_arg_ret_object<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    method: &str,
    arg: &str,
) -> JResult<JObject<'j>> {
    let arg_str = env.new_string(arg)?;
    env.call_method(
        subject,
        JNIString::new(method),
        jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[(&arg_str).into()],
    )?
    .l()
}

pub fn get_package_manager<'j>(env: &mut Env<'j>, subject: &JObject<'j>) -> JResult<JObject<'j>> {
    env.call_method(
        subject,
        jni::jni_str!("getPackageManager"),
        jni::jni_sig!("()Landroid/content/pm/PackageManager;"),
        &[],
    )?
    .l()
}

pub fn has_system_feature<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    name: &str,
) -> JResult<bool> {
    call_method_string_arg_ret_bool(env, subject, "hasSystemFeature", name)
}

pub fn get_system_service<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    name: &str,
) -> JResult<JObject<'j>> {
    call_method_string_arg_ret_object(env, subject, "getSystemService", name)
}

/// Read an Android system property
pub fn get_system_property<'j>(
    env: &mut Env<'j>,
    name: &str,
    default_value: &str,
) -> JResult<JString<'j>> {
    let name_str = env.new_string(name)?;
    let default_str = env.new_string(default_value)?;
    let obj = env
        .call_static_method(
            jni::jni_str!("android/os/SystemProperties"),
            jni::jni_str!("get"),
            jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
            &[(&name_str).into(), (&default_str).into()],
        )?
        .l()?;
    Ok(unsafe { JString::from_raw(env, obj.into_raw()) })
}

pub fn get_devices<'j>(
    env: &mut Env<'j>,
    subject: &JObject<'j>,
    flags: i32,
) -> JResult<JObjectArray<'j>> {
    let obj = env
        .call_method(
            subject,
            jni::jni_str!("getDevices"),
            jni::jni_sig!("(I)[Landroid/media/AudioDeviceInfo;"),
            &[flags.into()],
        )?
        .l()?;
    let arr = unsafe { JObjectArray::<JObject<'j>>::from_raw(env, obj.into_raw()) };
    Ok(arr)
}
