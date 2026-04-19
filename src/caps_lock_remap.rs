// macOS only. Remaps Caps Lock → F18 at the IOKit HID level so the OS delivers it as a normal
// KeyDown event instead of FlagsChanged, allowing ViMouse to intercept it like any other key.
// Restored on exit or panic.
use core_foundation::{
    array::CFArray,
    base::{CFRelease, CFType, CFTypeRef, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use std::os::raw::c_void;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};

pub use crate::input::caps_lock_used_in_config;

type IOHIDEventSystemClientRef = *mut c_void;

const HID_USAGE_PREFIX: i64 = 0x700000000;
const HID_USAGE_CAPS_LOCK: i64 = HID_USAGE_PREFIX | 0x39;
const HID_USAGE_F18: i64 = HID_USAGE_PREFIX | 0x6D;

pub const VKEY_F18: u16 = 0x4F;

pub static CAPS_LOCK_REMAP_ACTIVE: AtomicBool = AtomicBool::new(false);
static REMAPPER: OnceLock<Mutex<CapsLockRemapper>> = OnceLock::new();
static ERROR_LOGGED: AtomicBool = AtomicBool::new(false);

type IOReturn = i32;
type IOConnect = u32;
const KERN_SUCCESS: IOReturn = 0;
const IO_HID_CAPS_LOCK_STATE: i32 = 1;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreateSimpleClient(
        allocator: *const c_void,
    ) -> IOHIDEventSystemClientRef;
    fn IOHIDEventSystemClientSetProperty(
        client: IOHIDEventSystemClientRef,
        key: CFStringRef,
        property: CFTypeRef,
    ) -> bool;
    fn IOHIDEventSystemClientCopyProperty(
        client: IOHIDEventSystemClientRef,
        key: CFStringRef,
    ) -> CFTypeRef;
    fn IORegistryEntryFromPath(master_port: u32, path: *const i8) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, connect_type: u32, connect: *mut IOConnect) -> IOReturn;
    fn IOServiceClose(connect: IOConnect) -> IOReturn;
    fn IOObjectRelease(object: u32) -> IOReturn;
    fn IOHIDSetModifierLockState(handle: IOConnect, selector: i32, state: bool) -> IOReturn;
    fn mach_task_self() -> u32;
}

struct CapsLockRemapper {
    client: Option<IOHIDEventSystemClientRef>,
    original_mapping: Option<CFType>,
    enabled: bool,
}

unsafe impl Send for CapsLockRemapper {}

impl CapsLockRemapper {
    fn new() -> Self {
        let client = unsafe { IOHIDEventSystemClientCreateSimpleClient(ptr::null()) };
        Self {
            client: (!client.is_null()).then_some(client),
            original_mapping: None,
            enabled: false,
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if self.enabled == enabled {
            return Ok(());
        }

        let Some(client) = self.client else {
            CAPS_LOCK_REMAP_ACTIVE.store(false, Ordering::Release);
            return Err("failed to create macOS HID remap client".to_string());
        };

        if enabled {
            if self.original_mapping.is_none() {
                self.original_mapping = copy_user_key_mapping(client);
            }

            let remap = build_caps_lock_remap(self.original_mapping.as_ref());
            if !set_user_key_mapping(client, &remap.as_CFType()) {
                CAPS_LOCK_REMAP_ACTIVE.store(false, Ordering::Release);
                return Err("failed to enable macOS Caps Lock remap".to_string());
            }

            self.enabled = true;
            CAPS_LOCK_REMAP_ACTIVE.store(true, Ordering::Release);
            return Ok(());
        }

        let restore_value = self
            .original_mapping
            .clone()
            .unwrap_or_else(empty_key_mapping_property);
        if !set_user_key_mapping(client, &restore_value) {
            return Err("failed to restore macOS Caps Lock remap".to_string());
        }

        self.enabled = false;
        CAPS_LOCK_REMAP_ACTIVE.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for CapsLockRemapper {
    fn drop(&mut self) {
        let _ = self.set_enabled(false);

        if let Some(client) = self.client.take() {
            unsafe {
                CFRelease(client as CFTypeRef);
            }
        }
    }
}

fn copy_user_key_mapping(client: IOHIDEventSystemClientRef) -> Option<CFType> {
    let key = CFString::from_static_string("UserKeyMapping");
    let value = unsafe { IOHIDEventSystemClientCopyProperty(client, key.as_concrete_TypeRef()) };

    if value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_create_rule(value) })
    }
}

fn set_user_key_mapping(client: IOHIDEventSystemClientRef, property: &CFType) -> bool {
    let key = CFString::from_static_string("UserKeyMapping");
    unsafe {
        IOHIDEventSystemClientSetProperty(
            client,
            key.as_concrete_TypeRef(),
            property.as_CFTypeRef(),
        )
    }
}

fn build_caps_lock_remap(original_mapping: Option<&CFType>) -> CFArray<CFType> {
    let mut mappings = Vec::new();

    if let Some(original_mapping) = original_mapping {
        if let Some(array) = original_mapping.downcast::<CFArray>() {
            for item in &array {
                let mapping = unsafe { CFType::wrap_under_get_rule(*item as CFTypeRef) };
                if mapping_source_usage(&mapping) != Some(HID_USAGE_CAPS_LOCK) {
                    mappings.push(mapping);
                }
            }
        }
    }

    mappings.push(caps_lock_remap_entry());
    CFArray::from_CFTypes(&mappings)
}

fn caps_lock_remap_entry() -> CFType {
    let src_key = CFString::from_static_string("HIDKeyboardModifierMappingSrc");
    let dst_key = CFString::from_static_string("HIDKeyboardModifierMappingDst");
    let src = CFNumber::from(HID_USAGE_CAPS_LOCK);
    let dst = CFNumber::from(HID_USAGE_F18);
    CFDictionary::<CFString, CFType>::from_CFType_pairs(&[
        (src_key, src.as_CFType()),
        (dst_key, dst.as_CFType()),
    ])
    .into_CFType()
}

fn mapping_source_usage(mapping: &CFType) -> Option<i64> {
    let dictionary = mapping.downcast::<CFDictionary>()?;
    let dictionary = unsafe {
        CFDictionary::<CFString, CFType>::wrap_under_get_rule(dictionary.as_concrete_TypeRef())
    };
    let src_key = CFString::from_static_string("HIDKeyboardModifierMappingSrc");
    let source = dictionary.find(&src_key)?;
    source.downcast::<CFNumber>()?.to_i64()
}

fn empty_key_mapping_property() -> CFType {
    CFArray::<CFType>::from_CFTypes(&[]).into_CFType()
}

fn log_error(error: &str) {
    if !ERROR_LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("caps lock remap error: {error}");
    }
}

pub fn set_enabled(enabled: bool) {
    let remapper = REMAPPER.get_or_init(|| Mutex::new(CapsLockRemapper::new()));
    let mut remapper = remapper.lock().expect("caps lock remapper poisoned");
    if let Err(error) = remapper.set_enabled(enabled) {
        log_error(&error);
    }
}

pub fn shutdown() {
    set_enabled(false);
}

pub fn turn_off_caps_lock() {
    unsafe {
        // kIOHIDParamConnectType = 1, kIOMasterPortDefault = 0
        const IO_HID_PARAM_CONNECT_TYPE: u32 = 1;

        let path = b"IOService:/IOResources/IOHIDSystem\0";
        let service = IORegistryEntryFromPath(0, path.as_ptr() as *const i8);
        if service == 0 {
            return;
        }

        let mut connect: IOConnect = 0;
        let kr = IOServiceOpen(service, mach_task_self(), IO_HID_PARAM_CONNECT_TYPE, &mut connect);
        IOObjectRelease(service);
        if kr != KERN_SUCCESS {
            return;
        }

        IOHIDSetModifierLockState(connect, IO_HID_CAPS_LOCK_STATE, false);
        IOServiceClose(connect);
    }
}

