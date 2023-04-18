#![allow(non_snake_case)]

use super::*;
use windows_sys::Win32::Media::Audio::{EDataFlow, ERole, AUDCLNT_SHAREMODE, WAVEFORMATEX};
use windows_sys::Win32::System::Com;

type BOOL = i32;
type HANDLE = isize;

pub trait AudioInterface {
    const IID: GUID;
}

#[repr(C)]
struct IAudioRenderClientV {
    base: IUnknownV,
    get_buffer: unsafe extern "system" fn(
        this: *mut c_void,
        numframesrequested: u32,
        ppdata: *mut *mut u8,
    ) -> HRESULT,
    release_buffer: unsafe extern "system" fn(
        this: *mut c_void,
        numframeswritten: u32,
        dwflags: u32,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IAudioRenderClient(Object<IAudioRenderClientV>);

impl AudioInterface for IAudioRenderClient {
    const IID: GUID = GUID::from_u128(0xf294acfc_3146_4483_a7bf_addca7c260e2);
}

impl IAudioRenderClient {
    #[inline]
    pub unsafe fn GetBuffer(&self, available_frames: u32) -> ComResult<*mut u8> {
        let mut out = std::ptr::null_mut();
        let res =
            unsafe { (self.0.vtbl().get_buffer)(self.0 .0.cast(), available_frames, &mut out) };

        if res >= 0 {
            Ok(out)
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn ReleaseBuffer(&self, frames: u32, flags: u32) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().release_buffer)(self.0 .0.cast(), frames, flags) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
struct IAudioCaptureClientV {
    base: IUnknownV,
    get_buffer: unsafe extern "system" fn(
        this: *mut c_void,
        ppdata: *mut *mut u8,
        pnumframestoread: *mut u32,
        pdwflags: *mut u32,
        pu64deviceposition: Option<*mut u64>,
        pu64qpcposition: Option<*mut u64>,
    ) -> HRESULT,
    release_buffer: unsafe extern "system" fn(this: *mut c_void, numframesread: u32) -> HRESULT,
    get_next_packet_size:
        unsafe extern "system" fn(this: *mut c_void, pnumframesinnextpacket: *mut u32) -> HRESULT,
}

#[repr(transparent)]
pub struct IAudioCaptureClient(Object<IAudioCaptureClientV>);

impl AudioInterface for IAudioCaptureClient {
    const IID: GUID = GUID::from_u128(0xc8adbd64_e71e_48a0_a4de_185c395cd317);
}

impl IAudioCaptureClient {
    #[inline]
    pub unsafe fn GetNextPacketSize(&self) -> ComResult<u32> {
        let mut num = 0;
        let res = unsafe { (self.0.vtbl().get_next_packet_size)(self.0 .0.cast(), &mut num) };

        if res >= 0 {
            Ok(num)
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetBuffer(
        &self,
        data: *mut *mut u8,
        frames_stored: *mut u32,
        flags: *mut u32,
        device_position: Option<*mut u64>,
        qpc_position: Option<*mut u64>,
    ) -> ComResult<()> {
        let res = unsafe {
            (self.0.vtbl().get_buffer)(
                self.0 .0.cast(),
                data,
                frames_stored,
                flags,
                device_position,
                qpc_position,
            )
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn ReleaseBuffer(&self, frames_read: u32) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().release_buffer)(self.0 .0.cast(), frames_read) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
struct IAudioClientV {
    base: IUnknownV,
    initialize: unsafe extern "system" fn(
        this: *mut c_void,
        sharemode: AUDCLNT_SHAREMODE,
        streamflags: u32,
        hnsbufferduration: i64,
        hnsperiodicity: i64,
        pformat: *const WAVEFORMATEX,
        audiosessionguid: Option<*const GUID>,
    ) -> HRESULT,
    get_buffer_size:
        unsafe extern "system" fn(this: *mut c_void, pnumbufferframes: *mut u32) -> HRESULT,
    __GetStreamLatency: usize,
    get_current_padding:
        unsafe extern "system" fn(this: *mut c_void, pnumpaddingframes: *mut u32) -> HRESULT,
    is_format_supported: unsafe extern "system" fn(
        this: *mut c_void,
        sharemode: AUDCLNT_SHAREMODE,
        pformat: *const WAVEFORMATEX,
        ppclosestmatch: Option<*mut *mut WAVEFORMATEX>,
    ) -> HRESULT,
    get_mix_format: unsafe extern "system" fn(
        this: *mut c_void,
        ppdeviceformat: *mut *mut WAVEFORMATEX,
    ) -> HRESULT,
    __GetDevicePeriod: usize,
    start: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    stop: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    __Reset: usize,
    set_event_handle: unsafe extern "system" fn(this: *mut c_void, eventhandle: HANDLE) -> HRESULT,
    get_service: unsafe extern "system" fn(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
}

#[derive(Clone)]
#[repr(transparent)]
pub struct IAudioClient(Object<IAudioClientV>);

impl AudioInterface for IAudioClient {
    const IID: GUID = GUID::from_u128(0x1cb9ad4c_dbfa_4c32_b178_c2f568a703b2);
}

impl IAudioClient {
    #[inline]
    pub unsafe fn Initialize(
        &self,
        share_mode: AUDCLNT_SHAREMODE,
        stream_flags: u32,
        buffer_duration: i64,
        periodicity: i64,
        format: *const WAVEFORMATEX,
        session: Option<*const GUID>,
    ) -> ComResult<()> {
        let res = unsafe {
            (self.0.vtbl().initialize)(
                self.0 .0.cast(),
                share_mode,
                stream_flags,
                buffer_duration,
                periodicity,
                format,
                session,
            )
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetService<A: AudioInterface>(&self) -> ComResult<A> {
        let mut out = std::mem::MaybeUninit::<A>::uninit();
        let res = unsafe {
            (self.0.vtbl().get_service)(self.0 .0.cast(), &A::IID, out.as_mut_ptr().cast())
        };

        if res >= 0 {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetBufferSize(&self) -> ComResult<u32> {
        let mut out = 0;
        let res = unsafe { (self.0.vtbl().get_buffer_size)(self.0 .0.cast(), &mut out) };

        if res >= 0 {
            Ok(out)
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetCurrentPadding(&self) -> ComResult<u32> {
        let mut out = 0;
        let res = unsafe { (self.0.vtbl().get_current_padding)(self.0 .0.cast(), &mut out) };

        if res >= 0 {
            Ok(out)
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn SetEventHandle(&self, handle: HANDLE) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().set_event_handle)(self.0 .0.cast(), handle) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetMixFormat(&self) -> ComResult<*mut WAVEFORMATEX> {
        let mut out = std::mem::MaybeUninit::uninit();
        let res = unsafe { (self.0.vtbl().get_mix_format)(self.0 .0.cast(), out.as_mut_ptr()) };

        if res >= 0 {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn IsFormatSupported(
        &self,
        share_mode: AUDCLNT_SHAREMODE,
        format: *const WAVEFORMATEX,
        closest_match: Option<*mut *mut WAVEFORMATEX>,
    ) -> HRESULT {
        unsafe {
            (self.0.vtbl().is_format_supported)(self.0 .0.cast(), share_mode, format, closest_match)
        }
    }

    #[inline]
    pub unsafe fn Start(&self) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().start)(self.0 .0.cast()) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn Stop(&self) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().stop)(self.0 .0.cast()) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn cast<T: AudioInterface>(&self) -> ComResult<T> {
        super::query_interface(self.0.ptr().cast(), &T::IID)
    }
}

#[repr(C)]
struct IAudioClient2V {
    base: IAudioClientV,
    __IsOffloadCapable: usize,
    __SetClientProperties: usize,
    get_buffer_size_limits: unsafe extern "system" fn(
        this: *mut c_void,
        pformat: *const WAVEFORMATEX,
        beventdriven: BOOL,
        phnsminbufferduration: *mut i64,
        phnsmaxbufferduration: *mut i64,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IAudioClient2(Object<IAudioClient2V>);

impl AudioInterface for IAudioClient2 {
    const IID: GUID = GUID::from_u128(0x726778cd_f60a_4eda_82de_e47610cd78aa);
}

impl IAudioClient2 {
    #[inline]
    pub unsafe fn GetBufferSizeLimits(
        &self,
        format: *const WAVEFORMATEX,
        event_driven: bool,
        min_buffer_dur: *mut i64,
        max_buffer_dur: *mut i64,
    ) -> ComResult<()> {
        let res = unsafe {
            (self.0.vtbl().get_buffer_size_limits)(
                self.0 .0.cast(),
                format,
                event_driven as _,
                min_buffer_dur,
                max_buffer_dur,
            )
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
pub struct IAudioClockV {
    base: IUnknownV,
    __GetFrequency: usize,
    get_position: unsafe extern "system" fn(
        this: *mut c_void,
        pu64position: *mut u64,
        pu64qpcposition: Option<*mut u64>,
    ) -> HRESULT,
    __GetCharacteristics: usize,
}

#[repr(transparent)]
pub struct IAudioClock(Object<IAudioClockV>);

impl AudioInterface for IAudioClock {
    const IID: GUID = GUID::from_u128(0xcd63314f_3fba_4a1b_812c_ef96358728e7);
}

impl IAudioClock {
    #[inline]
    pub unsafe fn GetPosition(
        &self,
        position: *mut u64,
        qpc_position: Option<*mut u64>,
    ) -> ComResult<()> {
        let res = unsafe { (self.0.vtbl().get_position)(self.0 .0.cast(), position, qpc_position) };

        if res >= 0 {
            Ok(())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
struct IMMDeviceEnumeratorV {
    base: IUnknownV,
    enum_audio_endpoints: unsafe extern "system" fn(
        this: *mut c_void,
        dataflow: EDataFlow,
        dwstatemask: u32,
        ppdevices: *mut *mut c_void,
    ) -> HRESULT,
    get_default_audio_endpoint: unsafe extern "system" fn(
        this: *mut c_void,
        dataflow: EDataFlow,
        role: ERole,
        ppendpoint: *mut *mut c_void,
    ) -> HRESULT,
    __GetDevice: usize,
    __RegisterEndpointNotificationCallback: usize,
    __UnregisterEndpointNotificationCallback: usize,
}

const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xa95664d2_9614_4f35_a746_de8db63617e6);

#[repr(transparent)]
pub struct IMMDeviceEnumerator(Object<IMMDeviceEnumeratorV>);

impl IMMDeviceEnumerator {
    #[inline]
    pub unsafe fn new() -> ComResult<Self> {
        let mut iptr = std::mem::MaybeUninit::<Self>::uninit();
        let res = Com::CoCreateInstance(
            &windows_sys::Win32::Media::Audio::MMDeviceEnumerator,
            std::ptr::null_mut(),
            Com::CLSCTX_ALL,
            &IID_IMM_DEVICE_ENUMERATOR,
            iptr.as_mut_ptr().cast(),
        );

        if res >= 0 {
            Ok(iptr.assume_init())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn EnumAudioEndpoints(
        &self,
        data_flow: EDataFlow,
        mask: u32,
    ) -> ComResult<IMMDeviceCollection> {
        let mut out = std::mem::MaybeUninit::<IMMDeviceCollection>::uninit();
        let res = unsafe {
            (self.0.vtbl().enum_audio_endpoints)(
                self.0 .0.cast(),
                data_flow,
                mask,
                out.as_mut_ptr().cast(),
            )
        };

        if res >= 0 {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetDefaultAudioEndpoint(
        &self,
        data_flow: EDataFlow,
        role: ERole,
    ) -> ComResult<IMMDevice> {
        let mut out = std::mem::MaybeUninit::<IMMDevice>::uninit();
        let res = unsafe {
            (self.0.vtbl().get_default_audio_endpoint)(
                self.0 .0.cast(),
                data_flow,
                role,
                out.as_mut_ptr().cast(),
            )
        };

        if res >= 0 {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
pub struct PROPERTYKEY {
    pub fmtid: GUID,
    pub pid: u32,
}

#[repr(C)]
struct IPropertyStoreV {
    base: IUnknownV,
    __GetCount: usize,
    __GetAt: usize,
    get_value: unsafe extern "system" fn(
        this: *mut c_void,
        key: *const PROPERTYKEY,
        pv: *mut Com::StructuredStorage::PROPVARIANT,
    ) -> HRESULT,
    __SetValue: usize,
    __Commit: usize,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct IPropertyStore(Object<IPropertyStoreV>);

impl IPropertyStore {
    #[inline]
    pub unsafe fn GetValue(
        &self,
        key: *const PROPERTYKEY,
    ) -> ComResult<Com::StructuredStorage::PROPVARIANT> {
        let mut out = std::mem::MaybeUninit::uninit();
        let res = unsafe { (self.0.vtbl().get_value)(self.0 .0.cast(), key, out.as_mut_ptr()) };

        if res >= 0 {
            Ok(out.assume_init())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
struct IMMDeviceV {
    base: IUnknownV,
    activate: unsafe extern "system" fn(
        this: *mut c_void,
        iid: *const GUID,
        dwclsctx: Com::CLSCTX,
        pactivationparams: Option<*const Com::StructuredStorage::PROPVARIANT>,
        ppinterface: *mut *mut c_void,
    ) -> HRESULT,
    open_property_store: unsafe extern "system" fn(
        this: *mut c_void,
        stgmaccess: Com::STGM,
        ppproperties: *mut *mut c_void,
    ) -> HRESULT,
    get_id: unsafe extern "system" fn(
        this: *mut c_void,
        ppstrid: *mut windows_sys::core::PWSTR,
    ) -> HRESULT,
    __GetState: usize,
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct IMMDevice(Object<IMMDeviceV>);

impl IMMDevice {
    #[inline]
    pub unsafe fn Activate<A: AudioInterface>(
        &self,
        cls_ctx: Com::CLSCTX,
        params: Option<*const Com::StructuredStorage::PROPVARIANT>,
    ) -> ComResult<A> {
        let mut out = std::mem::MaybeUninit::<A>::uninit();
        let res = unsafe {
            (self.0.vtbl().activate)(
                self.0 .0.cast(),
                &A::IID,
                cls_ctx,
                params,
                out.as_mut_ptr().cast(),
            )
        };

        if res >= 0 {
            Ok(out.assume_init())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn GetId(&self) -> ComResult<windows_sys::core::PWSTR> {
        let mut out = std::mem::MaybeUninit::uninit();
        let res = unsafe { (self.0.vtbl().get_id)(self.0 .0.cast(), out.as_mut_ptr()) };

        if res >= 0 {
            Ok(out.assume_init())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn OpenPropertyStore(&self, access: Com::STGM) -> ComResult<IPropertyStore> {
        let mut out = std::mem::MaybeUninit::<IPropertyStore>::uninit();
        let res = unsafe {
            (self.0.vtbl().open_property_store)(self.0 .0.cast(), access, out.as_mut_ptr().cast())
        };

        if res >= 0 {
            Ok(out.assume_init())
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn cast<T: AudioInterface>(&self) -> ComResult<T> {
        super::query_interface(self.0.ptr().cast(), &T::IID)
    }
}

#[repr(C)]
struct IMMDeviceCollectionV {
    base: IUnknownV,
    get_count: unsafe extern "system" fn(this: *mut c_void, pcdevices: *mut u32) -> HRESULT,
    item: unsafe extern "system" fn(
        this: *mut c_void,
        ndevice: u32,
        ppdevice: *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IMMDeviceCollection(Object<IMMDeviceCollectionV>);

impl IMMDeviceCollection {
    #[inline]
    pub unsafe fn GetCount(&self) -> ComResult<u32> {
        let mut count = 0;
        let res = unsafe { (self.0.vtbl().get_count)(self.0 .0.cast(), &mut count) };

        if res >= 0 {
            Ok(count)
        } else {
            Err(res)
        }
    }

    #[inline]
    pub unsafe fn Item(&self, i: u32) -> ComResult<IMMDevice> {
        let mut out = std::mem::MaybeUninit::<IMMDevice>::uninit();
        let res = unsafe { (self.0.vtbl().item)(self.0 .0.cast(), i, out.as_mut_ptr().cast()) };

        if res >= 0 {
            Ok(out.assume_init())
        } else {
            Err(res)
        }
    }
}

#[repr(C)]
struct IMMEndpointV {
    base: IUnknownV,
    get_data_flow:
        unsafe extern "system" fn(this: *mut c_void, pdataflow: *mut EDataFlow) -> HRESULT,
}

#[repr(transparent)]
pub struct IMMEndpoint(Object<IMMEndpointV>);

impl AudioInterface for IMMEndpoint {
    const IID: GUID = GUID::from_u128(0x1be09788_6894_4089_8586_9a2a6c265ac5);
}

impl IMMEndpoint {
    #[inline]
    pub unsafe fn GetDataFlow(&self) -> ComResult<EDataFlow> {
        let mut out = -1;
        let res = unsafe { (self.0.vtbl().get_data_flow)(self.0 .0.cast(), &mut out) };

        if res >= 0 {
            Ok(out)
        } else {
            Err(res)
        }
    }
}
