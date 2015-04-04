#![allow(missing_copy_implementations)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use libc::{c_int, c_uint, c_long, c_longlong, size_t, ssize_t, c_void, c_char, c_uchar, c_ulong, c_double, FILE, c_ushort, c_short, pid_t, timeval, timespec};
use std::{mem};

extern crate libc;

pub static SND_PCM_NONBLOCK: c_int = 0x1;
pub static SND_PCM_ASYNC:    c_int = 0x2;

#[repr(C)]
pub struct snd_dlsym_link {
    pub next: *mut snd_dlsym_link,
    pub dlsym_name: *const c_char,
    pub dlsym_ptr: *const c_void,
}

pub enum snd_async_handler_t { }

pub type snd_async_callback_t = Option<extern fn(arg1: *mut snd_async_handler_t)>;
pub enum snd_shm_area { }
pub type snd_timestamp_t = timeval;
pub type snd_htimestamp_t = timespec;
pub enum snd_input_t { }

pub type snd_input_type_t = c_uint;
pub const SND_INPUT_STDIO:  c_uint = 0;
pub const SND_INPUT_BUFFER: c_uint = 1;

pub enum snd_output_t { }

pub type snd_output_type_t = c_uint;
pub const SND_OUTPUT_STDIO:  c_uint = 0;
pub const SND_OUTPUT_BUFFER: c_uint = 1;

pub type snd_lib_error_handler_t =
    Option<extern fn(arg1: *const c_char, arg2: c_int, arg3: *const c_char, arg4: c_int,
                     arg5: *const c_char, ...)>;

//pub type snd_local_error_handler_t =
//    Option<extern fn(arg1: *const c_char, arg2: c_int, arg3: *const c_char, arg4: c_int,
//                     arg5: *const c_char, arg6: va_list)>;

pub type snd_config_type_t = c_uint;
pub const SND_CONFIG_TYPE_INTEGER:   c_uint = 0;
pub const SND_CONFIG_TYPE_INTEGER64: c_uint = 1;
pub const SND_CONFIG_TYPE_REAL:      c_uint = 2;
pub const SND_CONFIG_TYPE_STRING:    c_uint = 3;
pub const SND_CONFIG_TYPE_POINTER:   c_uint = 4;
pub const SND_CONFIG_TYPE_COMPOUND:  c_uint = 1024;

pub enum snd_config_t { }

pub enum Struct__snd_config_iterator { }
pub type snd_config_iterator_t = *mut Struct__snd_config_iterator;

pub enum snd_config_update_t { }

#[repr(C)]
pub struct snd_devname_t {
    pub name:    *mut c_char,
    pub comment: *mut c_char,
    pub next:    *mut snd_devname_t,
}

pub enum snd_pcm_info_t { }
pub enum snd_pcm_hw_params_t { }
pub enum snd_pcm_sw_params_t { }
pub enum snd_pcm_status_t { }
pub enum snd_pcm_access_mask_t { }
pub enum snd_pcm_format_mask_t { }
pub enum snd_pcm_subformat_mask_t { }

pub type snd_pcm_class_t = c_uint;
pub const SND_PCM_CLASS_GENERIC:   c_uint = 0;
pub const SND_PCM_CLASS_MULTI:     c_uint = 1;
pub const SND_PCM_CLASS_MODEM:     c_uint = 2;
pub const SND_PCM_CLASS_DIGITIZER: c_uint = 3;
pub const SND_PCM_CLASS_LAST:      c_uint = 3;

pub type snd_pcm_subclass_t = c_uint;
pub const SND_PCM_SUBCLASS_GENERIC_MIX: c_uint = 0;
pub const SND_PCM_SUBCLASS_MULTI_MIX:   c_uint = 1;
pub const SND_PCM_SUBCLASS_LAST:        c_uint = 1;

pub type snd_pcm_stream_t = c_uint;
pub const SND_PCM_STREAM_PLAYBACK: c_uint = 0;
pub const SND_PCM_STREAM_CAPTURE:  c_uint = 1;
pub const SND_PCM_STREAM_LAST:     c_uint = 1;

pub type snd_pcm_access_t = c_uint;
pub const SND_PCM_ACCESS_MMAP_INTERLEAVED:    c_uint = 0;
pub const SND_PCM_ACCESS_MMAP_NONINTERLEAVED: c_uint = 1;
pub const SND_PCM_ACCESS_MMAP_COMPLEX:        c_uint = 2;
pub const SND_PCM_ACCESS_RW_INTERLEAVED:      c_uint = 3;
pub const SND_PCM_ACCESS_RW_NONINTERLEAVED:   c_uint = 4;
pub const SND_PCM_ACCESS_LAST:                c_uint = 4;

pub type snd_pcm_format_t = c_int;
pub const SND_PCM_FORMAT_UNKNOWN:            c_int = -1;
pub const SND_PCM_FORMAT_S8:                 c_int = 0;
pub const SND_PCM_FORMAT_U8:                 c_int = 1;
pub const SND_PCM_FORMAT_S16_LE:             c_int = 2;
pub const SND_PCM_FORMAT_S16_BE:             c_int = 3;
pub const SND_PCM_FORMAT_U16_LE:             c_int = 4;
pub const SND_PCM_FORMAT_U16_BE:             c_int = 5;
pub const SND_PCM_FORMAT_S24_LE:             c_int = 6;
pub const SND_PCM_FORMAT_S24_BE:             c_int = 7;
pub const SND_PCM_FORMAT_U24_LE:             c_int = 8;
pub const SND_PCM_FORMAT_U24_BE:             c_int = 9;
pub const SND_PCM_FORMAT_S32_LE:             c_int = 10;
pub const SND_PCM_FORMAT_S32_BE:             c_int = 11;
pub const SND_PCM_FORMAT_U32_LE:             c_int = 12;
pub const SND_PCM_FORMAT_U32_BE:             c_int = 13;
pub const SND_PCM_FORMAT_FLOAT_LE:           c_int = 14;
pub const SND_PCM_FORMAT_FLOAT_BE:           c_int = 15;
pub const SND_PCM_FORMAT_FLOAT64_LE:         c_int = 16;
pub const SND_PCM_FORMAT_FLOAT64_BE:         c_int = 17;
pub const SND_PCM_FORMAT_IEC958_SUBFRAME_LE: c_int = 18;
pub const SND_PCM_FORMAT_IEC958_SUBFRAME_BE: c_int = 19;
pub const SND_PCM_FORMAT_MU_LAW:             c_int = 20;
pub const SND_PCM_FORMAT_A_LAW:              c_int = 21;
pub const SND_PCM_FORMAT_IMA_ADPCM:          c_int = 22;
pub const SND_PCM_FORMAT_MPEG:               c_int = 23;
pub const SND_PCM_FORMAT_GSM:                c_int = 24;
pub const SND_PCM_FORMAT_SPECIAL:            c_int = 31;
pub const SND_PCM_FORMAT_S24_3LE:            c_int = 32;
pub const SND_PCM_FORMAT_S24_3BE:            c_int = 33;
pub const SND_PCM_FORMAT_U24_3LE:            c_int = 34;
pub const SND_PCM_FORMAT_U24_3BE:            c_int = 35;
pub const SND_PCM_FORMAT_S20_3LE:            c_int = 36;
pub const SND_PCM_FORMAT_S20_3BE:            c_int = 37;
pub const SND_PCM_FORMAT_U20_3LE:            c_int = 38;
pub const SND_PCM_FORMAT_U20_3BE:            c_int = 39;
pub const SND_PCM_FORMAT_S18_3LE:            c_int = 40;
pub const SND_PCM_FORMAT_S18_3BE:            c_int = 41;
pub const SND_PCM_FORMAT_U18_3LE:            c_int = 42;
pub const SND_PCM_FORMAT_U18_3BE:            c_int = 43;
pub const SND_PCM_FORMAT_G723_24:            c_int = 44;
pub const SND_PCM_FORMAT_G723_24_1B:         c_int = 45;
pub const SND_PCM_FORMAT_G723_40:            c_int = 46;
pub const SND_PCM_FORMAT_G723_40_1B:         c_int = 47;
pub const SND_PCM_FORMAT_DSD_U8:             c_int = 48;
pub const SND_PCM_FORMAT_DSD_U16_LE:         c_int = 49;
pub const SND_PCM_FORMAT_LAST:               c_int = 49;
pub const SND_PCM_FORMAT_S16:                c_int = 2;
pub const SND_PCM_FORMAT_U16:                c_int = 4;
pub const SND_PCM_FORMAT_S24:                c_int = 6;
pub const SND_PCM_FORMAT_U24:                c_int = 8;
pub const SND_PCM_FORMAT_S32:                c_int = 10;
pub const SND_PCM_FORMAT_U32:                c_int = 12;
pub const SND_PCM_FORMAT_FLOAT:              c_int = 14;
pub const SND_PCM_FORMAT_FLOAT64:            c_int = 16;
pub const SND_PCM_FORMAT_IEC958_SUBFRAME:    c_int = 18;

pub type snd_pcm_subformat_t = c_uint;
pub const SND_PCM_SUBFORMAT_STD:  c_uint = 0;
pub const SND_PCM_SUBFORMAT_LAST: c_uint = 0;

pub type snd_pcm_state_t = c_uint;
pub const SND_PCM_STATE_OPEN:         c_uint = 0;
pub const SND_PCM_STATE_SETUP:        c_uint = 1;
pub const SND_PCM_STATE_PREPARED:     c_uint = 2;
pub const SND_PCM_STATE_RUNNING:      c_uint = 3;
pub const SND_PCM_STATE_XRUN:         c_uint = 4;
pub const SND_PCM_STATE_DRAINING:     c_uint = 5;
pub const SND_PCM_STATE_PAUSED:       c_uint = 6;
pub const SND_PCM_STATE_SUSPENDED:    c_uint = 7;
pub const SND_PCM_STATE_DISCONNECTED: c_uint = 8;
pub const SND_PCM_STATE_LAST:         c_uint = 8;

pub type snd_pcm_start_t = c_uint;
pub const SND_PCM_START_DATA:     c_uint = 0;
pub const SND_PCM_START_EXPLICIT: c_uint = 1;
pub const SND_PCM_START_LAST:     c_uint = 1;

pub type snd_pcm_xrun_t = c_uint;
pub const SND_PCM_XRUN_NONE: c_uint = 0;
pub const SND_PCM_XRUN_STOP: c_uint = 1;
pub const SND_PCM_XRUN_LAST: c_uint = 1;

pub type snd_pcm_tstamp_t = c_uint;
pub const SND_PCM_TSTAMP_NONE:   c_uint = 0;
pub const SND_PCM_TSTAMP_ENABLE: c_uint = 1;
pub const SND_PCM_TSTAMP_MMAP:   c_uint = 1;
pub const SND_PCM_TSTAMP_LAST:   c_uint = 1;

pub type snd_pcm_uframes_t = c_ulong;
pub type snd_pcm_sframes_t = c_long;
pub enum snd_pcm_t { }

pub type snd_pcm_type_t = c_uint;
pub const SND_PCM_TYPE_HW:           c_uint = 0;
pub const SND_PCM_TYPE_HOOKS:        c_uint = 1;
pub const SND_PCM_TYPE_MULTI:        c_uint = 2;
pub const SND_PCM_TYPE_FILE:         c_uint = 3;
pub const SND_PCM_TYPE_NULL:         c_uint = 4;
pub const SND_PCM_TYPE_SHM:          c_uint = 5;
pub const SND_PCM_TYPE_INET:         c_uint = 6;
pub const SND_PCM_TYPE_COPY:         c_uint = 7;
pub const SND_PCM_TYPE_LINEAR:       c_uint = 8;
pub const SND_PCM_TYPE_ALAW:         c_uint = 9;
pub const SND_PCM_TYPE_MULAW:        c_uint = 10;
pub const SND_PCM_TYPE_ADPCM:        c_uint = 11;
pub const SND_PCM_TYPE_RATE:         c_uint = 12;
pub const SND_PCM_TYPE_ROUTE:        c_uint = 13;
pub const SND_PCM_TYPE_PLUG:         c_uint = 14;
pub const SND_PCM_TYPE_SHARE:        c_uint = 15;
pub const SND_PCM_TYPE_METER:        c_uint = 16;
pub const SND_PCM_TYPE_MIX:          c_uint = 17;
pub const SND_PCM_TYPE_DROUTE:       c_uint = 18;
pub const SND_PCM_TYPE_LBSERVER:     c_uint = 19;
pub const SND_PCM_TYPE_LINEAR_FLOAT: c_uint = 20;
pub const SND_PCM_TYPE_LADSPA:       c_uint = 21;
pub const SND_PCM_TYPE_DMIX:         c_uint = 22;
pub const SND_PCM_TYPE_JACK:         c_uint = 23;
pub const SND_PCM_TYPE_DSNOOP:       c_uint = 24;
pub const SND_PCM_TYPE_DSHARE:       c_uint = 25;
pub const SND_PCM_TYPE_IEC958:       c_uint = 26;
pub const SND_PCM_TYPE_SOFTVOL:      c_uint = 27;
pub const SND_PCM_TYPE_IOPLUG:       c_uint = 28;
pub const SND_PCM_TYPE_EXTPLUG:      c_uint = 29;
pub const SND_PCM_TYPE_MMAP_EMUL:    c_uint = 30;
pub const SND_PCM_TYPE_LAST:         c_uint = 30;

#[repr(C)]
pub struct snd_pcm_channel_area_t {
    pub addr: *mut c_void,
    pub first: c_uint,
    pub step: c_uint,
}

#[repr(C)]
pub struct snd_pcm_sync_id_t {
    pub data: [u32; 4us],
}
impl snd_pcm_sync_id_t {
    pub fn id(&mut self) -> *mut [c_uchar; 16us] {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn id16(&mut self) -> *mut [c_ushort; 8us] {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn id32(&mut self) -> *mut [c_uint; 4us] {
        unsafe { ::std::mem::transmute(self) }
    }
}

pub enum snd_pcm_scope_t { }

pub type snd_pcm_chmap_type = c_uint;
pub const SND_CHMAP_TYPE_NONE:   c_uint = 0;
pub const SND_CHMAP_TYPE_FIXED:  c_uint = 1;
pub const SND_CHMAP_TYPE_VAR:    c_uint = 2;
pub const SND_CHMAP_TYPE_PAIRED: c_uint = 3;
pub const SND_CHMAP_TYPE_LAST:   c_uint = 3;

pub type snd_pcm_chmap_position = c_uint;
pub const SND_CHMAP_UNKNOWN: c_uint = 0;
pub const SND_CHMAP_NA:      c_uint = 1;
pub const SND_CHMAP_MONO:    c_uint = 2;
pub const SND_CHMAP_FL:      c_uint = 3;
pub const SND_CHMAP_FR:      c_uint = 4;
pub const SND_CHMAP_RL:      c_uint = 5;
pub const SND_CHMAP_RR:      c_uint = 6;
pub const SND_CHMAP_FC:      c_uint = 7;
pub const SND_CHMAP_LFE:     c_uint = 8;
pub const SND_CHMAP_SL:      c_uint = 9;
pub const SND_CHMAP_SR:      c_uint = 10;
pub const SND_CHMAP_RC:      c_uint = 11;
pub const SND_CHMAP_FLC:     c_uint = 12;
pub const SND_CHMAP_FRC:     c_uint = 13;
pub const SND_CHMAP_RLC:     c_uint = 14;
pub const SND_CHMAP_RRC:     c_uint = 15;
pub const SND_CHMAP_FLW:     c_uint = 16;
pub const SND_CHMAP_FRW:     c_uint = 17;
pub const SND_CHMAP_FLH:     c_uint = 18;
pub const SND_CHMAP_FCH:     c_uint = 19;
pub const SND_CHMAP_FRH:     c_uint = 20;
pub const SND_CHMAP_TC:      c_uint = 21;
pub const SND_CHMAP_TFL:     c_uint = 22;
pub const SND_CHMAP_TFR:     c_uint = 23;
pub const SND_CHMAP_TFC:     c_uint = 24;
pub const SND_CHMAP_TRL:     c_uint = 25;
pub const SND_CHMAP_TRR:     c_uint = 26;
pub const SND_CHMAP_TRC:     c_uint = 27;
pub const SND_CHMAP_TFLC:    c_uint = 28;
pub const SND_CHMAP_TFRC:    c_uint = 29;
pub const SND_CHMAP_TSL:     c_uint = 30;
pub const SND_CHMAP_TSR:     c_uint = 31;
pub const SND_CHMAP_LLFE:    c_uint = 32;
pub const SND_CHMAP_RLFE:    c_uint = 33;
pub const SND_CHMAP_BC:      c_uint = 34;
pub const SND_CHMAP_BLC:     c_uint = 35;
pub const SND_CHMAP_BRC:     c_uint = 36;
pub const SND_CHMAP_LAST:    c_uint = 36;

#[repr(C)]
pub struct snd_pcm_chmap_t {
    pub channels: c_uint,
    pub pos: [c_uint; 0us],
}

#[repr(C)]
pub struct snd_pcm_chmap_query_t {
    pub _type: snd_pcm_chmap_type,
    pub map: snd_pcm_chmap_t,
}

pub type snd_pcm_hook_type_t = c_uint;
pub const SND_PCM_HOOK_TYPE_HW_PARAMS: c_uint = 0;
pub const SND_PCM_HOOK_TYPE_HW_FREE:   c_uint = 1;
pub const SND_PCM_HOOK_TYPE_CLOSE:     c_uint = 2;
pub const SND_PCM_HOOK_TYPE_LAST:      c_uint = 2;

pub enum snd_pcm_hook_t { }
pub type snd_pcm_hook_func_t = Option<extern fn(arg1: *mut snd_pcm_hook_t) -> c_int>;

#[repr(C)]
pub struct snd_pcm_scope_ops_t {
    pub enable:  Option<extern fn (arg1: *mut snd_pcm_scope_t) -> c_int>,
    pub disable: Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
    pub start:   Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
    pub stop:    Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
    pub update:  Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
    pub reset:   Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
    pub close:   Option<extern fn (arg1: *mut snd_pcm_scope_t)>,
}

pub type snd_spcm_latency_t = c_uint;
pub const SND_SPCM_LATENCY_STANDARD: c_uint = 0;
pub const SND_SPCM_LATENCY_MEDIUM:   c_uint = 1;
pub const SND_SPCM_LATENCY_REALTIME: c_uint = 2;

pub type snd_spcm_xrun_type_t = c_uint;
pub const SND_SPCM_XRUN_IGNORE: c_uint = 0;
pub const SND_SPCM_XRUN_STOP:   c_uint = 1;

pub type snd_spcm_duplex_type_t = c_uint;
pub const SND_SPCM_DUPLEX_LIBERAL:  c_uint = 0;
pub const SND_SPCM_DUPLEX_PEDANTIC: c_uint = 1;

pub enum snd_rawmidi_info_t { }

pub enum snd_rawmidi_params_t { }

pub enum snd_rawmidi_status_t { }

pub type snd_rawmidi_stream_t = c_uint;
pub const SND_RAWMIDI_STREAM_OUTPUT: c_uint = 0;
pub const SND_RAWMIDI_STREAM_INPUT:  c_uint = 1;
pub const SND_RAWMIDI_STREAM_LAST:   c_uint = 1;

pub enum snd_rawmidi_t { }

pub type snd_rawmidi_type_t = c_uint;
pub const SND_RAWMIDI_TYPE_HW:      c_uint = 0;
pub const SND_RAWMIDI_TYPE_SHM:     c_uint = 1;
pub const SND_RAWMIDI_TYPE_INET:    c_uint = 2;
pub const SND_RAWMIDI_TYPE_VIRTUAL: c_uint = 3;

pub enum snd_timer_id_t { }

pub enum snd_timer_ginfo_t { }

pub enum snd_timer_gparams_t { }

pub enum snd_timer_gstatus_t { }

pub enum snd_timer_info_t { }

pub enum snd_timer_params_t { }

pub enum snd_timer_status_t { }

pub type snd_timer_class_t = c_int;
pub const SND_TIMER_CLASS_NONE:   c_int = -1;
pub const SND_TIMER_CLASS_SLAVE:  c_int = 0;
pub const SND_TIMER_CLASS_GLOBAL: c_int = 1;
pub const SND_TIMER_CLASS_CARD:   c_int = 2;
pub const SND_TIMER_CLASS_PCM:    c_int = 3;
pub const SND_TIMER_CLASS_LAST:   c_int = 3;

pub type snd_timer_slave_class_t = c_uint;
pub const SND_TIMER_SCLASS_NONE:          c_uint = 0;
pub const SND_TIMER_SCLASS_APPLICATION:   c_uint = 1;
pub const SND_TIMER_SCLASS_SEQUENCER:     c_uint = 2;
pub const SND_TIMER_SCLASS_OSS_SEQUENCER: c_uint = 3;
pub const SND_TIMER_SCLASS_LAST:          c_uint = 3;

pub type snd_timer_event_t = c_uint;
pub const SND_TIMER_EVENT_RESOLUTION: c_uint = 0;
pub const SND_TIMER_EVENT_TICK:       c_uint = 1;
pub const SND_TIMER_EVENT_START:      c_uint = 2;
pub const SND_TIMER_EVENT_STOP:       c_uint = 3;
pub const SND_TIMER_EVENT_CONTINUE:   c_uint = 4;
pub const SND_TIMER_EVENT_PAUSE:      c_uint = 5;
pub const SND_TIMER_EVENT_EARLY:      c_uint = 6;
pub const SND_TIMER_EVENT_SUSPEND:    c_uint = 7;
pub const SND_TIMER_EVENT_RESUME:     c_uint = 8;
pub const SND_TIMER_EVENT_MSTART:     c_uint = 12;
pub const SND_TIMER_EVENT_MSTOP:      c_uint = 13;
pub const SND_TIMER_EVENT_MCONTINUE:  c_uint = 14;
pub const SND_TIMER_EVENT_MPAUSE:     c_uint = 15;
pub const SND_TIMER_EVENT_MSUSPEND:   c_uint = 17;
pub const SND_TIMER_EVENT_MRESUME:    c_uint = 18;

#[repr(C)]
pub struct snd_timer_read_t {
    pub resolution: c_uint,
    pub ticks: c_uint,
}

#[repr(C)]
pub struct snd_timer_tread_t {
    pub event: snd_timer_event_t,
    pub tstamp: snd_htimestamp_t,
    pub val: c_uint,
}

pub type snd_timer_type_t = c_uint;
pub const SND_TIMER_TYPE_HW:   c_uint = 0;
pub const SND_TIMER_TYPE_SHM:  c_uint = 1;
pub const SND_TIMER_TYPE_INET: c_uint = 2;

pub enum snd_timer_query_t { }

pub enum snd_timer_t { }

pub enum snd_hwdep_info_t { }

pub enum snd_hwdep_dsp_status_t { }

pub enum snd_hwdep_dsp_image_t { }

pub type snd_hwdep_iface_t = c_uint;
pub const SND_HWDEP_IFACE_OPL2:           c_uint = 0;
pub const SND_HWDEP_IFACE_OPL3:           c_uint = 1;
pub const SND_HWDEP_IFACE_OPL4:           c_uint = 2;
pub const SND_HWDEP_IFACE_SB16CSP:        c_uint = 3;
pub const SND_HWDEP_IFACE_EMU10K1:        c_uint = 4;
pub const SND_HWDEP_IFACE_YSS225:         c_uint = 5;
pub const SND_HWDEP_IFACE_ICS2115:        c_uint = 6;
pub const SND_HWDEP_IFACE_SSCAPE:         c_uint = 7;
pub const SND_HWDEP_IFACE_VX:             c_uint = 8;
pub const SND_HWDEP_IFACE_MIXART:         c_uint = 9;
pub const SND_HWDEP_IFACE_USX2Y:          c_uint = 10;
pub const SND_HWDEP_IFACE_EMUX_WAVETABLE: c_uint = 11;
pub const SND_HWDEP_IFACE_BLUETOOTH:      c_uint = 12;
pub const SND_HWDEP_IFACE_USX2Y_PCM:      c_uint = 13;
pub const SND_HWDEP_IFACE_PCXHR:          c_uint = 14;
pub const SND_HWDEP_IFACE_SB_RC:          c_uint = 15;
pub const SND_HWDEP_IFACE_LAST:           c_uint = 15;

pub type snd_hwdep_type_t = c_uint;
pub const SND_HWDEP_TYPE_HW:   c_uint = 0;
pub const SND_HWDEP_TYPE_SHM:  c_uint = 1;
pub const SND_HWDEP_TYPE_INET: c_uint = 2;

pub enum snd_hwdep_t { }

#[repr(C)]
pub struct snd_aes_iec958_t {
    pub status: [c_uchar; 24us],
    pub subcode: [c_uchar; 147us],
    pub pad: c_uchar,
    pub dig_subframe: [c_uchar; 4us],
}

pub enum snd_ctl_card_info_t { }

pub enum snd_ctl_elem_id_t { }

pub enum snd_ctl_elem_list_t { }

pub enum snd_ctl_elem_info_t { }

pub enum snd_ctl_elem_value_t { }

pub enum snd_ctl_event_t { }

pub type snd_ctl_elem_type_t = c_uint;
pub const SND_CTL_ELEM_TYPE_NONE:       c_uint = 0;
pub const SND_CTL_ELEM_TYPE_BOOLEAN:    c_uint = 1;
pub const SND_CTL_ELEM_TYPE_INTEGER:    c_uint = 2;
pub const SND_CTL_ELEM_TYPE_ENUMERATED: c_uint = 3;
pub const SND_CTL_ELEM_TYPE_BYTES:      c_uint = 4;
pub const SND_CTL_ELEM_TYPE_IEC958:     c_uint = 5;
pub const SND_CTL_ELEM_TYPE_INTEGER64:  c_uint = 6;
pub const SND_CTL_ELEM_TYPE_LAST:       c_uint = 6;

pub type snd_ctl_elem_iface_t = c_uint;
pub const SND_CTL_ELEM_IFACE_CARD:      c_uint = 0;
pub const SND_CTL_ELEM_IFACE_HWDEP:     c_uint = 1;
pub const SND_CTL_ELEM_IFACE_MIXER:     c_uint = 2;
pub const SND_CTL_ELEM_IFACE_PCM:       c_uint = 3;
pub const SND_CTL_ELEM_IFACE_RAWMIDI:   c_uint = 4;
pub const SND_CTL_ELEM_IFACE_TIMER:     c_uint = 5;
pub const SND_CTL_ELEM_IFACE_SEQUENCER: c_uint = 6;
pub const SND_CTL_ELEM_IFACE_LAST:      c_uint = 6;

pub type snd_ctl_event_type_t = c_uint;
pub const SND_CTL_EVENT_ELEM: c_uint = 0;
pub const SND_CTL_EVENT_LAST: c_uint = 0;

pub type snd_ctl_type_t = c_uint;
pub const SND_CTL_TYPE_HW:   c_uint = 0;
pub const SND_CTL_TYPE_SHM:  c_uint = 1;
pub const SND_CTL_TYPE_INET: c_uint = 2;
pub const SND_CTL_TYPE_EXT:  c_uint = 3;

pub enum snd_ctl_t { }

pub enum snd_sctl_t { }

pub enum snd_hctl_elem_t { }

pub enum snd_hctl_t { }

pub type snd_hctl_compare_t = Option<extern fn(arg1: *const snd_hctl_elem_t,
                                               arg2: *const snd_hctl_elem_t) -> c_int>;
pub type snd_hctl_callback_t = Option<extern fn(arg1: *mut snd_hctl_t, arg2: c_uint,
                                                arg3: *mut snd_hctl_elem_t) -> c_int>;
pub type snd_hctl_elem_callback_t = Option<extern fn(arg1: *mut snd_hctl_elem_t,
                                                     arg2: c_uint) -> c_int>;

pub enum snd_mixer_t { }

pub enum snd_mixer_class_t { }

pub enum snd_mixer_elem_t { }

pub type snd_mixer_callback_t = Option<extern fn(arg1: *mut snd_mixer_t, arg2: c_uint,
                                                 arg3: *mut snd_mixer_elem_t) -> c_int>;
pub type snd_mixer_elem_callback_t = Option<extern fn(arg1: *mut snd_mixer_elem_t,
                                                      arg2: c_uint) -> c_int>;
pub type snd_mixer_compare_t = Option<extern fn(arg1: *const snd_mixer_elem_t,
                                                arg2: *const snd_mixer_elem_t) -> c_int>;
pub type snd_mixer_event_t = Option<extern fn(arg1: *mut snd_mixer_class_t,
                                              arg2: c_uint, arg3: *mut snd_hctl_elem_t,
                                              arg4: *mut snd_mixer_elem_t) -> c_int>;

pub type snd_mixer_elem_type_t = c_uint;
pub const SND_MIXER_ELEM_SIMPLE: c_uint = 0;
pub const SND_MIXER_ELEM_LAST:   c_uint = 0;

pub type snd_mixer_selem_channel_id_t = c_int;
pub const SND_MIXER_SCHN_UNKNOWN:      c_int = -1;
pub const SND_MIXER_SCHN_FRONT_LEFT:   c_int = 0;
pub const SND_MIXER_SCHN_FRONT_RIGHT:  c_int = 1;
pub const SND_MIXER_SCHN_REAR_LEFT:    c_int = 2;
pub const SND_MIXER_SCHN_REAR_RIGHT:   c_int = 3;
pub const SND_MIXER_SCHN_FRONT_CENTER: c_int = 4;
pub const SND_MIXER_SCHN_WOOFER:       c_int = 5;
pub const SND_MIXER_SCHN_SIDE_LEFT:    c_int = 6;
pub const SND_MIXER_SCHN_SIDE_RIGHT:   c_int = 7;
pub const SND_MIXER_SCHN_REAR_CENTER:  c_int = 8;
pub const SND_MIXER_SCHN_LAST:         c_int = 31;
pub const SND_MIXER_SCHN_MONO:         c_int = 0;

pub type snd_mixer_selem_regopt_abstract = c_uint;
pub static SND_MIXER_SABSTRACT_NONE:  c_uint = 0;
pub static SND_MIXER_SABSTRACT_BASIC: c_uint = 1;

#[repr(C)]
pub struct snd_mixer_selem_regopt {
    pub ver: c_int,
    pub _abstract: snd_mixer_selem_regopt_abstract,
    pub device: *const c_char,
    pub playback_pcm: *mut snd_pcm_t,
    pub capture_pcm: *mut snd_pcm_t,
}

pub enum snd_mixer_selem_id_t { }

pub type snd_seq_event_type_t = c_uchar;

pub const SND_SEQ_EVENT_SYSTEM:            c_uint = 0;
pub const SND_SEQ_EVENT_RESULT:            c_uint = 1;
pub const SND_SEQ_EVENT_NOTE:              c_uint = 5;
pub const SND_SEQ_EVENT_NOTEON:            c_uint = 6;
pub const SND_SEQ_EVENT_NOTEOFF:           c_uint = 7;
pub const SND_SEQ_EVENT_KEYPRESS:          c_uint = 8;
pub const SND_SEQ_EVENT_CONTROLLER:        c_uint = 10;
pub const SND_SEQ_EVENT_PGMCHANGE:         c_uint = 11;
pub const SND_SEQ_EVENT_CHANPRESS:         c_uint = 12;
pub const SND_SEQ_EVENT_PITCHBEND:         c_uint = 13;
pub const SND_SEQ_EVENT_CONTROL14:         c_uint = 14;
pub const SND_SEQ_EVENT_NONREGPARAM:       c_uint = 15;
pub const SND_SEQ_EVENT_REGPARAM:          c_uint = 16;
pub const SND_SEQ_EVENT_SONGPOS:           c_uint = 20;
pub const SND_SEQ_EVENT_SONGSEL:           c_uint = 21;
pub const SND_SEQ_EVENT_QFRAME:            c_uint = 22;
pub const SND_SEQ_EVENT_TIMESIGN:          c_uint = 23;
pub const SND_SEQ_EVENT_KEYSIGN:           c_uint = 24;
pub const SND_SEQ_EVENT_START:             c_uint = 30;
pub const SND_SEQ_EVENT_CONTINUE:          c_uint = 31;
pub const SND_SEQ_EVENT_STOP:              c_uint = 32;
pub const SND_SEQ_EVENT_SETPOS_TICK:       c_uint = 33;
pub const SND_SEQ_EVENT_SETPOS_TIME:       c_uint = 34;
pub const SND_SEQ_EVENT_TEMPO:             c_uint = 35;
pub const SND_SEQ_EVENT_CLOCK:             c_uint = 36;
pub const SND_SEQ_EVENT_TICK:              c_uint = 37;
pub const SND_SEQ_EVENT_QUEUE_SKEW:        c_uint = 38;
pub const SND_SEQ_EVENT_SYNC_POS:          c_uint = 39;
pub const SND_SEQ_EVENT_TUNE_REQUEST:      c_uint = 40;
pub const SND_SEQ_EVENT_RESET:             c_uint = 41;
pub const SND_SEQ_EVENT_SENSING:           c_uint = 42;
pub const SND_SEQ_EVENT_ECHO:              c_uint = 50;
pub const SND_SEQ_EVENT_OSS:               c_uint = 51;
pub const SND_SEQ_EVENT_CLIENT_START:      c_uint = 60;
pub const SND_SEQ_EVENT_CLIENT_EXIT:       c_uint = 61;
pub const SND_SEQ_EVENT_CLIENT_CHANGE:     c_uint = 62;
pub const SND_SEQ_EVENT_PORT_START:        c_uint = 63;
pub const SND_SEQ_EVENT_PORT_EXIT:         c_uint = 64;
pub const SND_SEQ_EVENT_PORT_CHANGE:       c_uint = 65;
pub const SND_SEQ_EVENT_PORT_SUBSCRIBED:   c_uint = 66;
pub const SND_SEQ_EVENT_PORT_UNSUBSCRIBED: c_uint = 67;
pub const SND_SEQ_EVENT_USR0:              c_uint = 90;
pub const SND_SEQ_EVENT_USR1:              c_uint = 91;
pub const SND_SEQ_EVENT_USR2:              c_uint = 92;
pub const SND_SEQ_EVENT_USR3:              c_uint = 93;
pub const SND_SEQ_EVENT_USR4:              c_uint = 94;
pub const SND_SEQ_EVENT_USR5:              c_uint = 95;
pub const SND_SEQ_EVENT_USR6:              c_uint = 96;
pub const SND_SEQ_EVENT_USR7:              c_uint = 97;
pub const SND_SEQ_EVENT_USR8:              c_uint = 98;
pub const SND_SEQ_EVENT_USR9:              c_uint = 99;
pub const SND_SEQ_EVENT_SYSEX:             c_uint = 130;
pub const SND_SEQ_EVENT_BOUNCE:            c_uint = 131;
pub const SND_SEQ_EVENT_USR_VAR0:          c_uint = 135;
pub const SND_SEQ_EVENT_USR_VAR1:          c_uint = 136;
pub const SND_SEQ_EVENT_USR_VAR2:          c_uint = 137;
pub const SND_SEQ_EVENT_USR_VAR3:          c_uint = 138;
pub const SND_SEQ_EVENT_USR_VAR4:          c_uint = 139;
pub const SND_SEQ_EVENT_NONE:              c_uint = 255;

#[repr(C)]
pub struct snd_seq_addr_t {
    pub client: c_uchar,
    pub port:   c_uchar,
}

#[repr(C)]
pub struct snd_seq_connect_t {
    pub sender: snd_seq_addr_t,
    pub dest:   snd_seq_addr_t,
}

#[repr(C)]
pub struct snd_seq_real_time_t {
    pub tv_sec:  c_uint,
    pub tv_nsec: c_uint,
}

pub type snd_seq_tick_time_t = c_uint;

#[repr(C)]
pub struct snd_seq_timestamp_t {
    pub data: [u32; 2us],
}
impl snd_seq_timestamp_t {
    pub fn tick(&mut self) -> *mut snd_seq_tick_time_t {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn time(&mut self) -> *mut snd_seq_real_time_t {
        unsafe { ::std::mem::transmute(self) }
    }
}

#[repr(C)]
pub struct snd_seq_ev_note_t {
    pub channel: c_uchar,
    pub note: c_uchar,
    pub velocity: c_uchar,
    pub off_velocity: c_uchar,
    pub duration: c_uint,
}

#[repr(C)]
pub struct snd_seq_ev_ctrl_t {
    pub channel: c_uchar,
    pub unused: [c_uchar; 3us],
    pub param: c_uint,
    pub value: c_int,
}

#[repr(C)]
pub struct snd_seq_ev_raw8_t {
    pub d: [c_uchar; 12us],
}

#[repr(C)]
pub struct snd_seq_ev_raw32_t {
    pub d: [c_uint; 3us],
}

#[repr(C)]
pub struct snd_seq_ev_ext_t {
    pub len: c_uint,
    pub ptr: *mut c_void,
}

#[repr(C)]
pub struct snd_seq_result_t {
    pub event: c_int,
    pub result: c_int,
}

#[repr(C)]
pub struct snd_seq_queue_skew_t {
    pub value: c_uint,
    pub base: c_uint,
}

#[repr(C)]
pub struct snd_seq_ev_queue_control_t {
    pub queue: c_uchar,
    pub unused: [c_uchar; 3us],
    pub param: Union_Unnamed9,
}

#[repr(C)]
pub struct Union_Unnamed9 {
    pub data: [u32; 2us],
}
impl Union_Unnamed9 {
    pub fn value(&mut self) -> *mut c_int {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn time(&mut self) -> *mut snd_seq_timestamp_t {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn position(&mut self) -> *mut c_uint {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn skew(&mut self) -> *mut snd_seq_queue_skew_t {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn d32(&mut self) -> *mut [c_uint; 2us] {
        unsafe { ::std::mem::transmute(self) }
    }
    pub fn d8(&mut self) -> *mut [c_uchar; 8us] {
        unsafe { ::std::mem::transmute(self) }
    }
}

#[repr(C)]
pub struct snd_seq_event_t {
    pub _type:  snd_seq_event_type_t,
    pub flags:  c_uchar,
    pub tag:    c_uchar,
    pub queue:  c_uchar,
    pub time:   snd_seq_timestamp_t,
    pub source: snd_seq_addr_t,
    pub dest:   snd_seq_addr_t,
    pub data:   Union_Unnamed10,
}

#[repr(C)]
pub struct Union_Unnamed10 {
    pub data: [u32; 3us],
}
impl Union_Unnamed10 {
    pub fn note(&mut self) -> *mut snd_seq_ev_note_t {
        unsafe { mem::transmute(self) }
    }
    pub fn control(&mut self) -> *mut snd_seq_ev_ctrl_t {
        unsafe { mem::transmute(self) }
    }
    pub fn raw8(&mut self) -> *mut snd_seq_ev_raw8_t {
        unsafe { mem::transmute(self) }
    }
    pub fn raw32(&mut self) -> *mut snd_seq_ev_raw32_t {
        unsafe { mem::transmute(self) }
    }
    pub fn ext(&mut self) -> *mut snd_seq_ev_ext_t {
        unsafe { mem::transmute(self) }
    }
    pub fn queue(&mut self) -> *mut snd_seq_ev_queue_control_t {
        unsafe { mem::transmute(self) }
    }
    pub fn time(&mut self) -> *mut snd_seq_timestamp_t {
        unsafe { mem::transmute(self) }
    }
    pub fn addr(&mut self) -> *mut snd_seq_addr_t {
        unsafe { mem::transmute(self) }
    }
    pub fn connect(&mut self) -> *mut snd_seq_connect_t {
        unsafe { mem::transmute(self) }
    }
    pub fn result(&mut self) -> *mut snd_seq_result_t {
        unsafe { mem::transmute(self) }
    }
}

pub enum snd_seq_t { }

pub type snd_seq_type_t = c_uint;
pub const SND_SEQ_TYPE_HW:   c_uint = 0;
pub const SND_SEQ_TYPE_SHM:  c_uint = 1;
pub const SND_SEQ_TYPE_INET: c_uint = 2;

pub enum snd_seq_system_info_t { }

pub enum snd_seq_client_info_t { }

pub type snd_seq_client_type_t = c_uint;
pub const SND_SEQ_USER_CLIENT:   c_uint = 1;
pub const SND_SEQ_KERNEL_CLIENT: c_uint = 2;

pub enum snd_seq_client_pool_t { }

pub enum snd_seq_port_info_t { }

pub enum snd_seq_port_subscribe_t { }

pub enum snd_seq_query_subscribe_t { }

pub type snd_seq_query_subs_type_t = c_uint;
pub const SND_SEQ_QUERY_SUBS_READ:  c_uint = 0;
pub const SND_SEQ_QUERY_SUBS_WRITE: c_uint = 1;

pub enum snd_seq_queue_info_t { }

pub enum snd_seq_queue_status_t { }

pub enum snd_seq_queue_tempo_t { }

pub enum snd_seq_queue_timer_t { }

pub type snd_seq_queue_timer_type_t = c_uint;
pub const SND_SEQ_TIMER_ALSA:       c_uint = 0;
pub const SND_SEQ_TIMER_MIDI_CLOCK: c_uint = 1;
pub const SND_SEQ_TIMER_MIDI_TICK:  c_uint = 2;

pub enum snd_seq_remove_events_t { }

pub const SND_SEQ_EVFLG_RESULT:     c_uint = 0;
pub const SND_SEQ_EVFLG_NOTE:       c_uint = 1;
pub const SND_SEQ_EVFLG_CONTROL:    c_uint = 2;
pub const SND_SEQ_EVFLG_QUEUE:      c_uint = 3;
pub const SND_SEQ_EVFLG_SYSTEM:     c_uint = 4;
pub const SND_SEQ_EVFLG_MESSAGE:    c_uint = 5;
pub const SND_SEQ_EVFLG_CONNECTION: c_uint = 6;
pub const SND_SEQ_EVFLG_SAMPLE:     c_uint = 7;
pub const SND_SEQ_EVFLG_USERS:      c_uint = 8;
pub const SND_SEQ_EVFLG_INSTR:      c_uint = 9;
pub const SND_SEQ_EVFLG_QUOTE:      c_uint = 10;
pub const SND_SEQ_EVFLG_NONE:       c_uint = 11;
pub const SND_SEQ_EVFLG_RAW:        c_uint = 12;
pub const SND_SEQ_EVFLG_FIXED:      c_uint = 13;
pub const SND_SEQ_EVFLG_VARIABLE:   c_uint = 14;
pub const SND_SEQ_EVFLG_VARUSR:     c_uint = 15;

pub const SND_SEQ_EVFLG_NOTE_ONEARG: c_uint = 0;
pub const SND_SEQ_EVFLG_NOTE_TWOARG: c_uint = 1;

pub const SND_SEQ_EVFLG_QUEUE_NOARG: c_uint = 0;
pub const SND_SEQ_EVFLG_QUEUE_TICK:  c_uint = 1;
pub const SND_SEQ_EVFLG_QUEUE_TIME:  c_uint = 2;
pub const SND_SEQ_EVFLG_QUEUE_VALUE: c_uint = 3;

pub enum snd_midi_event_t { }

extern "C" {
    pub static mut snd_dlsym_start: *mut snd_dlsym_link;
    pub static mut snd_lib_error: snd_lib_error_handler_t;
    pub static mut snd_config: *mut snd_config_t;
    pub static mut snd_seq_event_types: *const c_uint;
    pub fn snd_asoundlib_version() -> *const c_char;
    pub fn snd_dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    pub fn snd_dlsym(handle: *mut c_void, name: *const c_char, version: *const c_char) -> *mut c_void;
    pub fn snd_dlclose(handle: *mut c_void) -> c_int;
    pub fn snd_async_add_handler(handler: *mut *mut snd_async_handler_t, fd: c_int, callback: snd_async_callback_t, private_data: *mut c_void) -> c_int;
    pub fn snd_async_del_handler(handler: *mut snd_async_handler_t) -> c_int;
    pub fn snd_async_handler_get_fd(handler: *mut snd_async_handler_t) -> c_int;
    pub fn snd_async_handler_get_signo(handler: *mut snd_async_handler_t) -> c_int;
    pub fn snd_async_handler_get_callback_private(handler: *mut snd_async_handler_t) -> *mut c_void;
    pub fn snd_shm_area_create(shmid: c_int, ptr: *mut c_void) -> *mut snd_shm_area;
    pub fn snd_shm_area_share(area: *mut snd_shm_area) -> *mut snd_shm_area;
    pub fn snd_shm_area_destroy(area: *mut snd_shm_area) -> c_int;
    pub fn snd_user_file(file: *const c_char, result: *mut *mut c_char) -> c_int;
    pub fn snd_input_stdio_open(inputp: *mut *mut snd_input_t, file: *const c_char, mode: *const c_char) -> c_int;
    pub fn snd_input_stdio_attach(inputp: *mut *mut snd_input_t, fp: *mut FILE, _close: c_int) -> c_int;
    pub fn snd_input_buffer_open(inputp: *mut *mut snd_input_t, buffer: *const c_char, size: ssize_t) -> c_int;
    pub fn snd_input_close(input: *mut snd_input_t) -> c_int;
    pub fn snd_input_scanf(input: *mut snd_input_t, format: *const c_char, ...) -> c_int;
    pub fn snd_input_gets(input: *mut snd_input_t, str: *mut c_char, size: size_t) -> *mut c_char;
    pub fn snd_input_getc(input: *mut snd_input_t) -> c_int;
    pub fn snd_input_ungetc(input: *mut snd_input_t, c: c_int) -> c_int;
    pub fn snd_output_stdio_open(outputp: *mut *mut snd_output_t, file: *const c_char, mode: *const c_char) -> c_int;
    pub fn snd_output_stdio_attach(outputp: *mut *mut snd_output_t, fp: *mut FILE, _close: c_int) -> c_int;
    pub fn snd_output_buffer_open(outputp: *mut *mut snd_output_t) -> c_int;
    pub fn snd_output_buffer_string(output: *mut snd_output_t, buf: *mut *mut c_char) -> size_t;
    pub fn snd_output_close(output: *mut snd_output_t) -> c_int;
    pub fn snd_output_printf(output: *mut snd_output_t, format: *const c_char, ...) -> c_int;
    //pub fn snd_output_vprintf(output: *mut snd_output_t, format: *const c_char, args: va_list) -> c_int;
    pub fn snd_output_puts(output: *mut snd_output_t, str: *const c_char) -> c_int;
    pub fn snd_output_putc(output: *mut snd_output_t, c: c_int) -> c_int;
    pub fn snd_output_flush(output: *mut snd_output_t) -> c_int;
    pub fn snd_strerror(errnum: c_int) -> *const c_char;
    pub fn snd_lib_error_set_handler(handler: snd_lib_error_handler_t) -> c_int;
    //pub fn snd_lib_error_set_local(func: snd_local_error_handler_t) -> snd_local_error_handler_t;
    pub fn snd_config_top(config: *mut *mut snd_config_t) -> c_int;
    pub fn snd_config_load(config: *mut snd_config_t, _in: *mut snd_input_t) -> c_int;
    pub fn snd_config_load_override(config: *mut snd_config_t, _in: *mut snd_input_t) -> c_int;
    pub fn snd_config_save(config: *mut snd_config_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_config_update() -> c_int;
    pub fn snd_config_update_r(top: *mut *mut snd_config_t, update: *mut *mut snd_config_update_t, path: *const c_char) -> c_int;
    pub fn snd_config_update_free(update: *mut snd_config_update_t) -> c_int;
    pub fn snd_config_update_free_global() -> c_int;
    pub fn snd_config_search(config: *mut snd_config_t, key: *const c_char, result: *mut *mut snd_config_t) -> c_int;
    pub fn snd_config_searchv(config: *mut snd_config_t, result: *mut *mut snd_config_t, ...) -> c_int;
    pub fn snd_config_search_definition(config: *mut snd_config_t, base: *const c_char, key: *const c_char, result: *mut *mut snd_config_t) -> c_int;
    pub fn snd_config_expand(config: *mut snd_config_t, root: *mut snd_config_t, args: *const c_char, private_data: *mut snd_config_t, result: *mut *mut snd_config_t) -> c_int;
    pub fn snd_config_evaluate(config: *mut snd_config_t, root: *mut snd_config_t, private_data: *mut snd_config_t, result: *mut *mut snd_config_t) -> c_int;
    pub fn snd_config_add(config: *mut snd_config_t, leaf: *mut snd_config_t) -> c_int;
    pub fn snd_config_delete(config: *mut snd_config_t) -> c_int;
    pub fn snd_config_delete_compound_members(config: *const snd_config_t) -> c_int;
    pub fn snd_config_copy(dst: *mut *mut snd_config_t, src: *mut snd_config_t) -> c_int;
    pub fn snd_config_make(config: *mut *mut snd_config_t, key: *const c_char, _type: snd_config_type_t) -> c_int;
    pub fn snd_config_make_integer(config: *mut *mut snd_config_t, key: *const c_char) -> c_int;
    pub fn snd_config_make_integer64(config: *mut *mut snd_config_t, key: *const c_char) -> c_int;
    pub fn snd_config_make_real(config: *mut *mut snd_config_t, key: *const c_char) -> c_int;
    pub fn snd_config_make_string(config: *mut *mut snd_config_t, key: *const c_char) -> c_int;
    pub fn snd_config_make_pointer(config: *mut *mut snd_config_t, key: *const c_char) -> c_int;
    pub fn snd_config_make_compound(config: *mut *mut snd_config_t, key: *const c_char, join: c_int) -> c_int;
    pub fn snd_config_imake_integer(config: *mut *mut snd_config_t, key: *const c_char, value: c_long) -> c_int;
    pub fn snd_config_imake_integer64(config: *mut *mut snd_config_t, key: *const c_char, value: c_longlong) -> c_int;
    pub fn snd_config_imake_real(config: *mut *mut snd_config_t, key: *const c_char, value: c_double) -> c_int;
    pub fn snd_config_imake_string(config: *mut *mut snd_config_t, key: *const c_char, ascii: *const c_char) -> c_int;
    pub fn snd_config_imake_pointer(config: *mut *mut snd_config_t, key: *const c_char, ptr: *const c_void) -> c_int;
    pub fn snd_config_get_type(config: *const snd_config_t) -> snd_config_type_t;
    pub fn snd_config_set_id(config: *mut snd_config_t, id: *const c_char) -> c_int;
    pub fn snd_config_set_integer(config: *mut snd_config_t, value: c_long) -> c_int;
    pub fn snd_config_set_integer64(config: *mut snd_config_t, value: c_longlong) -> c_int;
    pub fn snd_config_set_real(config: *mut snd_config_t, value: c_double) -> c_int;
    pub fn snd_config_set_string(config: *mut snd_config_t, value: *const c_char) -> c_int;
    pub fn snd_config_set_ascii(config: *mut snd_config_t, ascii: *const c_char) -> c_int;
    pub fn snd_config_set_pointer(config: *mut snd_config_t, ptr: *const c_void) -> c_int;
    pub fn snd_config_get_id(config: *const snd_config_t, value: *mut *const c_char) -> c_int;
    pub fn snd_config_get_integer(config: *const snd_config_t, value: *mut c_long) -> c_int;
    pub fn snd_config_get_integer64(config: *const snd_config_t, value: *mut c_longlong) -> c_int;
    pub fn snd_config_get_real(config: *const snd_config_t, value: *mut c_double) -> c_int;
    pub fn snd_config_get_ireal(config: *const snd_config_t, value: *mut c_double) -> c_int;
    pub fn snd_config_get_string(config: *const snd_config_t, value: *mut *const c_char) -> c_int;
    pub fn snd_config_get_ascii(config: *const snd_config_t, value: *mut *mut c_char) -> c_int;
    pub fn snd_config_get_pointer(config: *const snd_config_t, value: *mut *const c_void) -> c_int;
    pub fn snd_config_test_id(config: *const snd_config_t, id: *const c_char) -> c_int;
    pub fn snd_config_iterator_first(node: *const snd_config_t) -> snd_config_iterator_t;
    pub fn snd_config_iterator_next(iterator: snd_config_iterator_t) -> snd_config_iterator_t;
    pub fn snd_config_iterator_end(node: *const snd_config_t) -> snd_config_iterator_t;
    pub fn snd_config_iterator_entry(iterator: snd_config_iterator_t) -> *mut snd_config_t;
    pub fn snd_config_get_bool_ascii(ascii: *const c_char) -> c_int;
    pub fn snd_config_get_bool(conf: *const snd_config_t) -> c_int;
    pub fn snd_config_get_ctl_iface_ascii(ascii: *const c_char) -> c_int;
    pub fn snd_config_get_ctl_iface(conf: *const snd_config_t) -> c_int;
    pub fn snd_names_list(iface: *const c_char, list: *mut *mut snd_devname_t) -> c_int;
    pub fn snd_names_list_free(list: *mut snd_devname_t);
    pub fn snd_pcm_open(pcm: *mut *mut snd_pcm_t, name: *const c_char, stream: snd_pcm_stream_t, mode: c_int) -> c_int;
    pub fn snd_pcm_open_lconf(pcm: *mut *mut snd_pcm_t, name: *const c_char, stream: snd_pcm_stream_t, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_pcm_open_fallback(pcm: *mut *mut snd_pcm_t, root: *mut snd_config_t, name: *const c_char, orig_name: *const c_char, stream: snd_pcm_stream_t, mode: c_int) -> c_int;
    pub fn snd_pcm_close(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_name(pcm: *mut snd_pcm_t) -> *const c_char;
    pub fn snd_pcm_type(pcm: *mut snd_pcm_t) -> snd_pcm_type_t;
    pub fn snd_pcm_stream(pcm: *mut snd_pcm_t) -> snd_pcm_stream_t;
    pub fn snd_pcm_poll_descriptors_count(pcm: *mut snd_pcm_t) -> c_int;
    //pub fn snd_pcm_poll_descriptors(pcm: *mut snd_pcm_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_pcm_poll_descriptors_revents(pcm: *mut snd_pcm_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_pcm_nonblock(pcm: *mut snd_pcm_t, nonblock: c_int) -> c_int;
    pub fn snd_async_add_pcm_handler(handler: *mut *mut snd_async_handler_t, pcm: *mut snd_pcm_t, callback: snd_async_callback_t, private_data: *mut c_void) -> c_int;
    pub fn snd_async_handler_get_pcm(handler: *mut snd_async_handler_t) -> *mut snd_pcm_t;
    pub fn snd_pcm_info(pcm: *mut snd_pcm_t, info: *mut snd_pcm_info_t) -> c_int;
    pub fn snd_pcm_hw_params_current(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_free(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_sw_params_current(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t) -> c_int;
    pub fn snd_pcm_sw_params(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t) -> c_int;
    pub fn snd_pcm_prepare(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_reset(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_status(pcm: *mut snd_pcm_t, status: *mut snd_pcm_status_t) -> c_int;
    pub fn snd_pcm_start(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_drop(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_drain(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_pause(pcm: *mut snd_pcm_t, enable: c_int) -> c_int;
    pub fn snd_pcm_state(pcm: *mut snd_pcm_t) -> snd_pcm_state_t;
    pub fn snd_pcm_hwsync(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_delay(pcm: *mut snd_pcm_t, delayp: *mut snd_pcm_sframes_t) -> c_int;
    pub fn snd_pcm_resume(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_htimestamp(pcm: *mut snd_pcm_t, avail: *mut snd_pcm_uframes_t, tstamp: *mut snd_htimestamp_t) -> c_int;
    pub fn snd_pcm_avail(pcm: *mut snd_pcm_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_avail_update(pcm: *mut snd_pcm_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_avail_delay(pcm: *mut snd_pcm_t, availp: *mut snd_pcm_sframes_t, delayp: *mut snd_pcm_sframes_t) -> c_int;
    pub fn snd_pcm_rewindable(pcm: *mut snd_pcm_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_rewind(pcm: *mut snd_pcm_t, frames: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_forwardable(pcm: *mut snd_pcm_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_forward(pcm: *mut snd_pcm_t, frames: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_writei(pcm: *mut snd_pcm_t, buffer: *const c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_readi(pcm: *mut snd_pcm_t, buffer: *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_writen(pcm: *mut snd_pcm_t, bufs: *mut *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_readn(pcm: *mut snd_pcm_t, bufs: *mut *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_wait(pcm: *mut snd_pcm_t, timeout: c_int) -> c_int;
    pub fn snd_pcm_link(pcm1: *mut snd_pcm_t, pcm2: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_unlink(pcm: *mut snd_pcm_t) -> c_int;
    pub fn snd_pcm_query_chmaps(pcm: *mut snd_pcm_t) -> *mut *mut snd_pcm_chmap_query_t;
    pub fn snd_pcm_query_chmaps_from_hw(card: c_int, dev: c_int, subdev: c_int, stream: snd_pcm_stream_t) -> *mut *mut snd_pcm_chmap_query_t;
    pub fn snd_pcm_free_chmaps(maps: *mut *mut snd_pcm_chmap_query_t);
    pub fn snd_pcm_get_chmap(pcm: *mut snd_pcm_t) -> *mut snd_pcm_chmap_t;
    pub fn snd_pcm_set_chmap(pcm: *mut snd_pcm_t, map: *const snd_pcm_chmap_t) -> c_int;
    pub fn snd_pcm_chmap_type_name(val: snd_pcm_chmap_type) -> *const c_char;
    pub fn snd_pcm_chmap_name(val: snd_pcm_chmap_position) -> *const c_char;
    pub fn snd_pcm_chmap_long_name(val: snd_pcm_chmap_position) -> *const c_char;
    pub fn snd_pcm_chmap_print(map: *const snd_pcm_chmap_t, maxlen: size_t, buf: *mut c_char) -> c_int;
    pub fn snd_pcm_chmap_from_string(str: *const c_char) -> c_uint;
    pub fn snd_pcm_chmap_parse_string(str: *const c_char) -> *mut snd_pcm_chmap_t;
    pub fn snd_pcm_recover(pcm: *mut snd_pcm_t, err: c_int, silent: c_int) -> c_int;
    pub fn snd_pcm_set_params(pcm: *mut snd_pcm_t, format: snd_pcm_format_t, access: snd_pcm_access_t, channels: c_uint, rate: c_uint, soft_resample: c_int, latency: c_uint) -> c_int;
    pub fn snd_pcm_get_params(pcm: *mut snd_pcm_t, buffer_size: *mut snd_pcm_uframes_t, period_size: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_info_sizeof() -> size_t;
    pub fn snd_pcm_info_malloc(ptr: *mut *mut snd_pcm_info_t) -> c_int;
    pub fn snd_pcm_info_free(obj: *mut snd_pcm_info_t);
    pub fn snd_pcm_info_copy(dst: *mut snd_pcm_info_t, src: *const snd_pcm_info_t);
    pub fn snd_pcm_info_get_device(obj: *const snd_pcm_info_t) -> c_uint;
    pub fn snd_pcm_info_get_subdevice(obj: *const snd_pcm_info_t) -> c_uint;
    pub fn snd_pcm_info_get_stream(obj: *const snd_pcm_info_t) -> snd_pcm_stream_t;
    pub fn snd_pcm_info_get_card(obj: *const snd_pcm_info_t) -> c_int;
    pub fn snd_pcm_info_get_id(obj: *const snd_pcm_info_t) -> *const c_char;
    pub fn snd_pcm_info_get_name(obj: *const snd_pcm_info_t) -> *const c_char;
    pub fn snd_pcm_info_get_subdevice_name(obj: *const snd_pcm_info_t) -> *const c_char;
    pub fn snd_pcm_info_get_class(obj: *const snd_pcm_info_t) -> snd_pcm_class_t;
    pub fn snd_pcm_info_get_subclass(obj: *const snd_pcm_info_t) -> snd_pcm_subclass_t;
    pub fn snd_pcm_info_get_subdevices_count(obj: *const snd_pcm_info_t) -> c_uint;
    pub fn snd_pcm_info_get_subdevices_avail(obj: *const snd_pcm_info_t) -> c_uint;
    pub fn snd_pcm_info_get_sync(obj: *const snd_pcm_info_t) -> snd_pcm_sync_id_t;
    pub fn snd_pcm_info_set_device(obj: *mut snd_pcm_info_t, val: c_uint);
    pub fn snd_pcm_info_set_subdevice(obj: *mut snd_pcm_info_t, val: c_uint);
    pub fn snd_pcm_info_set_stream(obj: *mut snd_pcm_info_t, val: snd_pcm_stream_t);
    pub fn snd_pcm_hw_params_any(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_mmap_sample_resolution(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_double(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_batch(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_block_transfer(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_monotonic(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_overrange(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_pause(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_resume(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_half_duplex(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_is_joint_duplex(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_sync_start(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_can_disable_period_wakeup(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_supports_audio_wallclock_ts(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_get_rate_numden(params: *const snd_pcm_hw_params_t, rate_num: *mut c_uint, rate_den: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_sbits(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_get_fifo_size(params: *const snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_sizeof() -> size_t;
    pub fn snd_pcm_hw_params_malloc(ptr: *mut *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_free(obj: *mut snd_pcm_hw_params_t);
    pub fn snd_pcm_hw_params_copy(dst: *mut snd_pcm_hw_params_t, src: *const snd_pcm_hw_params_t);
    pub fn snd_pcm_hw_params_get_access(params: *const snd_pcm_hw_params_t, _access: *mut snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_hw_params_test_access(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, _access: snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_hw_params_set_access(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, _access: snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_hw_params_set_access_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, _access: *mut snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_hw_params_set_access_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, _access: *mut snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_hw_params_set_access_mask(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_access_mask_t) -> c_int;
    pub fn snd_pcm_hw_params_get_access_mask(params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_access_mask_t) -> c_int;
    pub fn snd_pcm_hw_params_get_format(params: *const snd_pcm_hw_params_t, val: *mut snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hw_params_test_format(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hw_params_set_format(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hw_params_set_format_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, format: *mut snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hw_params_set_format_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, format: *mut snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hw_params_set_format_mask(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_format_mask_t) -> c_int;
    pub fn snd_pcm_hw_params_get_format_mask(params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_format_mask_t);
    pub fn snd_pcm_hw_params_get_subformat(params: *const snd_pcm_hw_params_t, subformat: *mut snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_hw_params_test_subformat(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, subformat: snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_hw_params_set_subformat(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, subformat: snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_hw_params_set_subformat_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, subformat: *mut snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_hw_params_set_subformat_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, subformat: *mut snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_hw_params_set_subformat_mask(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_subformat_mask_t) -> c_int;
    pub fn snd_pcm_hw_params_get_subformat_mask(params: *mut snd_pcm_hw_params_t, mask: *mut snd_pcm_subformat_mask_t);
    pub fn snd_pcm_hw_params_get_channels(params: *const snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_channels_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_channels_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_test_channels(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, max: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_channels_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_rate(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_rate_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_rate_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_rate(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, mindir: *mut c_int, max: *mut c_uint, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_rate_resample(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_rate_resample(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_export_buffer(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_export_buffer(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_set_period_wakeup(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_period_wakeup(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_period_time(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_period_time_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_period_time_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_period_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, mindir: *mut c_int, max: *mut c_uint, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_time_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_period_size(params: *const snd_pcm_hw_params_t, frames: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_period_size_min(params: *const snd_pcm_hw_params_t, frames: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_period_size_max(params: *const snd_pcm_hw_params_t, frames: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_period_size(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_uframes_t, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_uframes_t, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut snd_pcm_uframes_t, mindir: *mut c_int, max: *mut snd_pcm_uframes_t, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_period_size_integer(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_get_periods(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_periods_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_periods_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_periods(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, mindir: *mut c_int, max: *mut c_uint, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_periods_integer(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_time(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_time_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_time_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_buffer_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, mindir: *mut c_int, max: *mut c_uint, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_time_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_size(params: *const snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_size_min(params: *const snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_get_buffer_size_max(params: *const snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_test_buffer_size(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut snd_pcm_uframes_t, max: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_set_buffer_size_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_hw_params_get_min_align(params: *const snd_pcm_hw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_sizeof() -> size_t;
    pub fn snd_pcm_sw_params_malloc(ptr: *mut *mut snd_pcm_sw_params_t) -> c_int;
    pub fn snd_pcm_sw_params_free(obj: *mut snd_pcm_sw_params_t);
    pub fn snd_pcm_sw_params_copy(dst: *mut snd_pcm_sw_params_t, src: *const snd_pcm_sw_params_t);
    pub fn snd_pcm_sw_params_get_boundary(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_tstamp_mode(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_tstamp_t) -> c_int;
    pub fn snd_pcm_sw_params_get_tstamp_mode(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_tstamp_t) -> c_int;
    pub fn snd_pcm_sw_params_set_avail_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_avail_min(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_period_event(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: c_int) -> c_int;
    pub fn snd_pcm_sw_params_get_period_event(params: *const snd_pcm_sw_params_t, val: *mut c_int) -> c_int;
    pub fn snd_pcm_sw_params_set_start_threshold(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_start_threshold(paramsm: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_stop_threshold(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_stop_threshold(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_silence_threshold(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_silence_threshold(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_silence_size(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_silence_size(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_access_mask_sizeof() -> size_t;
    pub fn snd_pcm_access_mask_malloc(ptr: *mut *mut snd_pcm_access_mask_t) -> c_int;
    pub fn snd_pcm_access_mask_free(obj: *mut snd_pcm_access_mask_t);
    pub fn snd_pcm_access_mask_copy(dst: *mut snd_pcm_access_mask_t, src: *const snd_pcm_access_mask_t);
    pub fn snd_pcm_access_mask_none(mask: *mut snd_pcm_access_mask_t);
    pub fn snd_pcm_access_mask_any(mask: *mut snd_pcm_access_mask_t);
    pub fn snd_pcm_access_mask_test(mask: *const snd_pcm_access_mask_t, val: snd_pcm_access_t) -> c_int;
    pub fn snd_pcm_access_mask_empty(mask: *const snd_pcm_access_mask_t) -> c_int;
    pub fn snd_pcm_access_mask_set(mask: *mut snd_pcm_access_mask_t, val: snd_pcm_access_t);
    pub fn snd_pcm_access_mask_reset(mask: *mut snd_pcm_access_mask_t, val: snd_pcm_access_t);
    pub fn snd_pcm_format_mask_sizeof() -> size_t;
    pub fn snd_pcm_format_mask_malloc(ptr: *mut *mut snd_pcm_format_mask_t) -> c_int;
    pub fn snd_pcm_format_mask_free(obj: *mut snd_pcm_format_mask_t);
    pub fn snd_pcm_format_mask_copy(dst: *mut snd_pcm_format_mask_t, src: *const snd_pcm_format_mask_t);
    pub fn snd_pcm_format_mask_none(mask: *mut snd_pcm_format_mask_t);
    pub fn snd_pcm_format_mask_any(mask: *mut snd_pcm_format_mask_t);
    pub fn snd_pcm_format_mask_test(mask: *const snd_pcm_format_mask_t, val: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_mask_empty(mask: *const snd_pcm_format_mask_t) -> c_int;
    pub fn snd_pcm_format_mask_set(mask: *mut snd_pcm_format_mask_t, val: snd_pcm_format_t);
    pub fn snd_pcm_format_mask_reset(mask: *mut snd_pcm_format_mask_t, val: snd_pcm_format_t);
    pub fn snd_pcm_subformat_mask_sizeof() -> size_t;
    pub fn snd_pcm_subformat_mask_malloc(ptr: *mut *mut snd_pcm_subformat_mask_t) -> c_int;
    pub fn snd_pcm_subformat_mask_free(obj: *mut snd_pcm_subformat_mask_t);
    pub fn snd_pcm_subformat_mask_copy(dst: *mut snd_pcm_subformat_mask_t, src: *const snd_pcm_subformat_mask_t);
    pub fn snd_pcm_subformat_mask_none(mask: *mut snd_pcm_subformat_mask_t);
    pub fn snd_pcm_subformat_mask_any(mask: *mut snd_pcm_subformat_mask_t);
    pub fn snd_pcm_subformat_mask_test(mask: *const snd_pcm_subformat_mask_t, val: snd_pcm_subformat_t) -> c_int;
    pub fn snd_pcm_subformat_mask_empty(mask: *const snd_pcm_subformat_mask_t) -> c_int;
    pub fn snd_pcm_subformat_mask_set(mask: *mut snd_pcm_subformat_mask_t, val: snd_pcm_subformat_t);
    pub fn snd_pcm_subformat_mask_reset(mask: *mut snd_pcm_subformat_mask_t, val: snd_pcm_subformat_t);
    pub fn snd_pcm_status_sizeof() -> size_t;
    pub fn snd_pcm_status_malloc(ptr: *mut *mut snd_pcm_status_t) -> c_int;
    pub fn snd_pcm_status_free(obj: *mut snd_pcm_status_t);
    pub fn snd_pcm_status_copy(dst: *mut snd_pcm_status_t, src: *const snd_pcm_status_t);
    pub fn snd_pcm_status_get_state(obj: *const snd_pcm_status_t) -> snd_pcm_state_t;
    pub fn snd_pcm_status_get_trigger_tstamp(obj: *const snd_pcm_status_t, ptr: *mut snd_timestamp_t);
    pub fn snd_pcm_status_get_trigger_htstamp(obj: *const snd_pcm_status_t, ptr: *mut snd_htimestamp_t);
    pub fn snd_pcm_status_get_tstamp(obj: *const snd_pcm_status_t, ptr: *mut snd_timestamp_t);
    pub fn snd_pcm_status_get_htstamp(obj: *const snd_pcm_status_t, ptr: *mut snd_htimestamp_t);
    pub fn snd_pcm_status_get_audio_htstamp(obj: *const snd_pcm_status_t, ptr: *mut snd_htimestamp_t);
    pub fn snd_pcm_status_get_delay(obj: *const snd_pcm_status_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_status_get_avail(obj: *const snd_pcm_status_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_status_get_avail_max(obj: *const snd_pcm_status_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_status_get_overrange(obj: *const snd_pcm_status_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_type_name(_type: snd_pcm_type_t) -> *const c_char;
    pub fn snd_pcm_stream_name(stream: snd_pcm_stream_t) -> *const c_char;
    pub fn snd_pcm_access_name(_access: snd_pcm_access_t) -> *const c_char;
    pub fn snd_pcm_format_name(format: snd_pcm_format_t) -> *const c_char;
    pub fn snd_pcm_format_description(format: snd_pcm_format_t) -> *const c_char;
    pub fn snd_pcm_subformat_name(subformat: snd_pcm_subformat_t) -> *const c_char;
    pub fn snd_pcm_subformat_description(subformat: snd_pcm_subformat_t) -> *const c_char;
    pub fn snd_pcm_format_value(name: *const c_char) -> snd_pcm_format_t;
    pub fn snd_pcm_tstamp_mode_name(mode: snd_pcm_tstamp_t) -> *const c_char;
    pub fn snd_pcm_state_name(state: snd_pcm_state_t) -> *const c_char;
    pub fn snd_pcm_dump(pcm: *mut snd_pcm_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_dump_hw_setup(pcm: *mut snd_pcm_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_dump_sw_setup(pcm: *mut snd_pcm_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_dump_setup(pcm: *mut snd_pcm_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_hw_params_dump(params: *mut snd_pcm_hw_params_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_sw_params_dump(params: *mut snd_pcm_sw_params_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_status_dump(status: *mut snd_pcm_status_t, out: *mut snd_output_t) -> c_int;
    pub fn snd_pcm_mmap_begin(pcm: *mut snd_pcm_t, areas: *mut *const snd_pcm_channel_area_t, offset: *mut snd_pcm_uframes_t, frames: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_mmap_commit(pcm: *mut snd_pcm_t, offset: snd_pcm_uframes_t, frames: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_mmap_writei(pcm: *mut snd_pcm_t, buffer: *const c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_mmap_readi(pcm: *mut snd_pcm_t, buffer: *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_mmap_writen(pcm: *mut snd_pcm_t, bufs: *mut *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_mmap_readn(pcm: *mut snd_pcm_t, bufs: *mut *mut c_void, size: snd_pcm_uframes_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_format_signed(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_unsigned(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_linear(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_float(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_little_endian(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_big_endian(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_cpu_endian(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_width(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_format_physical_width(format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_build_linear_format(width: c_int, pwidth: c_int, unsignd: c_int, big_endian: c_int) -> snd_pcm_format_t;
    pub fn snd_pcm_format_size(format: snd_pcm_format_t, samples: size_t) -> ssize_t;
    pub fn snd_pcm_format_silence(format: snd_pcm_format_t) -> u8;
    pub fn snd_pcm_format_silence_16(format: snd_pcm_format_t) -> u16;
    pub fn snd_pcm_format_silence_32(format: snd_pcm_format_t) -> u32;
    pub fn snd_pcm_format_silence_64(format: snd_pcm_format_t) -> u64;
    pub fn snd_pcm_format_set_silence(format: snd_pcm_format_t, buf: *mut c_void, samples: c_uint) -> c_int;
    pub fn snd_pcm_bytes_to_frames(pcm: *mut snd_pcm_t, bytes: ssize_t) -> snd_pcm_sframes_t;
    pub fn snd_pcm_frames_to_bytes(pcm: *mut snd_pcm_t, frames: snd_pcm_sframes_t) -> ssize_t;
    pub fn snd_pcm_bytes_to_samples(pcm: *mut snd_pcm_t, bytes: ssize_t) -> c_long;
    pub fn snd_pcm_samples_to_bytes(pcm: *mut snd_pcm_t, samples: c_long) -> ssize_t;
    pub fn snd_pcm_area_silence(dst_channel: *const snd_pcm_channel_area_t, dst_offset: snd_pcm_uframes_t, samples: c_uint, format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_areas_silence(dst_channels: *const snd_pcm_channel_area_t, dst_offset: snd_pcm_uframes_t, channels: c_uint, frames: snd_pcm_uframes_t, format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_area_copy(dst_channel: *const snd_pcm_channel_area_t, dst_offset: snd_pcm_uframes_t, src_channel: *const snd_pcm_channel_area_t, src_offset: snd_pcm_uframes_t, samples: c_uint, format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_areas_copy(dst_channels: *const snd_pcm_channel_area_t, dst_offset: snd_pcm_uframes_t, src_channels: *const snd_pcm_channel_area_t, src_offset: snd_pcm_uframes_t, channels: c_uint, frames: snd_pcm_uframes_t, format: snd_pcm_format_t) -> c_int;
    pub fn snd_pcm_hook_get_pcm(hook: *mut snd_pcm_hook_t) -> *mut snd_pcm_t;
    pub fn snd_pcm_hook_get_private(hook: *mut snd_pcm_hook_t) -> *mut c_void;
    pub fn snd_pcm_hook_set_private(hook: *mut snd_pcm_hook_t, private_data: *mut c_void);
    pub fn snd_pcm_hook_add(hookp: *mut *mut snd_pcm_hook_t, pcm: *mut snd_pcm_t, _type: snd_pcm_hook_type_t, func: snd_pcm_hook_func_t, private_data: *mut c_void) -> c_int;
    pub fn snd_pcm_hook_remove(hook: *mut snd_pcm_hook_t) -> c_int;
    pub fn snd_pcm_meter_get_bufsize(pcm: *mut snd_pcm_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_meter_get_channels(pcm: *mut snd_pcm_t) -> c_uint;
    pub fn snd_pcm_meter_get_rate(pcm: *mut snd_pcm_t) -> c_uint;
    pub fn snd_pcm_meter_get_now(pcm: *mut snd_pcm_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_meter_get_boundary(pcm: *mut snd_pcm_t) -> snd_pcm_uframes_t;
    pub fn snd_pcm_meter_add_scope(pcm: *mut snd_pcm_t, scope: *mut snd_pcm_scope_t) -> c_int;
    pub fn snd_pcm_meter_search_scope(pcm: *mut snd_pcm_t, name: *const c_char) -> *mut snd_pcm_scope_t;
    pub fn snd_pcm_scope_malloc(ptr: *mut *mut snd_pcm_scope_t) -> c_int;
    pub fn snd_pcm_scope_set_ops(scope: *mut snd_pcm_scope_t, val: *const snd_pcm_scope_ops_t);
    pub fn snd_pcm_scope_set_name(scope: *mut snd_pcm_scope_t, val: *const c_char);
    pub fn snd_pcm_scope_get_name(scope: *mut snd_pcm_scope_t) -> *const c_char;
    pub fn snd_pcm_scope_get_callback_private(scope: *mut snd_pcm_scope_t) -> *mut c_void;
    pub fn snd_pcm_scope_set_callback_private(scope: *mut snd_pcm_scope_t, val: *mut c_void);
    pub fn snd_pcm_scope_s16_open(pcm: *mut snd_pcm_t, name: *const c_char, scopep: *mut *mut snd_pcm_scope_t) -> c_int;
    pub fn snd_pcm_scope_s16_get_channel_buffer(scope: *mut snd_pcm_scope_t, channel: c_uint) -> *mut i16;
    pub fn snd_spcm_init(pcm: *mut snd_pcm_t, rate: c_uint, channels: c_uint, format: snd_pcm_format_t, subformat: snd_pcm_subformat_t, latency: snd_spcm_latency_t, _access: snd_pcm_access_t, xrun_type: snd_spcm_xrun_type_t) -> c_int;
    pub fn snd_spcm_init_duplex(playback_pcm: *mut snd_pcm_t, capture_pcm: *mut snd_pcm_t, rate: c_uint, channels: c_uint, format: snd_pcm_format_t, subformat: snd_pcm_subformat_t, latency: snd_spcm_latency_t, _access: snd_pcm_access_t, xrun_type: snd_spcm_xrun_type_t, duplex_type: snd_spcm_duplex_type_t) -> c_int;
    pub fn snd_spcm_init_get_params(pcm: *mut snd_pcm_t, rate: *mut c_uint, buffer_size: *mut snd_pcm_uframes_t, period_size: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_start_mode_name(mode: snd_pcm_start_t) -> *const c_char;
    pub fn snd_pcm_xrun_mode_name(mode: snd_pcm_xrun_t) -> *const c_char;
    pub fn snd_pcm_sw_params_set_start_mode(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_start_t) -> c_int;
    pub fn snd_pcm_sw_params_get_start_mode(params: *const snd_pcm_sw_params_t) -> snd_pcm_start_t;
    pub fn snd_pcm_sw_params_set_xrun_mode(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_xrun_t) -> c_int;
    pub fn snd_pcm_sw_params_get_xrun_mode(params: *const snd_pcm_sw_params_t) -> snd_pcm_xrun_t;
    pub fn snd_pcm_sw_params_set_xfer_align(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_get_xfer_align(params: *const snd_pcm_sw_params_t, val: *mut snd_pcm_uframes_t) -> c_int;
    pub fn snd_pcm_sw_params_set_sleep_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_sw_params_t, val: c_uint) -> c_int;
    pub fn snd_pcm_sw_params_get_sleep_min(params: *const snd_pcm_sw_params_t, val: *mut c_uint) -> c_int;
    pub fn snd_pcm_hw_params_get_tick_time(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_tick_time_min(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_get_tick_time_max(params: *const snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_test_tick_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: c_uint, dir: c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_min(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_max(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_minmax(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, min: *mut c_uint, mindir: *mut c_int, max: *mut c_uint, maxdir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_near(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_first(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_pcm_hw_params_set_tick_time_last(pcm: *mut snd_pcm_t, params: *mut snd_pcm_hw_params_t, val: *mut c_uint, dir: *mut c_int) -> c_int;
    pub fn snd_rawmidi_open(in_rmidi: *mut *mut snd_rawmidi_t, out_rmidi: *mut *mut snd_rawmidi_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_rawmidi_open_lconf(in_rmidi: *mut *mut snd_rawmidi_t, out_rmidi: *mut *mut snd_rawmidi_t, name: *const c_char, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_rawmidi_close(rmidi: *mut snd_rawmidi_t) -> c_int;
    pub fn snd_rawmidi_poll_descriptors_count(rmidi: *mut snd_rawmidi_t) -> c_int;
    //pub fn snd_rawmidi_poll_descriptors(rmidi: *mut snd_rawmidi_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_rawmidi_poll_descriptors_revents(rawmidi: *mut snd_rawmidi_t, pfds: *mut Struct_pollfd, nfds: c_uint, revent: *mut c_ushort) -> c_int;
    pub fn snd_rawmidi_nonblock(rmidi: *mut snd_rawmidi_t, nonblock: c_int) -> c_int;
    pub fn snd_rawmidi_info_sizeof() -> size_t;
    pub fn snd_rawmidi_info_malloc(ptr: *mut *mut snd_rawmidi_info_t) -> c_int;
    pub fn snd_rawmidi_info_free(obj: *mut snd_rawmidi_info_t);
    pub fn snd_rawmidi_info_copy(dst: *mut snd_rawmidi_info_t, src: *const snd_rawmidi_info_t);
    pub fn snd_rawmidi_info_get_device(obj: *const snd_rawmidi_info_t) -> c_uint;
    pub fn snd_rawmidi_info_get_subdevice(obj: *const snd_rawmidi_info_t) -> c_uint;
    pub fn snd_rawmidi_info_get_stream(obj: *const snd_rawmidi_info_t) -> snd_rawmidi_stream_t;
    pub fn snd_rawmidi_info_get_card(obj: *const snd_rawmidi_info_t) -> c_int;
    pub fn snd_rawmidi_info_get_flags(obj: *const snd_rawmidi_info_t) -> c_uint;
    pub fn snd_rawmidi_info_get_id(obj: *const snd_rawmidi_info_t) -> *const c_char;
    pub fn snd_rawmidi_info_get_name(obj: *const snd_rawmidi_info_t) -> *const c_char;
    pub fn snd_rawmidi_info_get_subdevice_name(obj: *const snd_rawmidi_info_t) -> *const c_char;
    pub fn snd_rawmidi_info_get_subdevices_count(obj: *const snd_rawmidi_info_t) -> c_uint;
    pub fn snd_rawmidi_info_get_subdevices_avail(obj: *const snd_rawmidi_info_t) -> c_uint;
    pub fn snd_rawmidi_info_set_device(obj: *mut snd_rawmidi_info_t, val: c_uint);
    pub fn snd_rawmidi_info_set_subdevice(obj: *mut snd_rawmidi_info_t, val: c_uint);
    pub fn snd_rawmidi_info_set_stream(obj: *mut snd_rawmidi_info_t, val: snd_rawmidi_stream_t);
    pub fn snd_rawmidi_info(rmidi: *mut snd_rawmidi_t, info: *mut snd_rawmidi_info_t) -> c_int;
    pub fn snd_rawmidi_params_sizeof() -> size_t;
    pub fn snd_rawmidi_params_malloc(ptr: *mut *mut snd_rawmidi_params_t) -> c_int;
    pub fn snd_rawmidi_params_free(obj: *mut snd_rawmidi_params_t);
    pub fn snd_rawmidi_params_copy(dst: *mut snd_rawmidi_params_t, src: *const snd_rawmidi_params_t);
    pub fn snd_rawmidi_params_set_buffer_size(rmidi: *mut snd_rawmidi_t, params: *mut snd_rawmidi_params_t, val: size_t) -> c_int;
    pub fn snd_rawmidi_params_get_buffer_size(params: *const snd_rawmidi_params_t) -> size_t;
    pub fn snd_rawmidi_params_set_avail_min(rmidi: *mut snd_rawmidi_t, params: *mut snd_rawmidi_params_t, val: size_t) -> c_int;
    pub fn snd_rawmidi_params_get_avail_min(params: *const snd_rawmidi_params_t) -> size_t;
    pub fn snd_rawmidi_params_set_no_active_sensing(rmidi: *mut snd_rawmidi_t, params: *mut snd_rawmidi_params_t, val: c_int) -> c_int;
    pub fn snd_rawmidi_params_get_no_active_sensing(params: *const snd_rawmidi_params_t) -> c_int;
    pub fn snd_rawmidi_params(rmidi: *mut snd_rawmidi_t, params: *mut snd_rawmidi_params_t) -> c_int;
    pub fn snd_rawmidi_params_current(rmidi: *mut snd_rawmidi_t, params: *mut snd_rawmidi_params_t) -> c_int;
    pub fn snd_rawmidi_status_sizeof() -> size_t;
    pub fn snd_rawmidi_status_malloc(ptr: *mut *mut snd_rawmidi_status_t) -> c_int;
    pub fn snd_rawmidi_status_free(obj: *mut snd_rawmidi_status_t);
    pub fn snd_rawmidi_status_copy(dst: *mut snd_rawmidi_status_t, src: *const snd_rawmidi_status_t);
    pub fn snd_rawmidi_status_get_tstamp(obj: *const snd_rawmidi_status_t, ptr: *mut snd_htimestamp_t);
    pub fn snd_rawmidi_status_get_avail(obj: *const snd_rawmidi_status_t) -> size_t;
    pub fn snd_rawmidi_status_get_xruns(obj: *const snd_rawmidi_status_t) -> size_t;
    pub fn snd_rawmidi_status(rmidi: *mut snd_rawmidi_t, status: *mut snd_rawmidi_status_t) -> c_int;
    pub fn snd_rawmidi_drain(rmidi: *mut snd_rawmidi_t) -> c_int;
    pub fn snd_rawmidi_drop(rmidi: *mut snd_rawmidi_t) -> c_int;
    pub fn snd_rawmidi_write(rmidi: *mut snd_rawmidi_t, buffer: *const c_void, size: size_t) -> ssize_t;
    pub fn snd_rawmidi_read(rmidi: *mut snd_rawmidi_t, buffer: *mut c_void, size: size_t) -> ssize_t;
    pub fn snd_rawmidi_name(rmidi: *mut snd_rawmidi_t) -> *const c_char;
    pub fn snd_rawmidi_type(rmidi: *mut snd_rawmidi_t) -> snd_rawmidi_type_t;
    pub fn snd_rawmidi_stream(rawmidi: *mut snd_rawmidi_t) -> snd_rawmidi_stream_t;
    pub fn snd_timer_query_open(handle: *mut *mut snd_timer_query_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_timer_query_open_lconf(handle: *mut *mut snd_timer_query_t, name: *const c_char, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_timer_query_close(handle: *mut snd_timer_query_t) -> c_int;
    pub fn snd_timer_query_next_device(handle: *mut snd_timer_query_t, tid: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_query_info(handle: *mut snd_timer_query_t, info: *mut snd_timer_ginfo_t) -> c_int;
    pub fn snd_timer_query_params(handle: *mut snd_timer_query_t, params: *mut snd_timer_gparams_t) -> c_int;
    pub fn snd_timer_query_status(handle: *mut snd_timer_query_t, status: *mut snd_timer_gstatus_t) -> c_int;
    pub fn snd_timer_open(handle: *mut *mut snd_timer_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_timer_open_lconf(handle: *mut *mut snd_timer_t, name: *const c_char, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_timer_close(handle: *mut snd_timer_t) -> c_int;
    pub fn snd_async_add_timer_handler(handler: *mut *mut snd_async_handler_t, timer: *mut snd_timer_t, callback: snd_async_callback_t, private_data: *mut c_void) -> c_int;
    pub fn snd_async_handler_get_timer(handler: *mut snd_async_handler_t) -> *mut snd_timer_t;
    pub fn snd_timer_poll_descriptors_count(handle: *mut snd_timer_t) -> c_int;
    //pub fn snd_timer_poll_descriptors(handle: *mut snd_timer_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_timer_poll_descriptors_revents(timer: *mut snd_timer_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_timer_info(handle: *mut snd_timer_t, timer: *mut snd_timer_info_t) -> c_int;
    pub fn snd_timer_params(handle: *mut snd_timer_t, params: *mut snd_timer_params_t) -> c_int;
    pub fn snd_timer_status(handle: *mut snd_timer_t, status: *mut snd_timer_status_t) -> c_int;
    pub fn snd_timer_start(handle: *mut snd_timer_t) -> c_int;
    pub fn snd_timer_stop(handle: *mut snd_timer_t) -> c_int;
    pub fn snd_timer_continue(handle: *mut snd_timer_t) -> c_int;
    pub fn snd_timer_read(handle: *mut snd_timer_t, buffer: *mut c_void, size: size_t) -> ssize_t;
    pub fn snd_timer_id_sizeof() -> size_t;
    pub fn snd_timer_id_malloc(ptr: *mut *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_id_free(obj: *mut snd_timer_id_t);
    pub fn snd_timer_id_copy(dst: *mut snd_timer_id_t, src: *const snd_timer_id_t);
    pub fn snd_timer_id_set_class(id: *mut snd_timer_id_t, dev_class: c_int);
    pub fn snd_timer_id_get_class(id: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_id_set_sclass(id: *mut snd_timer_id_t, dev_sclass: c_int);
    pub fn snd_timer_id_get_sclass(id: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_id_set_card(id: *mut snd_timer_id_t, card: c_int);
    pub fn snd_timer_id_get_card(id: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_id_set_device(id: *mut snd_timer_id_t, device: c_int);
    pub fn snd_timer_id_get_device(id: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_id_set_subdevice(id: *mut snd_timer_id_t, subdevice: c_int);
    pub fn snd_timer_id_get_subdevice(id: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_ginfo_sizeof() -> size_t;
    pub fn snd_timer_ginfo_malloc(ptr: *mut *mut snd_timer_ginfo_t) -> c_int;
    pub fn snd_timer_ginfo_free(obj: *mut snd_timer_ginfo_t);
    pub fn snd_timer_ginfo_copy(dst: *mut snd_timer_ginfo_t, src: *const snd_timer_ginfo_t);
    pub fn snd_timer_ginfo_set_tid(obj: *mut snd_timer_ginfo_t, tid: *mut snd_timer_id_t) -> c_int;
    pub fn snd_timer_ginfo_get_tid(obj: *mut snd_timer_ginfo_t) -> *mut snd_timer_id_t;
    pub fn snd_timer_ginfo_get_flags(obj: *mut snd_timer_ginfo_t) -> c_uint;
    pub fn snd_timer_ginfo_get_card(obj: *mut snd_timer_ginfo_t) -> c_int;
    pub fn snd_timer_ginfo_get_id(obj: *mut snd_timer_ginfo_t) -> *mut c_char;
    pub fn snd_timer_ginfo_get_name(obj: *mut snd_timer_ginfo_t) -> *mut c_char;
    pub fn snd_timer_ginfo_get_resolution(obj: *mut snd_timer_ginfo_t) -> c_ulong;
    pub fn snd_timer_ginfo_get_resolution_min(obj: *mut snd_timer_ginfo_t) -> c_ulong;
    pub fn snd_timer_ginfo_get_resolution_max(obj: *mut snd_timer_ginfo_t) -> c_ulong;
    pub fn snd_timer_ginfo_get_clients(obj: *mut snd_timer_ginfo_t) -> c_uint;
    pub fn snd_timer_info_sizeof() -> size_t;
    pub fn snd_timer_info_malloc(ptr: *mut *mut snd_timer_info_t) -> c_int;
    pub fn snd_timer_info_free(obj: *mut snd_timer_info_t);
    pub fn snd_timer_info_copy(dst: *mut snd_timer_info_t, src: *const snd_timer_info_t);
    pub fn snd_timer_info_is_slave(info: *mut snd_timer_info_t) -> c_int;
    pub fn snd_timer_info_get_card(info: *mut snd_timer_info_t) -> c_int;
    pub fn snd_timer_info_get_id(info: *mut snd_timer_info_t) -> *const c_char;
    pub fn snd_timer_info_get_name(info: *mut snd_timer_info_t) -> *const c_char;
    pub fn snd_timer_info_get_resolution(info: *mut snd_timer_info_t) -> c_long;
    pub fn snd_timer_params_sizeof() -> size_t;
    pub fn snd_timer_params_malloc(ptr: *mut *mut snd_timer_params_t) -> c_int;
    pub fn snd_timer_params_free(obj: *mut snd_timer_params_t);
    pub fn snd_timer_params_copy(dst: *mut snd_timer_params_t, src: *const snd_timer_params_t);
    pub fn snd_timer_params_set_auto_start(params: *mut snd_timer_params_t, auto_start: c_int) -> c_int;
    pub fn snd_timer_params_get_auto_start(params: *mut snd_timer_params_t) -> c_int;
    pub fn snd_timer_params_set_exclusive(params: *mut snd_timer_params_t, exclusive: c_int) -> c_int;
    pub fn snd_timer_params_get_exclusive(params: *mut snd_timer_params_t) -> c_int;
    pub fn snd_timer_params_set_early_event(params: *mut snd_timer_params_t, early_event: c_int) -> c_int;
    pub fn snd_timer_params_get_early_event(params: *mut snd_timer_params_t) -> c_int;
    pub fn snd_timer_params_set_ticks(params: *mut snd_timer_params_t, ticks: c_long);
    pub fn snd_timer_params_get_ticks(params: *mut snd_timer_params_t) -> c_long;
    pub fn snd_timer_params_set_queue_size(params: *mut snd_timer_params_t, queue_size: c_long);
    pub fn snd_timer_params_get_queue_size(params: *mut snd_timer_params_t) -> c_long;
    pub fn snd_timer_params_set_filter(params: *mut snd_timer_params_t, filter: c_uint);
    pub fn snd_timer_params_get_filter(params: *mut snd_timer_params_t) -> c_uint;
    pub fn snd_timer_status_sizeof() -> size_t;
    pub fn snd_timer_status_malloc(ptr: *mut *mut snd_timer_status_t) -> c_int;
    pub fn snd_timer_status_free(obj: *mut snd_timer_status_t);
    pub fn snd_timer_status_copy(dst: *mut snd_timer_status_t, src: *const snd_timer_status_t);
    pub fn snd_timer_status_get_timestamp(status: *mut snd_timer_status_t) -> snd_htimestamp_t;
    pub fn snd_timer_status_get_resolution(status: *mut snd_timer_status_t) -> c_long;
    pub fn snd_timer_status_get_lost(status: *mut snd_timer_status_t) -> c_long;
    pub fn snd_timer_status_get_overrun(status: *mut snd_timer_status_t) -> c_long;
    pub fn snd_timer_status_get_queue(status: *mut snd_timer_status_t) -> c_long;
    pub fn snd_timer_info_get_ticks(info: *mut snd_timer_info_t) -> c_long;
    pub fn snd_hwdep_open(hwdep: *mut *mut snd_hwdep_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_hwdep_close(hwdep: *mut snd_hwdep_t) -> c_int;
    //pub fn snd_hwdep_poll_descriptors(hwdep: *mut snd_hwdep_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_hwdep_poll_descriptors_revents(hwdep: *mut snd_hwdep_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_hwdep_nonblock(hwdep: *mut snd_hwdep_t, nonblock: c_int) -> c_int;
    pub fn snd_hwdep_info(hwdep: *mut snd_hwdep_t, info: *mut snd_hwdep_info_t) -> c_int;
    pub fn snd_hwdep_dsp_status(hwdep: *mut snd_hwdep_t, status: *mut snd_hwdep_dsp_status_t) -> c_int;
    pub fn snd_hwdep_dsp_load(hwdep: *mut snd_hwdep_t, block: *mut snd_hwdep_dsp_image_t) -> c_int;
    pub fn snd_hwdep_ioctl(hwdep: *mut snd_hwdep_t, request: c_uint, arg: *mut c_void) -> c_int;
    pub fn snd_hwdep_write(hwdep: *mut snd_hwdep_t, buffer: *const c_void, size: size_t) -> ssize_t;
    pub fn snd_hwdep_read(hwdep: *mut snd_hwdep_t, buffer: *mut c_void, size: size_t) -> ssize_t;
    pub fn snd_hwdep_info_sizeof() -> size_t;
    pub fn snd_hwdep_info_malloc(ptr: *mut *mut snd_hwdep_info_t) -> c_int;
    pub fn snd_hwdep_info_free(obj: *mut snd_hwdep_info_t);
    pub fn snd_hwdep_info_copy(dst: *mut snd_hwdep_info_t, src: *const snd_hwdep_info_t);
    pub fn snd_hwdep_info_get_device(obj: *const snd_hwdep_info_t) -> c_uint;
    pub fn snd_hwdep_info_get_card(obj: *const snd_hwdep_info_t) -> c_int;
    pub fn snd_hwdep_info_get_id(obj: *const snd_hwdep_info_t) -> *const c_char;
    pub fn snd_hwdep_info_get_name(obj: *const snd_hwdep_info_t) -> *const c_char;
    pub fn snd_hwdep_info_get_iface(obj: *const snd_hwdep_info_t) -> snd_hwdep_iface_t;
    pub fn snd_hwdep_info_set_device(obj: *mut snd_hwdep_info_t, val: c_uint);
    pub fn snd_hwdep_dsp_status_sizeof() -> size_t;
    pub fn snd_hwdep_dsp_status_malloc(ptr: *mut *mut snd_hwdep_dsp_status_t) -> c_int;
    pub fn snd_hwdep_dsp_status_free(obj: *mut snd_hwdep_dsp_status_t);
    pub fn snd_hwdep_dsp_status_copy(dst: *mut snd_hwdep_dsp_status_t, src: *const snd_hwdep_dsp_status_t);
    pub fn snd_hwdep_dsp_status_get_version(obj: *const snd_hwdep_dsp_status_t) -> c_uint;
    pub fn snd_hwdep_dsp_status_get_id(obj: *const snd_hwdep_dsp_status_t) -> *const c_char;
    pub fn snd_hwdep_dsp_status_get_num_dsps(obj: *const snd_hwdep_dsp_status_t) -> c_uint;
    pub fn snd_hwdep_dsp_status_get_dsp_loaded(obj: *const snd_hwdep_dsp_status_t) -> c_uint;
    pub fn snd_hwdep_dsp_status_get_chip_ready(obj: *const snd_hwdep_dsp_status_t) -> c_uint;
    pub fn snd_hwdep_dsp_image_sizeof() -> size_t;
    pub fn snd_hwdep_dsp_image_malloc(ptr: *mut *mut snd_hwdep_dsp_image_t) -> c_int;
    pub fn snd_hwdep_dsp_image_free(obj: *mut snd_hwdep_dsp_image_t);
    pub fn snd_hwdep_dsp_image_copy(dst: *mut snd_hwdep_dsp_image_t, src: *const snd_hwdep_dsp_image_t);
    pub fn snd_hwdep_dsp_image_get_index(obj: *const snd_hwdep_dsp_image_t) -> c_uint;
    pub fn snd_hwdep_dsp_image_get_name(obj: *const snd_hwdep_dsp_image_t) -> *const c_char;
    pub fn snd_hwdep_dsp_image_get_image(obj: *const snd_hwdep_dsp_image_t) -> *const c_void;
    pub fn snd_hwdep_dsp_image_get_length(obj: *const snd_hwdep_dsp_image_t) -> size_t;
    pub fn snd_hwdep_dsp_image_set_index(obj: *mut snd_hwdep_dsp_image_t, _index: c_uint);
    pub fn snd_hwdep_dsp_image_set_name(obj: *mut snd_hwdep_dsp_image_t, name: *const c_char);
    pub fn snd_hwdep_dsp_image_set_image(obj: *mut snd_hwdep_dsp_image_t, buffer: *mut c_void);
    pub fn snd_hwdep_dsp_image_set_length(obj: *mut snd_hwdep_dsp_image_t, length: size_t);
    pub fn snd_card_load(card: c_int) -> c_int;
    pub fn snd_card_next(card: *mut c_int) -> c_int;
    pub fn snd_card_get_index(name: *const c_char) -> c_int;
    pub fn snd_card_get_name(card: c_int, name: *mut *mut c_char) -> c_int;
    pub fn snd_card_get_longname(card: c_int, name: *mut *mut c_char) -> c_int;
    pub fn snd_device_name_hint(card: c_int, iface: *const c_char, hints: *mut *mut *mut c_void) -> c_int;
    pub fn snd_device_name_free_hint(hints: *mut *mut c_void) -> c_int;
    pub fn snd_device_name_get_hint(hint: *const c_void, id: *const c_char) -> *mut c_char;
    pub fn snd_ctl_open(ctl: *mut *mut snd_ctl_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_ctl_open_lconf(ctl: *mut *mut snd_ctl_t, name: *const c_char, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_ctl_open_fallback(ctl: *mut *mut snd_ctl_t, root: *mut snd_config_t, name: *const c_char, orig_name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_ctl_close(ctl: *mut snd_ctl_t) -> c_int;
    pub fn snd_ctl_nonblock(ctl: *mut snd_ctl_t, nonblock: c_int) -> c_int;
    pub fn snd_async_add_ctl_handler(handler: *mut *mut snd_async_handler_t, ctl: *mut snd_ctl_t, callback: snd_async_callback_t, private_data: *mut c_void) -> c_int;
    pub fn snd_async_handler_get_ctl(handler: *mut snd_async_handler_t) -> *mut snd_ctl_t;
    pub fn snd_ctl_poll_descriptors_count(ctl: *mut snd_ctl_t) -> c_int;
    //pub fn snd_ctl_poll_descriptors(ctl: *mut snd_ctl_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_ctl_poll_descriptors_revents(ctl: *mut snd_ctl_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_ctl_subscribe_events(ctl: *mut snd_ctl_t, subscribe: c_int) -> c_int;
    pub fn snd_ctl_card_info(ctl: *mut snd_ctl_t, info: *mut snd_ctl_card_info_t) -> c_int;
    pub fn snd_ctl_elem_list(ctl: *mut snd_ctl_t, list: *mut snd_ctl_elem_list_t) -> c_int;
    pub fn snd_ctl_elem_info(ctl: *mut snd_ctl_t, info: *mut snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_read(ctl: *mut snd_ctl_t, value: *mut snd_ctl_elem_value_t) -> c_int;
    pub fn snd_ctl_elem_write(ctl: *mut snd_ctl_t, value: *mut snd_ctl_elem_value_t) -> c_int;
    pub fn snd_ctl_elem_lock(ctl: *mut snd_ctl_t, id: *mut snd_ctl_elem_id_t) -> c_int;
    pub fn snd_ctl_elem_unlock(ctl: *mut snd_ctl_t, id: *mut snd_ctl_elem_id_t) -> c_int;
    pub fn snd_ctl_elem_tlv_read(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, tlv: *mut c_uint, tlv_size: c_uint) -> c_int;
    pub fn snd_ctl_elem_tlv_write(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, tlv: *const c_uint) -> c_int;
    pub fn snd_ctl_elem_tlv_command(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, tlv: *const c_uint) -> c_int;
    pub fn snd_ctl_hwdep_next_device(ctl: *mut snd_ctl_t, device: *mut c_int) -> c_int;
    pub fn snd_ctl_hwdep_info(ctl: *mut snd_ctl_t, info: *mut snd_hwdep_info_t) -> c_int;
    pub fn snd_ctl_pcm_next_device(ctl: *mut snd_ctl_t, device: *mut c_int) -> c_int;
    pub fn snd_ctl_pcm_info(ctl: *mut snd_ctl_t, info: *mut snd_pcm_info_t) -> c_int;
    pub fn snd_ctl_pcm_prefer_subdevice(ctl: *mut snd_ctl_t, subdev: c_int) -> c_int;
    pub fn snd_ctl_rawmidi_next_device(ctl: *mut snd_ctl_t, device: *mut c_int) -> c_int;
    pub fn snd_ctl_rawmidi_info(ctl: *mut snd_ctl_t, info: *mut snd_rawmidi_info_t) -> c_int;
    pub fn snd_ctl_rawmidi_prefer_subdevice(ctl: *mut snd_ctl_t, subdev: c_int) -> c_int;
    pub fn snd_ctl_set_power_state(ctl: *mut snd_ctl_t, state: c_uint) -> c_int;
    pub fn snd_ctl_get_power_state(ctl: *mut snd_ctl_t, state: *mut c_uint) -> c_int;
    pub fn snd_ctl_read(ctl: *mut snd_ctl_t, event: *mut snd_ctl_event_t) -> c_int;
    pub fn snd_ctl_wait(ctl: *mut snd_ctl_t, timeout: c_int) -> c_int;
    pub fn snd_ctl_name(ctl: *mut snd_ctl_t) -> *const c_char;
    pub fn snd_ctl_type(ctl: *mut snd_ctl_t) -> snd_ctl_type_t;
    pub fn snd_ctl_elem_type_name(_type: snd_ctl_elem_type_t) -> *const c_char;
    pub fn snd_ctl_elem_iface_name(iface: snd_ctl_elem_iface_t) -> *const c_char;
    pub fn snd_ctl_event_type_name(_type: snd_ctl_event_type_t) -> *const c_char;
    pub fn snd_ctl_event_elem_get_mask(obj: *const snd_ctl_event_t) -> c_uint;
    pub fn snd_ctl_event_elem_get_numid(obj: *const snd_ctl_event_t) -> c_uint;
    pub fn snd_ctl_event_elem_get_id(obj: *const snd_ctl_event_t, ptr: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_event_elem_get_interface(obj: *const snd_ctl_event_t) -> snd_ctl_elem_iface_t;
    pub fn snd_ctl_event_elem_get_device(obj: *const snd_ctl_event_t) -> c_uint;
    pub fn snd_ctl_event_elem_get_subdevice(obj: *const snd_ctl_event_t) -> c_uint;
    pub fn snd_ctl_event_elem_get_name(obj: *const snd_ctl_event_t) -> *const c_char;
    pub fn snd_ctl_event_elem_get_index(obj: *const snd_ctl_event_t) -> c_uint;
    pub fn snd_ctl_elem_list_alloc_space(obj: *mut snd_ctl_elem_list_t, entries: c_uint) -> c_int;
    pub fn snd_ctl_elem_list_free_space(obj: *mut snd_ctl_elem_list_t);
    pub fn snd_ctl_ascii_elem_id_get(id: *mut snd_ctl_elem_id_t) -> *mut c_char;
    pub fn snd_ctl_ascii_elem_id_parse(dst: *mut snd_ctl_elem_id_t, str: *const c_char) -> c_int;
    pub fn snd_ctl_ascii_value_parse(handle: *mut snd_ctl_t, dst: *mut snd_ctl_elem_value_t, info: *mut snd_ctl_elem_info_t, value: *const c_char) -> c_int;
    pub fn snd_ctl_elem_id_sizeof() -> size_t;
    pub fn snd_ctl_elem_id_malloc(ptr: *mut *mut snd_ctl_elem_id_t) -> c_int;
    pub fn snd_ctl_elem_id_free(obj: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_id_clear(obj: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_id_copy(dst: *mut snd_ctl_elem_id_t, src: *const snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_id_get_numid(obj: *const snd_ctl_elem_id_t) -> c_uint;
    pub fn snd_ctl_elem_id_get_interface(obj: *const snd_ctl_elem_id_t) -> snd_ctl_elem_iface_t;
    pub fn snd_ctl_elem_id_get_device(obj: *const snd_ctl_elem_id_t) -> c_uint;
    pub fn snd_ctl_elem_id_get_subdevice(obj: *const snd_ctl_elem_id_t) -> c_uint;
    pub fn snd_ctl_elem_id_get_name(obj: *const snd_ctl_elem_id_t) -> *const c_char;
    pub fn snd_ctl_elem_id_get_index(obj: *const snd_ctl_elem_id_t) -> c_uint;
    pub fn snd_ctl_elem_id_set_numid(obj: *mut snd_ctl_elem_id_t, val: c_uint);
    pub fn snd_ctl_elem_id_set_interface(obj: *mut snd_ctl_elem_id_t, val: snd_ctl_elem_iface_t);
    pub fn snd_ctl_elem_id_set_device(obj: *mut snd_ctl_elem_id_t, val: c_uint);
    pub fn snd_ctl_elem_id_set_subdevice(obj: *mut snd_ctl_elem_id_t, val: c_uint);
    pub fn snd_ctl_elem_id_set_name(obj: *mut snd_ctl_elem_id_t, val: *const c_char);
    pub fn snd_ctl_elem_id_set_index(obj: *mut snd_ctl_elem_id_t, val: c_uint);
    pub fn snd_ctl_card_info_sizeof() -> size_t;
    pub fn snd_ctl_card_info_malloc(ptr: *mut *mut snd_ctl_card_info_t) -> c_int;
    pub fn snd_ctl_card_info_free(obj: *mut snd_ctl_card_info_t);
    pub fn snd_ctl_card_info_clear(obj: *mut snd_ctl_card_info_t);
    pub fn snd_ctl_card_info_copy(dst: *mut snd_ctl_card_info_t, src: *const snd_ctl_card_info_t);
    pub fn snd_ctl_card_info_get_card(obj: *const snd_ctl_card_info_t) -> c_int;
    pub fn snd_ctl_card_info_get_id(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_card_info_get_driver(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_card_info_get_name(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_card_info_get_longname(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_card_info_get_mixername(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_card_info_get_components(obj: *const snd_ctl_card_info_t) -> *const c_char;
    pub fn snd_ctl_event_sizeof() -> size_t;
    pub fn snd_ctl_event_malloc(ptr: *mut *mut snd_ctl_event_t) -> c_int;
    pub fn snd_ctl_event_free(obj: *mut snd_ctl_event_t);
    pub fn snd_ctl_event_clear(obj: *mut snd_ctl_event_t);
    pub fn snd_ctl_event_copy(dst: *mut snd_ctl_event_t, src: *const snd_ctl_event_t);
    pub fn snd_ctl_event_get_type(obj: *const snd_ctl_event_t) -> snd_ctl_event_type_t;
    pub fn snd_ctl_elem_list_sizeof() -> size_t;
    pub fn snd_ctl_elem_list_malloc(ptr: *mut *mut snd_ctl_elem_list_t) -> c_int;
    pub fn snd_ctl_elem_list_free(obj: *mut snd_ctl_elem_list_t);
    pub fn snd_ctl_elem_list_clear(obj: *mut snd_ctl_elem_list_t);
    pub fn snd_ctl_elem_list_copy(dst: *mut snd_ctl_elem_list_t, src: *const snd_ctl_elem_list_t);
    pub fn snd_ctl_elem_list_set_offset(obj: *mut snd_ctl_elem_list_t, val: c_uint);
    pub fn snd_ctl_elem_list_get_used(obj: *const snd_ctl_elem_list_t) -> c_uint;
    pub fn snd_ctl_elem_list_get_count(obj: *const snd_ctl_elem_list_t) -> c_uint;
    pub fn snd_ctl_elem_list_get_id(obj: *const snd_ctl_elem_list_t, idx: c_uint, ptr: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_list_get_numid(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> c_uint;
    pub fn snd_ctl_elem_list_get_interface(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> snd_ctl_elem_iface_t;
    pub fn snd_ctl_elem_list_get_device(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> c_uint;
    pub fn snd_ctl_elem_list_get_subdevice(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> c_uint;
    pub fn snd_ctl_elem_list_get_name(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> *const c_char;
    pub fn snd_ctl_elem_list_get_index(obj: *const snd_ctl_elem_list_t, idx: c_uint) -> c_uint;
    pub fn snd_ctl_elem_info_sizeof() -> size_t;
    pub fn snd_ctl_elem_info_malloc(ptr: *mut *mut snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_free(obj: *mut snd_ctl_elem_info_t);
    pub fn snd_ctl_elem_info_clear(obj: *mut snd_ctl_elem_info_t);
    pub fn snd_ctl_elem_info_copy(dst: *mut snd_ctl_elem_info_t, src: *const snd_ctl_elem_info_t);
    pub fn snd_ctl_elem_info_get_type(obj: *const snd_ctl_elem_info_t) -> snd_ctl_elem_type_t;
    pub fn snd_ctl_elem_info_is_readable(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_writable(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_volatile(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_inactive(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_locked(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_tlv_readable(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_tlv_writable(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_tlv_commandable(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_owner(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_is_user(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_get_owner(obj: *const snd_ctl_elem_info_t) -> pid_t;
    pub fn snd_ctl_elem_info_get_count(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_get_min(obj: *const snd_ctl_elem_info_t) -> c_long;
    pub fn snd_ctl_elem_info_get_max(obj: *const snd_ctl_elem_info_t) -> c_long;
    pub fn snd_ctl_elem_info_get_step(obj: *const snd_ctl_elem_info_t) -> c_long;
    pub fn snd_ctl_elem_info_get_min64(obj: *const snd_ctl_elem_info_t) -> c_longlong;
    pub fn snd_ctl_elem_info_get_max64(obj: *const snd_ctl_elem_info_t) -> c_longlong;
    pub fn snd_ctl_elem_info_get_step64(obj: *const snd_ctl_elem_info_t) -> c_longlong;
    pub fn snd_ctl_elem_info_get_items(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_set_item(obj: *mut snd_ctl_elem_info_t, val: c_uint);
    pub fn snd_ctl_elem_info_get_item_name(obj: *const snd_ctl_elem_info_t) -> *const c_char;
    pub fn snd_ctl_elem_info_get_dimensions(obj: *const snd_ctl_elem_info_t) -> c_int;
    pub fn snd_ctl_elem_info_get_dimension(obj: *const snd_ctl_elem_info_t, idx: c_uint) -> c_int;
    pub fn snd_ctl_elem_info_get_id(obj: *const snd_ctl_elem_info_t, ptr: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_info_get_numid(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_get_interface(obj: *const snd_ctl_elem_info_t) -> snd_ctl_elem_iface_t;
    pub fn snd_ctl_elem_info_get_device(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_get_subdevice(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_get_name(obj: *const snd_ctl_elem_info_t) -> *const c_char;
    pub fn snd_ctl_elem_info_get_index(obj: *const snd_ctl_elem_info_t) -> c_uint;
    pub fn snd_ctl_elem_info_set_id(obj: *mut snd_ctl_elem_info_t, ptr: *const snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_info_set_numid(obj: *mut snd_ctl_elem_info_t, val: c_uint);
    pub fn snd_ctl_elem_info_set_interface(obj: *mut snd_ctl_elem_info_t, val: snd_ctl_elem_iface_t);
    pub fn snd_ctl_elem_info_set_device(obj: *mut snd_ctl_elem_info_t, val: c_uint);
    pub fn snd_ctl_elem_info_set_subdevice(obj: *mut snd_ctl_elem_info_t, val: c_uint);
    pub fn snd_ctl_elem_info_set_name(obj: *mut snd_ctl_elem_info_t, val: *const c_char);
    pub fn snd_ctl_elem_info_set_index(obj: *mut snd_ctl_elem_info_t, val: c_uint);
    pub fn snd_ctl_elem_add_integer(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, count: c_uint, imin: c_long, imax: c_long, istep: c_long) -> c_int;
    pub fn snd_ctl_elem_add_integer64(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, count: c_uint, imin: c_longlong, imax: c_longlong, istep: c_longlong) -> c_int;
    pub fn snd_ctl_elem_add_boolean(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, count: c_uint) -> c_int;
    pub fn snd_ctl_elem_add_enumerated(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, count: c_uint, items: c_uint, names: *const *const c_char) -> c_int;
    pub fn snd_ctl_elem_add_iec958(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t) -> c_int;
    pub fn snd_ctl_elem_remove(ctl: *mut snd_ctl_t, id: *mut snd_ctl_elem_id_t) -> c_int;
    pub fn snd_ctl_elem_value_sizeof() -> size_t;
    pub fn snd_ctl_elem_value_malloc(ptr: *mut *mut snd_ctl_elem_value_t) -> c_int;
    pub fn snd_ctl_elem_value_free(obj: *mut snd_ctl_elem_value_t);
    pub fn snd_ctl_elem_value_clear(obj: *mut snd_ctl_elem_value_t);
    pub fn snd_ctl_elem_value_copy(dst: *mut snd_ctl_elem_value_t, src: *const snd_ctl_elem_value_t);
    pub fn snd_ctl_elem_value_compare(left: *mut snd_ctl_elem_value_t, right: *const snd_ctl_elem_value_t) -> c_int;
    pub fn snd_ctl_elem_value_get_id(obj: *const snd_ctl_elem_value_t, ptr: *mut snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_value_get_numid(obj: *const snd_ctl_elem_value_t) -> c_uint;
    pub fn snd_ctl_elem_value_get_interface(obj: *const snd_ctl_elem_value_t) -> snd_ctl_elem_iface_t;
    pub fn snd_ctl_elem_value_get_device(obj: *const snd_ctl_elem_value_t) -> c_uint;
    pub fn snd_ctl_elem_value_get_subdevice(obj: *const snd_ctl_elem_value_t) -> c_uint;
    pub fn snd_ctl_elem_value_get_name(obj: *const snd_ctl_elem_value_t) -> *const c_char;
    pub fn snd_ctl_elem_value_get_index(obj: *const snd_ctl_elem_value_t) -> c_uint;
    pub fn snd_ctl_elem_value_set_id(obj: *mut snd_ctl_elem_value_t, ptr: *const snd_ctl_elem_id_t);
    pub fn snd_ctl_elem_value_set_numid(obj: *mut snd_ctl_elem_value_t, val: c_uint);
    pub fn snd_ctl_elem_value_set_interface(obj: *mut snd_ctl_elem_value_t, val: snd_ctl_elem_iface_t);
    pub fn snd_ctl_elem_value_set_device(obj: *mut snd_ctl_elem_value_t, val: c_uint);
    pub fn snd_ctl_elem_value_set_subdevice(obj: *mut snd_ctl_elem_value_t, val: c_uint);
    pub fn snd_ctl_elem_value_set_name(obj: *mut snd_ctl_elem_value_t, val: *const c_char);
    pub fn snd_ctl_elem_value_set_index(obj: *mut snd_ctl_elem_value_t, val: c_uint);
    pub fn snd_ctl_elem_value_get_boolean(obj: *const snd_ctl_elem_value_t, idx: c_uint) -> c_int;
    pub fn snd_ctl_elem_value_get_integer(obj: *const snd_ctl_elem_value_t, idx: c_uint) -> c_long;
    pub fn snd_ctl_elem_value_get_integer64(obj: *const snd_ctl_elem_value_t, idx: c_uint) -> c_longlong;
    pub fn snd_ctl_elem_value_get_enumerated(obj: *const snd_ctl_elem_value_t, idx: c_uint) -> c_uint;
    pub fn snd_ctl_elem_value_get_byte(obj: *const snd_ctl_elem_value_t, idx: c_uint) -> c_uchar;
    pub fn snd_ctl_elem_value_set_boolean(obj: *mut snd_ctl_elem_value_t, idx: c_uint, val: c_long);
    pub fn snd_ctl_elem_value_set_integer(obj: *mut snd_ctl_elem_value_t, idx: c_uint, val: c_long);
    pub fn snd_ctl_elem_value_set_integer64(obj: *mut snd_ctl_elem_value_t, idx: c_uint, val: c_longlong);
    pub fn snd_ctl_elem_value_set_enumerated(obj: *mut snd_ctl_elem_value_t, idx: c_uint, val: c_uint);
    pub fn snd_ctl_elem_value_set_byte(obj: *mut snd_ctl_elem_value_t, idx: c_uint, val: c_uchar);
    pub fn snd_ctl_elem_set_bytes(obj: *mut snd_ctl_elem_value_t, data: *mut c_void, size: size_t);
    pub fn snd_ctl_elem_value_get_bytes(obj: *const snd_ctl_elem_value_t) -> *const c_void;
    pub fn snd_ctl_elem_value_get_iec958(obj: *const snd_ctl_elem_value_t, ptr: *mut snd_aes_iec958_t);
    pub fn snd_ctl_elem_value_set_iec958(obj: *mut snd_ctl_elem_value_t, ptr: *const snd_aes_iec958_t);
    pub fn snd_tlv_parse_dB_info(tlv: *mut c_uint, tlv_size: c_uint, db_tlvp: *mut *mut c_uint) -> c_int;
    pub fn snd_tlv_get_dB_range(tlv: *mut c_uint, rangemin: c_long, rangemax: c_long, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_tlv_convert_to_dB(tlv: *mut c_uint, rangemin: c_long, rangemax: c_long, volume: c_long, db_gain: *mut c_long) -> c_int;
    pub fn snd_tlv_convert_from_dB(tlv: *mut c_uint, rangemin: c_long, rangemax: c_long, db_gain: c_long, value: *mut c_long, xdir: c_int) -> c_int;
    pub fn snd_ctl_get_dB_range(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_ctl_convert_to_dB(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, volume: c_long, db_gain: *mut c_long) -> c_int;
    pub fn snd_ctl_convert_from_dB(ctl: *mut snd_ctl_t, id: *const snd_ctl_elem_id_t, db_gain: c_long, value: *mut c_long, xdir: c_int) -> c_int;
    pub fn snd_hctl_compare_fast(c1: *const snd_hctl_elem_t, c2: *const snd_hctl_elem_t) -> c_int;
    pub fn snd_hctl_open(hctl: *mut *mut snd_hctl_t, name: *const c_char, mode: c_int) -> c_int;
    pub fn snd_hctl_open_ctl(hctlp: *mut *mut snd_hctl_t, ctl: *mut snd_ctl_t) -> c_int;
    pub fn snd_hctl_close(hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_hctl_nonblock(hctl: *mut snd_hctl_t, nonblock: c_int) -> c_int;
    pub fn snd_hctl_poll_descriptors_count(hctl: *mut snd_hctl_t) -> c_int;
    //pub fn snd_hctl_poll_descriptors(hctl: *mut snd_hctl_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_hctl_poll_descriptors_revents(ctl: *mut snd_hctl_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_hctl_get_count(hctl: *mut snd_hctl_t) -> c_uint;
    pub fn snd_hctl_set_compare(hctl: *mut snd_hctl_t, hsort: snd_hctl_compare_t) -> c_int;
    pub fn snd_hctl_first_elem(hctl: *mut snd_hctl_t) -> *mut snd_hctl_elem_t;
    pub fn snd_hctl_last_elem(hctl: *mut snd_hctl_t) -> *mut snd_hctl_elem_t;
    pub fn snd_hctl_find_elem(hctl: *mut snd_hctl_t, id: *const snd_ctl_elem_id_t) -> *mut snd_hctl_elem_t;
    pub fn snd_hctl_set_callback(hctl: *mut snd_hctl_t, callback: snd_hctl_callback_t);
    pub fn snd_hctl_set_callback_private(hctl: *mut snd_hctl_t, data: *mut c_void);
    pub fn snd_hctl_get_callback_private(hctl: *mut snd_hctl_t) -> *mut c_void;
    pub fn snd_hctl_load(hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_hctl_free(hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_hctl_handle_events(hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_hctl_name(hctl: *mut snd_hctl_t) -> *const c_char;
    pub fn snd_hctl_wait(hctl: *mut snd_hctl_t, timeout: c_int) -> c_int;
    pub fn snd_hctl_ctl(hctl: *mut snd_hctl_t) -> *mut snd_ctl_t;
    pub fn snd_hctl_elem_next(elem: *mut snd_hctl_elem_t) -> *mut snd_hctl_elem_t;
    pub fn snd_hctl_elem_prev(elem: *mut snd_hctl_elem_t) -> *mut snd_hctl_elem_t;
    pub fn snd_hctl_elem_info(elem: *mut snd_hctl_elem_t, info: *mut snd_ctl_elem_info_t) -> c_int;
    pub fn snd_hctl_elem_read(elem: *mut snd_hctl_elem_t, value: *mut snd_ctl_elem_value_t) -> c_int;
    pub fn snd_hctl_elem_write(elem: *mut snd_hctl_elem_t, value: *mut snd_ctl_elem_value_t) -> c_int;
    pub fn snd_hctl_elem_tlv_read(elem: *mut snd_hctl_elem_t, tlv: *mut c_uint, tlv_size: c_uint) -> c_int;
    pub fn snd_hctl_elem_tlv_write(elem: *mut snd_hctl_elem_t, tlv: *const c_uint) -> c_int;
    pub fn snd_hctl_elem_tlv_command(elem: *mut snd_hctl_elem_t, tlv: *const c_uint) -> c_int;
    pub fn snd_hctl_elem_get_hctl(elem: *mut snd_hctl_elem_t) -> *mut snd_hctl_t;
    pub fn snd_hctl_elem_get_id(obj: *const snd_hctl_elem_t, ptr: *mut snd_ctl_elem_id_t);
    pub fn snd_hctl_elem_get_numid(obj: *const snd_hctl_elem_t) -> c_uint;
    pub fn snd_hctl_elem_get_interface(obj: *const snd_hctl_elem_t) -> snd_ctl_elem_iface_t;
    pub fn snd_hctl_elem_get_device(obj: *const snd_hctl_elem_t) -> c_uint;
    pub fn snd_hctl_elem_get_subdevice(obj: *const snd_hctl_elem_t) -> c_uint;
    pub fn snd_hctl_elem_get_name(obj: *const snd_hctl_elem_t) -> *const c_char;
    pub fn snd_hctl_elem_get_index(obj: *const snd_hctl_elem_t) -> c_uint;
    pub fn snd_hctl_elem_set_callback(obj: *mut snd_hctl_elem_t, val: snd_hctl_elem_callback_t);
    pub fn snd_hctl_elem_get_callback_private(obj: *const snd_hctl_elem_t) -> *mut c_void;
    pub fn snd_hctl_elem_set_callback_private(obj: *mut snd_hctl_elem_t, val: *mut c_void);
    pub fn snd_sctl_build(ctl: *mut *mut snd_sctl_t, handle: *mut snd_ctl_t, config: *mut snd_config_t, private_data: *mut snd_config_t, mode: c_int) -> c_int;
    pub fn snd_sctl_free(handle: *mut snd_sctl_t) -> c_int;
    pub fn snd_sctl_install(handle: *mut snd_sctl_t) -> c_int;
    pub fn snd_sctl_remove(handle: *mut snd_sctl_t) -> c_int;
    pub fn snd_mixer_open(mixer: *mut *mut snd_mixer_t, mode: c_int) -> c_int;
    pub fn snd_mixer_close(mixer: *mut snd_mixer_t) -> c_int;
    pub fn snd_mixer_first_elem(mixer: *mut snd_mixer_t) -> *mut snd_mixer_elem_t;
    pub fn snd_mixer_last_elem(mixer: *mut snd_mixer_t) -> *mut snd_mixer_elem_t;
    pub fn snd_mixer_handle_events(mixer: *mut snd_mixer_t) -> c_int;
    pub fn snd_mixer_attach(mixer: *mut snd_mixer_t, name: *const c_char) -> c_int;
    pub fn snd_mixer_attach_hctl(mixer: *mut snd_mixer_t, hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_mixer_detach(mixer: *mut snd_mixer_t, name: *const c_char) -> c_int;
    pub fn snd_mixer_detach_hctl(mixer: *mut snd_mixer_t, hctl: *mut snd_hctl_t) -> c_int;
    pub fn snd_mixer_get_hctl(mixer: *mut snd_mixer_t, name: *const c_char, hctl: *mut *mut snd_hctl_t) -> c_int;
    pub fn snd_mixer_poll_descriptors_count(mixer: *mut snd_mixer_t) -> c_int;
    //pub fn snd_mixer_poll_descriptors(mixer: *mut snd_mixer_t, pfds: *mut Struct_pollfd, space: c_uint) -> c_int;
    //pub fn snd_mixer_poll_descriptors_revents(mixer: *mut snd_mixer_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_mixer_load(mixer: *mut snd_mixer_t) -> c_int;
    pub fn snd_mixer_free(mixer: *mut snd_mixer_t);
    pub fn snd_mixer_wait(mixer: *mut snd_mixer_t, timeout: c_int) -> c_int;
    pub fn snd_mixer_set_compare(mixer: *mut snd_mixer_t, msort: snd_mixer_compare_t) -> c_int;
    pub fn snd_mixer_set_callback(obj: *mut snd_mixer_t, val: snd_mixer_callback_t);
    pub fn snd_mixer_get_callback_private(obj: *const snd_mixer_t) -> *mut c_void;
    pub fn snd_mixer_set_callback_private(obj: *mut snd_mixer_t, val: *mut c_void);
    pub fn snd_mixer_get_count(obj: *const snd_mixer_t) -> c_uint;
    pub fn snd_mixer_class_unregister(clss: *mut snd_mixer_class_t) -> c_int;
    pub fn snd_mixer_elem_next(elem: *mut snd_mixer_elem_t) -> *mut snd_mixer_elem_t;
    pub fn snd_mixer_elem_prev(elem: *mut snd_mixer_elem_t) -> *mut snd_mixer_elem_t;
    pub fn snd_mixer_elem_set_callback(obj: *mut snd_mixer_elem_t, val: snd_mixer_elem_callback_t);
    pub fn snd_mixer_elem_get_callback_private(obj: *const snd_mixer_elem_t) -> *mut c_void;
    pub fn snd_mixer_elem_set_callback_private(obj: *mut snd_mixer_elem_t, val: *mut c_void);
    pub fn snd_mixer_elem_get_type(obj: *const snd_mixer_elem_t) -> snd_mixer_elem_type_t;
    pub fn snd_mixer_class_register(class_: *mut snd_mixer_class_t, mixer: *mut snd_mixer_t) -> c_int;
    pub fn snd_mixer_elem_new(elem: *mut *mut snd_mixer_elem_t, _type: snd_mixer_elem_type_t, compare_weight: c_int, private_data: *mut c_void, private_free: ::std::option::Option<extern "C" fn (arg1: *mut snd_mixer_elem_t)>) -> c_int;
    pub fn snd_mixer_elem_add(elem: *mut snd_mixer_elem_t, class_: *mut snd_mixer_class_t) -> c_int;
    pub fn snd_mixer_elem_remove(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_elem_free(elem: *mut snd_mixer_elem_t);
    pub fn snd_mixer_elem_info(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_elem_value(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_elem_attach(melem: *mut snd_mixer_elem_t, helem: *mut snd_hctl_elem_t) -> c_int;
    pub fn snd_mixer_elem_detach(melem: *mut snd_mixer_elem_t, helem: *mut snd_hctl_elem_t) -> c_int;
    pub fn snd_mixer_elem_empty(melem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_elem_get_private(melem: *const snd_mixer_elem_t) -> *mut c_void;
    pub fn snd_mixer_class_sizeof() -> size_t;
    pub fn snd_mixer_class_malloc(ptr: *mut *mut snd_mixer_class_t) -> c_int;
    pub fn snd_mixer_class_free(obj: *mut snd_mixer_class_t);
    pub fn snd_mixer_class_copy(dst: *mut snd_mixer_class_t, src: *const snd_mixer_class_t);
    pub fn snd_mixer_class_get_mixer(class_: *const snd_mixer_class_t) -> *mut snd_mixer_t;
    pub fn snd_mixer_class_get_event(class_: *const snd_mixer_class_t) -> snd_mixer_event_t;
    pub fn snd_mixer_class_get_private(class_: *const snd_mixer_class_t) -> *mut c_void;
    pub fn snd_mixer_class_get_compare(class_: *const snd_mixer_class_t) -> snd_mixer_compare_t;
    pub fn snd_mixer_class_set_event(class_: *mut snd_mixer_class_t, event: snd_mixer_event_t) -> c_int;
    pub fn snd_mixer_class_set_private(class_: *mut snd_mixer_class_t, private_data: *mut c_void) -> c_int;
    pub fn snd_mixer_class_set_private_free(class_: *mut snd_mixer_class_t, private_free: ::std::option::Option<extern "C" fn (arg1: *mut snd_mixer_class_t)>) -> c_int;
    pub fn snd_mixer_class_set_compare(class_: *mut snd_mixer_class_t, compare: snd_mixer_compare_t) -> c_int;
    pub fn snd_mixer_selem_channel_name(channel: snd_mixer_selem_channel_id_t) -> *const c_char;
    pub fn snd_mixer_selem_register(mixer: *mut snd_mixer_t, options: *mut snd_mixer_selem_regopt, classp: *mut *mut snd_mixer_class_t) -> c_int;
    pub fn snd_mixer_selem_get_id(element: *mut snd_mixer_elem_t, id: *mut snd_mixer_selem_id_t);
    pub fn snd_mixer_selem_get_name(elem: *mut snd_mixer_elem_t) -> *const c_char;
    pub fn snd_mixer_selem_get_index(elem: *mut snd_mixer_elem_t) -> c_uint;
    pub fn snd_mixer_find_selem(mixer: *mut snd_mixer_t, id: *const snd_mixer_selem_id_t) -> *mut snd_mixer_elem_t;
    pub fn snd_mixer_selem_is_active(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_is_playback_mono(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_playback_channel(obj: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t) -> c_int;
    pub fn snd_mixer_selem_is_capture_mono(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_channel(obj: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t) -> c_int;
    pub fn snd_mixer_selem_get_capture_group(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_common_volume(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_playback_volume(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_playback_volume_joined(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_volume(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_volume_joined(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_common_switch(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_playback_switch(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_playback_switch_joined(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_switch(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_switch_joined(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_has_capture_switch_exclusive(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_ask_playback_vol_dB(elem: *mut snd_mixer_elem_t, value: c_long, dBvalue: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_ask_capture_vol_dB(elem: *mut snd_mixer_elem_t, value: c_long, dBvalue: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_ask_playback_dB_vol(elem: *mut snd_mixer_elem_t, dBvalue: c_long, dir: c_int, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_ask_capture_dB_vol(elem: *mut snd_mixer_elem_t, dBvalue: c_long, dir: c_int, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_playback_volume(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_capture_volume(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_playback_dB(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_capture_dB(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_playback_switch(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_int) -> c_int;
    pub fn snd_mixer_selem_get_capture_switch(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: *mut c_int) -> c_int;
    pub fn snd_mixer_selem_set_playback_volume(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_long) -> c_int;
    pub fn snd_mixer_selem_set_capture_volume(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_long) -> c_int;
    pub fn snd_mixer_selem_set_playback_dB(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_long, dir: c_int) -> c_int;
    pub fn snd_mixer_selem_set_capture_dB(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_long, dir: c_int) -> c_int;
    pub fn snd_mixer_selem_set_playback_volume_all(elem: *mut snd_mixer_elem_t, value: c_long) -> c_int;
    pub fn snd_mixer_selem_set_capture_volume_all(elem: *mut snd_mixer_elem_t, value: c_long) -> c_int;
    pub fn snd_mixer_selem_set_playback_dB_all(elem: *mut snd_mixer_elem_t, value: c_long, dir: c_int) -> c_int;
    pub fn snd_mixer_selem_set_capture_dB_all(elem: *mut snd_mixer_elem_t, value: c_long, dir: c_int) -> c_int;
    pub fn snd_mixer_selem_set_playback_switch(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_int) -> c_int;
    pub fn snd_mixer_selem_set_capture_switch(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, value: c_int) -> c_int;
    pub fn snd_mixer_selem_set_playback_switch_all(elem: *mut snd_mixer_elem_t, value: c_int) -> c_int;
    pub fn snd_mixer_selem_set_capture_switch_all(elem: *mut snd_mixer_elem_t, value: c_int) -> c_int;
    pub fn snd_mixer_selem_get_playback_volume_range(elem: *mut snd_mixer_elem_t, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_playback_dB_range(elem: *mut snd_mixer_elem_t, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_set_playback_volume_range(elem: *mut snd_mixer_elem_t, min: c_long, max: c_long) -> c_int;
    pub fn snd_mixer_selem_get_capture_volume_range(elem: *mut snd_mixer_elem_t, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_get_capture_dB_range(elem: *mut snd_mixer_elem_t, min: *mut c_long, max: *mut c_long) -> c_int;
    pub fn snd_mixer_selem_set_capture_volume_range(elem: *mut snd_mixer_elem_t, min: c_long, max: c_long) -> c_int;
    pub fn snd_mixer_selem_is_enumerated(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_is_enum_playback(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_is_enum_capture(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_get_enum_items(elem: *mut snd_mixer_elem_t) -> c_int;
    pub fn snd_mixer_selem_get_enum_item_name(elem: *mut snd_mixer_elem_t, idx: c_uint, maxlen: size_t, str: *mut c_char) -> c_int;
    pub fn snd_mixer_selem_get_enum_item(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, idxp: *mut c_uint) -> c_int;
    pub fn snd_mixer_selem_set_enum_item(elem: *mut snd_mixer_elem_t, channel: snd_mixer_selem_channel_id_t, idx: c_uint) -> c_int;
    pub fn snd_mixer_selem_id_sizeof() -> size_t;
    pub fn snd_mixer_selem_id_malloc(ptr: *mut *mut snd_mixer_selem_id_t) -> c_int;
    pub fn snd_mixer_selem_id_free(obj: *mut snd_mixer_selem_id_t);
    pub fn snd_mixer_selem_id_copy(dst: *mut snd_mixer_selem_id_t, src: *const snd_mixer_selem_id_t);
    pub fn snd_mixer_selem_id_get_name(obj: *const snd_mixer_selem_id_t) -> *const c_char;
    pub fn snd_mixer_selem_id_get_index(obj: *const snd_mixer_selem_id_t) -> c_uint;
    pub fn snd_mixer_selem_id_set_name(obj: *mut snd_mixer_selem_id_t, val: *const c_char);
    pub fn snd_mixer_selem_id_set_index(obj: *mut snd_mixer_selem_id_t, val: c_uint);
    pub fn snd_seq_open(handle: *mut *mut snd_seq_t, name: *const c_char, streams: c_int, mode: c_int) -> c_int;
    pub fn snd_seq_open_lconf(handle: *mut *mut snd_seq_t, name: *const c_char, streams: c_int, mode: c_int, lconf: *mut snd_config_t) -> c_int;
    pub fn snd_seq_name(seq: *mut snd_seq_t) -> *const c_char;
    pub fn snd_seq_type(seq: *mut snd_seq_t) -> snd_seq_type_t;
    pub fn snd_seq_close(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_poll_descriptors_count(handle: *mut snd_seq_t, events: c_short) -> c_int;
    //pub fn snd_seq_poll_descriptors(handle: *mut snd_seq_t, pfds: *mut Struct_pollfd, space: c_uint, events: c_short) -> c_int;
    //pub fn snd_seq_poll_descriptors_revents(seq: *mut snd_seq_t, pfds: *mut Struct_pollfd, nfds: c_uint, revents: *mut c_ushort) -> c_int;
    pub fn snd_seq_nonblock(handle: *mut snd_seq_t, nonblock: c_int) -> c_int;
    pub fn snd_seq_client_id(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_get_output_buffer_size(handle: *mut snd_seq_t) -> size_t;
    pub fn snd_seq_get_input_buffer_size(handle: *mut snd_seq_t) -> size_t;
    pub fn snd_seq_set_output_buffer_size(handle: *mut snd_seq_t, size: size_t) -> c_int;
    pub fn snd_seq_set_input_buffer_size(handle: *mut snd_seq_t, size: size_t) -> c_int;
    pub fn snd_seq_system_info_sizeof() -> size_t;
    pub fn snd_seq_system_info_malloc(ptr: *mut *mut snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_free(ptr: *mut snd_seq_system_info_t);
    pub fn snd_seq_system_info_copy(dst: *mut snd_seq_system_info_t, src: *const snd_seq_system_info_t);
    pub fn snd_seq_system_info_get_queues(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_get_clients(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_get_ports(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_get_channels(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_get_cur_clients(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info_get_cur_queues(info: *const snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_system_info(handle: *mut snd_seq_t, info: *mut snd_seq_system_info_t) -> c_int;
    pub fn snd_seq_client_info_sizeof() -> size_t;
    pub fn snd_seq_client_info_malloc(ptr: *mut *mut snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_free(ptr: *mut snd_seq_client_info_t);
    pub fn snd_seq_client_info_copy(dst: *mut snd_seq_client_info_t, src: *const snd_seq_client_info_t);
    pub fn snd_seq_client_info_get_client(info: *const snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_get_type(info: *const snd_seq_client_info_t) -> snd_seq_client_type_t;
    pub fn snd_seq_client_info_get_name(info: *mut snd_seq_client_info_t) -> *const c_char;
    pub fn snd_seq_client_info_get_broadcast_filter(info: *const snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_get_error_bounce(info: *const snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_get_event_filter(info: *const snd_seq_client_info_t) -> *const c_uchar;
    pub fn snd_seq_client_info_get_num_ports(info: *const snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_get_event_lost(info: *const snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_info_set_client(info: *mut snd_seq_client_info_t, client: c_int);
    pub fn snd_seq_client_info_set_name(info: *mut snd_seq_client_info_t, name: *const c_char);
    pub fn snd_seq_client_info_set_broadcast_filter(info: *mut snd_seq_client_info_t, val: c_int);
    pub fn snd_seq_client_info_set_error_bounce(info: *mut snd_seq_client_info_t, val: c_int);
    pub fn snd_seq_client_info_set_event_filter(info: *mut snd_seq_client_info_t, filter: *mut c_uchar);
    pub fn snd_seq_client_info_event_filter_clear(info: *mut snd_seq_client_info_t);
    pub fn snd_seq_client_info_event_filter_add(info: *mut snd_seq_client_info_t, event_type: c_int);
    pub fn snd_seq_client_info_event_filter_del(info: *mut snd_seq_client_info_t, event_type: c_int);
    pub fn snd_seq_client_info_event_filter_check(info: *mut snd_seq_client_info_t, event_type: c_int) -> c_int;
    pub fn snd_seq_get_client_info(handle: *mut snd_seq_t, info: *mut snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_get_any_client_info(handle: *mut snd_seq_t, client: c_int, info: *mut snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_set_client_info(handle: *mut snd_seq_t, info: *mut snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_query_next_client(handle: *mut snd_seq_t, info: *mut snd_seq_client_info_t) -> c_int;
    pub fn snd_seq_client_pool_sizeof() -> size_t;
    pub fn snd_seq_client_pool_malloc(ptr: *mut *mut snd_seq_client_pool_t) -> c_int;
    pub fn snd_seq_client_pool_free(ptr: *mut snd_seq_client_pool_t);
    pub fn snd_seq_client_pool_copy(dst: *mut snd_seq_client_pool_t, src: *const snd_seq_client_pool_t);
    pub fn snd_seq_client_pool_get_client(info: *const snd_seq_client_pool_t) -> c_int;
    pub fn snd_seq_client_pool_get_output_pool(info: *const snd_seq_client_pool_t) -> size_t;
    pub fn snd_seq_client_pool_get_input_pool(info: *const snd_seq_client_pool_t) -> size_t;
    pub fn snd_seq_client_pool_get_output_room(info: *const snd_seq_client_pool_t) -> size_t;
    pub fn snd_seq_client_pool_get_output_free(info: *const snd_seq_client_pool_t) -> size_t;
    pub fn snd_seq_client_pool_get_input_free(info: *const snd_seq_client_pool_t) -> size_t;
    pub fn snd_seq_client_pool_set_output_pool(info: *mut snd_seq_client_pool_t, size: size_t);
    pub fn snd_seq_client_pool_set_input_pool(info: *mut snd_seq_client_pool_t, size: size_t);
    pub fn snd_seq_client_pool_set_output_room(info: *mut snd_seq_client_pool_t, size: size_t);
    pub fn snd_seq_get_client_pool(handle: *mut snd_seq_t, info: *mut snd_seq_client_pool_t) -> c_int;
    pub fn snd_seq_set_client_pool(handle: *mut snd_seq_t, info: *mut snd_seq_client_pool_t) -> c_int;
    pub fn snd_seq_port_info_sizeof() -> size_t;
    pub fn snd_seq_port_info_malloc(ptr: *mut *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_free(ptr: *mut snd_seq_port_info_t);
    pub fn snd_seq_port_info_copy(dst: *mut snd_seq_port_info_t, src: *const snd_seq_port_info_t);
    pub fn snd_seq_port_info_get_client(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_port(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_addr(info: *const snd_seq_port_info_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_port_info_get_name(info: *const snd_seq_port_info_t) -> *const c_char;
    pub fn snd_seq_port_info_get_capability(info: *const snd_seq_port_info_t) -> c_uint;
    pub fn snd_seq_port_info_get_type(info: *const snd_seq_port_info_t) -> c_uint;
    pub fn snd_seq_port_info_get_midi_channels(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_midi_voices(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_synth_voices(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_read_use(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_write_use(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_port_specified(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_timestamping(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_timestamp_real(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_get_timestamp_queue(info: *const snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_info_set_client(info: *mut snd_seq_port_info_t, client: c_int);
    pub fn snd_seq_port_info_set_port(info: *mut snd_seq_port_info_t, port: c_int);
    pub fn snd_seq_port_info_set_addr(info: *mut snd_seq_port_info_t, addr: *const snd_seq_addr_t);
    pub fn snd_seq_port_info_set_name(info: *mut snd_seq_port_info_t, name: *const c_char);
    pub fn snd_seq_port_info_set_capability(info: *mut snd_seq_port_info_t, capability: c_uint);
    pub fn snd_seq_port_info_set_type(info: *mut snd_seq_port_info_t, _type: c_uint);
    pub fn snd_seq_port_info_set_midi_channels(info: *mut snd_seq_port_info_t, channels: c_int);
    pub fn snd_seq_port_info_set_midi_voices(info: *mut snd_seq_port_info_t, voices: c_int);
    pub fn snd_seq_port_info_set_synth_voices(info: *mut snd_seq_port_info_t, voices: c_int);
    pub fn snd_seq_port_info_set_port_specified(info: *mut snd_seq_port_info_t, val: c_int);
    pub fn snd_seq_port_info_set_timestamping(info: *mut snd_seq_port_info_t, enable: c_int);
    pub fn snd_seq_port_info_set_timestamp_real(info: *mut snd_seq_port_info_t, realtime: c_int);
    pub fn snd_seq_port_info_set_timestamp_queue(info: *mut snd_seq_port_info_t, queue: c_int);
    pub fn snd_seq_create_port(handle: *mut snd_seq_t, info: *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_delete_port(handle: *mut snd_seq_t, port: c_int) -> c_int;
    pub fn snd_seq_get_port_info(handle: *mut snd_seq_t, port: c_int, info: *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_get_any_port_info(handle: *mut snd_seq_t, client: c_int, port: c_int, info: *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_set_port_info(handle: *mut snd_seq_t, port: c_int, info: *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_query_next_port(handle: *mut snd_seq_t, info: *mut snd_seq_port_info_t) -> c_int;
    pub fn snd_seq_port_subscribe_sizeof() -> size_t;
    pub fn snd_seq_port_subscribe_malloc(ptr: *mut *mut snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_port_subscribe_free(ptr: *mut snd_seq_port_subscribe_t);
    pub fn snd_seq_port_subscribe_copy(dst: *mut snd_seq_port_subscribe_t, src: *const snd_seq_port_subscribe_t);
    pub fn snd_seq_port_subscribe_get_sender(info: *const snd_seq_port_subscribe_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_port_subscribe_get_dest(info: *const snd_seq_port_subscribe_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_port_subscribe_get_queue(info: *const snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_port_subscribe_get_exclusive(info: *const snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_port_subscribe_get_time_update(info: *const snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_port_subscribe_get_time_real(info: *const snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_port_subscribe_set_sender(info: *mut snd_seq_port_subscribe_t, addr: *const snd_seq_addr_t);
    pub fn snd_seq_port_subscribe_set_dest(info: *mut snd_seq_port_subscribe_t, addr: *const snd_seq_addr_t);
    pub fn snd_seq_port_subscribe_set_queue(info: *mut snd_seq_port_subscribe_t, q: c_int);
    pub fn snd_seq_port_subscribe_set_exclusive(info: *mut snd_seq_port_subscribe_t, val: c_int);
    pub fn snd_seq_port_subscribe_set_time_update(info: *mut snd_seq_port_subscribe_t, val: c_int);
    pub fn snd_seq_port_subscribe_set_time_real(info: *mut snd_seq_port_subscribe_t, val: c_int);
    pub fn snd_seq_get_port_subscription(handle: *mut snd_seq_t, sub: *mut snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_subscribe_port(handle: *mut snd_seq_t, sub: *mut snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_unsubscribe_port(handle: *mut snd_seq_t, sub: *mut snd_seq_port_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_sizeof() -> size_t;
    pub fn snd_seq_query_subscribe_malloc(ptr: *mut *mut snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_free(ptr: *mut snd_seq_query_subscribe_t);
    pub fn snd_seq_query_subscribe_copy(dst: *mut snd_seq_query_subscribe_t, src: *const snd_seq_query_subscribe_t);
    pub fn snd_seq_query_subscribe_get_client(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_port(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_root(info: *const snd_seq_query_subscribe_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_query_subscribe_get_type(info: *const snd_seq_query_subscribe_t) -> snd_seq_query_subs_type_t;
    pub fn snd_seq_query_subscribe_get_index(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_num_subs(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_addr(info: *const snd_seq_query_subscribe_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_query_subscribe_get_queue(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_exclusive(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_time_update(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_get_time_real(info: *const snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_query_subscribe_set_client(info: *mut snd_seq_query_subscribe_t, client: c_int);
    pub fn snd_seq_query_subscribe_set_port(info: *mut snd_seq_query_subscribe_t, port: c_int);
    pub fn snd_seq_query_subscribe_set_root(info: *mut snd_seq_query_subscribe_t, addr: *const snd_seq_addr_t);
    pub fn snd_seq_query_subscribe_set_type(info: *mut snd_seq_query_subscribe_t, _type: snd_seq_query_subs_type_t);
    pub fn snd_seq_query_subscribe_set_index(info: *mut snd_seq_query_subscribe_t, _index: c_int);
    pub fn snd_seq_query_port_subscribers(seq: *mut snd_seq_t, subs: *mut snd_seq_query_subscribe_t) -> c_int;
    pub fn snd_seq_queue_info_sizeof() -> size_t;
    pub fn snd_seq_queue_info_malloc(ptr: *mut *mut snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_queue_info_free(ptr: *mut snd_seq_queue_info_t);
    pub fn snd_seq_queue_info_copy(dst: *mut snd_seq_queue_info_t, src: *const snd_seq_queue_info_t);
    pub fn snd_seq_queue_info_get_queue(info: *const snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_queue_info_get_name(info: *const snd_seq_queue_info_t) -> *const c_char;
    pub fn snd_seq_queue_info_get_owner(info: *const snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_queue_info_get_locked(info: *const snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_queue_info_get_flags(info: *const snd_seq_queue_info_t) -> c_uint;
    pub fn snd_seq_queue_info_set_name(info: *mut snd_seq_queue_info_t, name: *const c_char);
    pub fn snd_seq_queue_info_set_owner(info: *mut snd_seq_queue_info_t, owner: c_int);
    pub fn snd_seq_queue_info_set_locked(info: *mut snd_seq_queue_info_t, locked: c_int);
    pub fn snd_seq_queue_info_set_flags(info: *mut snd_seq_queue_info_t, flags: c_uint);
    pub fn snd_seq_create_queue(seq: *mut snd_seq_t, info: *mut snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_alloc_named_queue(seq: *mut snd_seq_t, name: *const c_char) -> c_int;
    pub fn snd_seq_alloc_queue(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_free_queue(handle: *mut snd_seq_t, q: c_int) -> c_int;
    pub fn snd_seq_get_queue_info(seq: *mut snd_seq_t, q: c_int, info: *mut snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_set_queue_info(seq: *mut snd_seq_t, q: c_int, info: *mut snd_seq_queue_info_t) -> c_int;
    pub fn snd_seq_query_named_queue(seq: *mut snd_seq_t, name: *const c_char) -> c_int;
    pub fn snd_seq_get_queue_usage(handle: *mut snd_seq_t, q: c_int) -> c_int;
    pub fn snd_seq_set_queue_usage(handle: *mut snd_seq_t, q: c_int, used: c_int) -> c_int;
    pub fn snd_seq_queue_status_sizeof() -> size_t;
    pub fn snd_seq_queue_status_malloc(ptr: *mut *mut snd_seq_queue_status_t) -> c_int;
    pub fn snd_seq_queue_status_free(ptr: *mut snd_seq_queue_status_t);
    pub fn snd_seq_queue_status_copy(dst: *mut snd_seq_queue_status_t, src: *const snd_seq_queue_status_t);
    pub fn snd_seq_queue_status_get_queue(info: *const snd_seq_queue_status_t) -> c_int;
    pub fn snd_seq_queue_status_get_events(info: *const snd_seq_queue_status_t) -> c_int;
    pub fn snd_seq_queue_status_get_tick_time(info: *const snd_seq_queue_status_t) -> snd_seq_tick_time_t;
    pub fn snd_seq_queue_status_get_real_time(info: *const snd_seq_queue_status_t) -> *const snd_seq_real_time_t;
    pub fn snd_seq_queue_status_get_status(info: *const snd_seq_queue_status_t) -> c_uint;
    pub fn snd_seq_get_queue_status(handle: *mut snd_seq_t, q: c_int, status: *mut snd_seq_queue_status_t) -> c_int;
    pub fn snd_seq_queue_tempo_sizeof() -> size_t;
    pub fn snd_seq_queue_tempo_malloc(ptr: *mut *mut snd_seq_queue_tempo_t) -> c_int;
    pub fn snd_seq_queue_tempo_free(ptr: *mut snd_seq_queue_tempo_t);
    pub fn snd_seq_queue_tempo_copy(dst: *mut snd_seq_queue_tempo_t, src: *const snd_seq_queue_tempo_t);
    pub fn snd_seq_queue_tempo_get_queue(info: *const snd_seq_queue_tempo_t) -> c_int;
    pub fn snd_seq_queue_tempo_get_tempo(info: *const snd_seq_queue_tempo_t) -> c_uint;
    pub fn snd_seq_queue_tempo_get_ppq(info: *const snd_seq_queue_tempo_t) -> c_int;
    pub fn snd_seq_queue_tempo_get_skew(info: *const snd_seq_queue_tempo_t) -> c_uint;
    pub fn snd_seq_queue_tempo_get_skew_base(info: *const snd_seq_queue_tempo_t) -> c_uint;
    pub fn snd_seq_queue_tempo_set_tempo(info: *mut snd_seq_queue_tempo_t, tempo: c_uint);
    pub fn snd_seq_queue_tempo_set_ppq(info: *mut snd_seq_queue_tempo_t, ppq: c_int);
    pub fn snd_seq_queue_tempo_set_skew(info: *mut snd_seq_queue_tempo_t, skew: c_uint);
    pub fn snd_seq_queue_tempo_set_skew_base(info: *mut snd_seq_queue_tempo_t, base: c_uint);
    pub fn snd_seq_get_queue_tempo(handle: *mut snd_seq_t, q: c_int, tempo: *mut snd_seq_queue_tempo_t) -> c_int;
    pub fn snd_seq_set_queue_tempo(handle: *mut snd_seq_t, q: c_int, tempo: *mut snd_seq_queue_tempo_t) -> c_int;
    pub fn snd_seq_queue_timer_sizeof() -> size_t;
    pub fn snd_seq_queue_timer_malloc(ptr: *mut *mut snd_seq_queue_timer_t) -> c_int;
    pub fn snd_seq_queue_timer_free(ptr: *mut snd_seq_queue_timer_t);
    pub fn snd_seq_queue_timer_copy(dst: *mut snd_seq_queue_timer_t, src: *const snd_seq_queue_timer_t);
    pub fn snd_seq_queue_timer_get_queue(info: *const snd_seq_queue_timer_t) -> c_int;
    pub fn snd_seq_queue_timer_get_type(info: *const snd_seq_queue_timer_t) -> snd_seq_queue_timer_type_t;
    pub fn snd_seq_queue_timer_get_id(info: *const snd_seq_queue_timer_t) -> *const snd_timer_id_t;
    pub fn snd_seq_queue_timer_get_resolution(info: *const snd_seq_queue_timer_t) -> c_uint;
    pub fn snd_seq_queue_timer_set_type(info: *mut snd_seq_queue_timer_t, _type: snd_seq_queue_timer_type_t);
    pub fn snd_seq_queue_timer_set_id(info: *mut snd_seq_queue_timer_t, id: *const snd_timer_id_t);
    pub fn snd_seq_queue_timer_set_resolution(info: *mut snd_seq_queue_timer_t, resolution: c_uint);
    pub fn snd_seq_get_queue_timer(handle: *mut snd_seq_t, q: c_int, timer: *mut snd_seq_queue_timer_t) -> c_int;
    pub fn snd_seq_set_queue_timer(handle: *mut snd_seq_t, q: c_int, timer: *mut snd_seq_queue_timer_t) -> c_int;
    pub fn snd_seq_free_event(ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_event_length(ev: *mut snd_seq_event_t) -> ssize_t;
    pub fn snd_seq_event_output(handle: *mut snd_seq_t, ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_event_output_buffer(handle: *mut snd_seq_t, ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_event_output_direct(handle: *mut snd_seq_t, ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_event_input(handle: *mut snd_seq_t, ev: *mut *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_event_input_pending(seq: *mut snd_seq_t, fetch_sequencer: c_int) -> c_int;
    pub fn snd_seq_drain_output(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_event_output_pending(seq: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_extract_output(handle: *mut snd_seq_t, ev: *mut *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_drop_output(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_drop_output_buffer(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_drop_input(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_drop_input_buffer(handle: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_remove_events_sizeof() -> size_t;
    pub fn snd_seq_remove_events_malloc(ptr: *mut *mut snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_remove_events_free(ptr: *mut snd_seq_remove_events_t);
    pub fn snd_seq_remove_events_copy(dst: *mut snd_seq_remove_events_t, src: *const snd_seq_remove_events_t);
    pub fn snd_seq_remove_events_get_condition(info: *const snd_seq_remove_events_t) -> c_uint;
    pub fn snd_seq_remove_events_get_queue(info: *const snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_remove_events_get_time(info: *const snd_seq_remove_events_t) -> *const snd_seq_timestamp_t;
    pub fn snd_seq_remove_events_get_dest(info: *const snd_seq_remove_events_t) -> *const snd_seq_addr_t;
    pub fn snd_seq_remove_events_get_channel(info: *const snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_remove_events_get_event_type(info: *const snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_remove_events_get_tag(info: *const snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_remove_events_set_condition(info: *mut snd_seq_remove_events_t, flags: c_uint);
    pub fn snd_seq_remove_events_set_queue(info: *mut snd_seq_remove_events_t, queue: c_int);
    pub fn snd_seq_remove_events_set_time(info: *mut snd_seq_remove_events_t, time: *const snd_seq_timestamp_t);
    pub fn snd_seq_remove_events_set_dest(info: *mut snd_seq_remove_events_t, addr: *const snd_seq_addr_t);
    pub fn snd_seq_remove_events_set_channel(info: *mut snd_seq_remove_events_t, channel: c_int);
    pub fn snd_seq_remove_events_set_event_type(info: *mut snd_seq_remove_events_t, _type: c_int);
    pub fn snd_seq_remove_events_set_tag(info: *mut snd_seq_remove_events_t, tag: c_int);
    pub fn snd_seq_remove_events(handle: *mut snd_seq_t, info: *mut snd_seq_remove_events_t) -> c_int;
    pub fn snd_seq_set_bit(nr: c_int, array: *mut c_void);
    pub fn snd_seq_unset_bit(nr: c_int, array: *mut c_void);
    pub fn snd_seq_change_bit(nr: c_int, array: *mut c_void) -> c_int;
    pub fn snd_seq_get_bit(nr: c_int, array: *mut c_void) -> c_int;
    pub fn snd_seq_control_queue(seq: *mut snd_seq_t, q: c_int, _type: c_int, value: c_int, ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_seq_create_simple_port(seq: *mut snd_seq_t, name: *const c_char, caps: c_uint, _type: c_uint) -> c_int;
    pub fn snd_seq_delete_simple_port(seq: *mut snd_seq_t, port: c_int) -> c_int;
    pub fn snd_seq_connect_from(seq: *mut snd_seq_t, my_port: c_int, src_client: c_int, src_port: c_int) -> c_int;
    pub fn snd_seq_connect_to(seq: *mut snd_seq_t, my_port: c_int, dest_client: c_int, dest_port: c_int) -> c_int;
    pub fn snd_seq_disconnect_from(seq: *mut snd_seq_t, my_port: c_int, src_client: c_int, src_port: c_int) -> c_int;
    pub fn snd_seq_disconnect_to(seq: *mut snd_seq_t, my_port: c_int, dest_client: c_int, dest_port: c_int) -> c_int;
    pub fn snd_seq_set_client_name(seq: *mut snd_seq_t, name: *const c_char) -> c_int;
    pub fn snd_seq_set_client_event_filter(seq: *mut snd_seq_t, event_type: c_int) -> c_int;
    pub fn snd_seq_set_client_pool_output(seq: *mut snd_seq_t, size: size_t) -> c_int;
    pub fn snd_seq_set_client_pool_output_room(seq: *mut snd_seq_t, size: size_t) -> c_int;
    pub fn snd_seq_set_client_pool_input(seq: *mut snd_seq_t, size: size_t) -> c_int;
    pub fn snd_seq_sync_output_queue(seq: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_parse_address(seq: *mut snd_seq_t, addr: *mut snd_seq_addr_t, str: *const c_char) -> c_int;
    pub fn snd_seq_reset_pool_output(seq: *mut snd_seq_t) -> c_int;
    pub fn snd_seq_reset_pool_input(seq: *mut snd_seq_t) -> c_int;
    pub fn snd_midi_event_new(bufsize: size_t, rdev: *mut *mut snd_midi_event_t) -> c_int;
    pub fn snd_midi_event_resize_buffer(dev: *mut snd_midi_event_t, bufsize: size_t) -> c_int;
    pub fn snd_midi_event_free(dev: *mut snd_midi_event_t);
    pub fn snd_midi_event_init(dev: *mut snd_midi_event_t);
    pub fn snd_midi_event_reset_encode(dev: *mut snd_midi_event_t);
    pub fn snd_midi_event_reset_decode(dev: *mut snd_midi_event_t);
    pub fn snd_midi_event_no_status(dev: *mut snd_midi_event_t, on: c_int);
    pub fn snd_midi_event_encode(dev: *mut snd_midi_event_t, buf: *const c_uchar, count: c_long, ev: *mut snd_seq_event_t) -> c_long;
    pub fn snd_midi_event_encode_byte(dev: *mut snd_midi_event_t, c: c_int, ev: *mut snd_seq_event_t) -> c_int;
    pub fn snd_midi_event_decode(dev: *mut snd_midi_event_t, buf: *mut c_uchar, count: c_long, ev: *const snd_seq_event_t) -> c_long;
}
