//! macOS SCSI pass-through through IOKit's SCSITask user client.
//!
//! The WL82 download-mode unit is a USB mass-storage device. macOS already
//! owns its storage interface, so this talks through the SCSI task user
//! client instead of detaching the kernel driver like a raw USB handle.

use super::{Direction, Transport, TransportError};
use core_foundation_sys::base::{
    kCFAllocatorDefault, CFAllocatorRef, CFGetTypeID, CFRelease, CFTypeRef,
};
use core_foundation_sys::dictionary::{
    kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreateMutable,
    CFDictionaryRef, CFDictionarySetValue,
};
use core_foundation_sys::number::{
    kCFNumberSInt32Type, CFNumberGetTypeID, CFNumberGetValue, CFNumberRef,
};
use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringRef};
use core_foundation_sys::uuid::{
    CFUUIDBytes, CFUUIDGetConstantUUIDWithBytes, CFUUIDGetUUIDBytes, CFUUIDRef,
};
use std::ffi::{c_char, c_void, CString};
use std::ptr::null_mut;

type IoObject = u32;
type IoIterator = u32;

const USB_VID: i64 = 0x4C4A;
const USB_PID: i64 = 0x8057;
const IO_SERVICE_PLANE: &[u8] = b"IOService\0";
const IO_REGISTRY_RECURSIVE: u32 = 0x1;
const IO_REGISTRY_PARENTS: u32 = 0x2;
const SCSI_STATUS_GOOD: u8 = 0;
const SCSI_DATA_NONE: u8 = 0;
const SCSI_DATA_OUT: u8 = 1;
const SCSI_DATA_IN: u8 = 2;
const SCSI_TIMEOUT_MS: u32 = 10_000;

const TASK_DEVICE_USER_CLIENT_TYPE: [u8; 16] = [
    0x7D, 0x66, 0x67, 0x8E, 0x08, 0xA2, 0x11, 0xD5, 0xA1, 0xB8, 0x00, 0x30, 0x65, 0x7D, 0x05, 0x2A,
];
const TASK_DEVICE_INTERFACE: [u8; 16] = [
    0x1B, 0xBC, 0x41, 0x32, 0x08, 0xA5, 0x11, 0xD5, 0x90, 0xED, 0x00, 0x30, 0x65, 0x7D, 0x05, 0x2A,
];
const CF_PLUGIN_INTERFACE: [u8; 16] = [
    0xC2, 0x44, 0xE8, 0x58, 0x10, 0x9C, 0x11, 0xD4, 0x91, 0xD4, 0x00, 0x50, 0xE4, 0xC6, 0x42, 0x6F,
];

#[repr(C)]
struct IoGuts {
    reserved: *mut c_void,
    query_interface: unsafe extern "C" fn(*mut c_void, CFUUIDBytes, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(*mut c_void) -> u64,
    release: unsafe extern "C" fn(*mut c_void) -> u64,
}

#[repr(C)]
struct IoCfgPlugInInterface {
    guts: IoGuts,
}

#[allow(dead_code)]
#[repr(C)]
struct ScsiTaskDeviceInterface {
    guts: IoGuts,
    version: u16,
    revision: u16,
    is_exclusive_access_available: unsafe extern "C" fn(*mut c_void) -> u8,
    add_callback_dispatcher_to_run_loop: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32,
    remove_callback_dispatcher_from_run_loop: unsafe extern "C" fn(*mut c_void),
    obtain_exclusive_access: unsafe extern "C" fn(*mut c_void) -> i32,
    release_exclusive_access: unsafe extern "C" fn(*mut c_void) -> i32,
    create_scsi_task: unsafe extern "C" fn(*mut c_void) -> *mut *mut ScsiTaskInterface,
}

#[allow(dead_code)]
#[repr(C)]
struct ScsiTaskInterface {
    guts: IoGuts,
    version: u16,
    revision: u16,
    is_task_active: unsafe extern "C" fn(*mut c_void) -> u8,
    set_task_attribute: unsafe extern "C" fn(*mut c_void, u8) -> i32,
    get_task_attribute: unsafe extern "C" fn(*mut c_void, *mut u8) -> i32,
    set_command_descriptor_block: unsafe extern "C" fn(*mut c_void, *mut u8, u8) -> i32,
    get_command_descriptor_block_size: unsafe extern "C" fn(*mut c_void) -> u8,
    get_command_descriptor_block: unsafe extern "C" fn(*mut c_void, *mut u8) -> i32,
    set_scatter_gather_entries:
        unsafe extern "C" fn(*mut c_void, *mut IoVirtualRange, u8, u64, u8) -> i32,
    set_timeout_duration: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    get_timeout_duration: unsafe extern "C" fn(*mut c_void) -> u32,
    set_task_completion_callback:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
    execute_task_async: unsafe extern "C" fn(*mut c_void) -> i32,
    execute_task_sync: unsafe extern "C" fn(*mut c_void, *mut u8, *mut u8, *mut u64) -> i32,
}

#[repr(C)]
struct IoVirtualRange {
    address: u64,
    length: u64,
}

// These pin the COM layouts before any pointer is transmitted to IOKit.
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<IoGuts>() == 32);
    assert!(offset_of!(ScsiTaskDeviceInterface, obtain_exclusive_access) == 64);
    assert!(offset_of!(ScsiTaskDeviceInterface, create_scsi_task) == 80);
    assert!(offset_of!(ScsiTaskInterface, set_scatter_gather_entries) == 88);
    assert!(offset_of!(ScsiTaskInterface, execute_task_sync) == 128);
    assert!(size_of::<IoVirtualRange>() == 16);
};

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceGetMatchingServices(
        master_port: u32,
        matching: CFDictionaryRef,
        existing: *mut IoIterator,
    ) -> i32;
    fn IOIteratorNext(iterator: IoIterator) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: IoObject, id: *mut u64) -> i32;
    fn IORegistryEntrySearchCFProperty(
        entry: IoObject,
        plane: *const c_char,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    fn IOCreatePlugInInterfaceForService(
        service: IoObject,
        plugin_type: CFUUIDRef,
        interface_type: CFUUIDRef,
        plugin: *mut *mut *mut IoCfgPlugInInterface,
        score: *mut i32,
    ) -> i32;
    fn IODestroyPlugInInterface(plugin: *mut *mut IoCfgPlugInInterface) -> i32;
}

fn cf_string(value: &str) -> CFStringRef {
    let value = CString::new(value).expect("CF string without NUL");
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, value.as_ptr(), kCFStringEncodingUTF8) }
}

fn uuid(bytes: [u8; 16]) -> CFUUIDRef {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            kCFAllocatorDefault,
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }
}

fn matching_iterator() -> Option<IoIterator> {
    let match_key = cf_string("IOPropertyMatch");
    let category_key = cf_string("SCSITaskDeviceCategory");
    let category_value = cf_string("SCSITaskUserClientDevice");
    let sub = unsafe {
        CFDictionaryCreateMutable(
            kCFAllocatorDefault,
            0,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    let matching = unsafe {
        CFDictionaryCreateMutable(
            kCFAllocatorDefault,
            0,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    if sub.is_null() || matching.is_null() {
        unsafe {
            if !sub.is_null() {
                CFRelease(sub as CFTypeRef);
            }
            if !matching.is_null() {
                CFRelease(matching as CFTypeRef);
            }
            CFRelease(match_key as CFTypeRef);
            CFRelease(category_key as CFTypeRef);
            CFRelease(category_value as CFTypeRef);
        }
        return None;
    }

    unsafe {
        CFDictionarySetValue(
            sub,
            category_key as *const c_void,
            category_value as *const c_void,
        );
        CFDictionarySetValue(matching, match_key as *const c_void, sub as *const c_void);
        CFRelease(sub as CFTypeRef);
        CFRelease(category_key as CFTypeRef);
        CFRelease(category_value as CFTypeRef);
        CFRelease(match_key as CFTypeRef);

        let mut iterator = 0;
        let result = IOServiceGetMatchingServices(0, matching as CFDictionaryRef, &mut iterator);
        if result == 0 && iterator != 0 {
            Some(iterator)
        } else {
            None
        }
    }
}

fn registry_id(service: IoObject) -> Option<u64> {
    let mut id = 0;
    (unsafe { IORegistryEntryGetRegistryEntryID(service, &mut id) } == 0).then_some(id)
}

fn usb_number(service: IoObject, key: &str) -> Option<i64> {
    let key = cf_string(key);
    let value = unsafe {
        IORegistryEntrySearchCFProperty(
            service,
            IO_SERVICE_PLANE.as_ptr() as *const c_char,
            key,
            kCFAllocatorDefault,
            IO_REGISTRY_RECURSIVE | IO_REGISTRY_PARENTS,
        )
    };
    unsafe { CFRelease(key as CFTypeRef) };
    if value.is_null() {
        return None;
    }
    let is_number = unsafe { CFGetTypeID(value) == CFNumberGetTypeID() };
    if !is_number {
        unsafe { CFRelease(value) };
        return None;
    }
    let mut number = 0i32;
    let ok = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberSInt32Type,
            &mut number as *mut i32 as *mut c_void,
        )
    };
    unsafe { CFRelease(value) };
    ok.then_some(number as i64)
}

fn candidate(service: IoObject) -> Option<(String, String)> {
    if usb_number(service, "idVendor")? != USB_VID || usb_number(service, "idProduct")? != USB_PID {
        return None;
    }
    let id = registry_id(service)?;
    let path = format!("scsi:{id:016x}");
    let label = format!("WL82 UBOOT1.00 ({path})");
    Some((path, label))
}

pub fn find_devices() -> Vec<(String, String)> {
    let Some(iterator) = matching_iterator() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    loop {
        let service = unsafe { IOIteratorNext(iterator) };
        if service == 0 {
            break;
        }
        if let Some(item) = candidate(service) {
            found.push(item);
        }
        unsafe { IOObjectRelease(service) };
    }
    unsafe { IOObjectRelease(iterator) };
    found
}

fn find_service(path: &str) -> Option<IoObject> {
    let id = u64::from_str_radix(path.strip_prefix("scsi:")?, 16).ok()?;
    let iterator = matching_iterator()?;
    loop {
        let service = unsafe { IOIteratorNext(iterator) };
        if service == 0 {
            break;
        }
        if registry_id(service) == Some(id) {
            unsafe { IOObjectRelease(iterator) };
            return Some(service);
        }
        unsafe { IOObjectRelease(service) };
    }
    unsafe { IOObjectRelease(iterator) };
    None
}

pub struct Device {
    service: IoObject,
    plugin: *mut *mut IoCfgPlugInInterface,
    device: *mut *mut ScsiTaskDeviceInterface,
    task: *mut *mut ScsiTaskInterface,
}

unsafe fn release_task(task: *mut *mut ScsiTaskInterface) {
    if task.is_null() {
        return;
    }
    let interface = *task;
    if !interface.is_null() {
        let _ = ((*interface).guts.release)(task as *mut c_void);
    }
}

unsafe fn release_device(device: *mut *mut ScsiTaskDeviceInterface, exclusive: bool) {
    if device.is_null() {
        return;
    }
    if exclusive {
        let _ = ((**device).release_exclusive_access)(device as *mut c_void);
    }
    let _ = ((**device).guts.release)(device as *mut c_void);
}

unsafe fn release_plugin(plugin: *mut *mut IoCfgPlugInInterface) {
    if !plugin.is_null() {
        IODestroyPlugInInterface(plugin);
    }
}

impl Device {
    pub fn open(path: &str) -> Result<Self, TransportError> {
        let service = find_service(path).ok_or_else(|| {
            TransportError::Failed(format!("{path}: download-mode unit not found"))
        })?;

        let mut plugin: *mut *mut IoCfgPlugInInterface = null_mut();
        let mut score = 0;
        let result = unsafe {
            IOCreatePlugInInterfaceForService(
                service,
                uuid(TASK_DEVICE_USER_CLIENT_TYPE),
                uuid(CF_PLUGIN_INTERFACE),
                &mut plugin,
                &mut score,
            )
        };
        if result != 0 || plugin.is_null() || unsafe { (*plugin).is_null() } {
            unsafe {
                if !plugin.is_null() {
                    release_plugin(plugin);
                }
                IOObjectRelease(service);
            }
            return Err(TransportError::Permission(format!(
                "macOS could not open the SCSI task client (IOKit {result:#x}); run with sudo"
            )));
        }

        let mut raw_device: *mut c_void = null_mut();
        let hr = unsafe {
            ((**plugin).guts.query_interface)(
                plugin as *mut c_void,
                CFUUIDGetUUIDBytes(uuid(TASK_DEVICE_INTERFACE)),
                &mut raw_device,
            )
        };
        if hr != 0 || raw_device.is_null() {
            unsafe {
                release_plugin(plugin);
                IOObjectRelease(service);
            }
            return Err(TransportError::Failed(format!(
                "macOS could not query the SCSI task device (HRESULT {hr:#x})"
            )));
        }
        let device = raw_device as *mut *mut ScsiTaskDeviceInterface;

        let exclusive = unsafe { ((**device).obtain_exclusive_access)(device as *mut c_void) };
        if exclusive != 0 {
            unsafe {
                release_device(device, false);
                release_plugin(plugin);
                IOObjectRelease(service);
            }
            return Err(TransportError::Permission(format!(
                "macOS denied exclusive access to the unit (IOKit {exclusive:#x}); unmount it and run with sudo"
            )));
        }

        let task = unsafe { ((**device).create_scsi_task)(device as *mut c_void) };
        if task.is_null() {
            unsafe {
                release_device(device, true);
                release_plugin(plugin);
                IOObjectRelease(service);
            }
            return Err(TransportError::Failed(
                "macOS could not create a SCSI task".into(),
            ));
        }

        Ok(Self {
            service,
            plugin,
            device,
            task,
        })
    }

    fn command_inner(
        &mut self,
        cdb: &[u8],
        mut data: Direction<'_>,
    ) -> Result<usize, TransportError> {
        if cdb.len() > 16 {
            return Err(TransportError::Failed("CDB longer than 16 bytes".into()));
        }
        let task = unsafe { *self.task };
        if task.is_null() {
            return Err(TransportError::Failed("SCSI task is closed".into()));
        }
        let task_handle = self.task as *mut c_void;

        let (address, length, direction) = match &mut data {
            Direction::In(bytes) => (bytes.as_mut_ptr() as u64, bytes.len(), SCSI_DATA_IN),
            Direction::Out(bytes) => (bytes.as_ptr() as u64, bytes.len(), SCSI_DATA_OUT),
        };
        let mut range = IoVirtualRange {
            address,
            length: length as u64,
        };
        let range_ptr: *mut IoVirtualRange = if length == 0 {
            null_mut()
        } else {
            &mut range as *mut IoVirtualRange
        };
        let entries = u8::from(length != 0);

        let status = unsafe {
            ((*task).set_scatter_gather_entries)(
                task_handle,
                range_ptr,
                entries,
                length as u64,
                if length == 0 {
                    SCSI_DATA_NONE
                } else {
                    direction
                },
            )
        };
        if status != 0 {
            return Err(TransportError::Failed(format!(
                "macOS could not set the SCSI data buffer (IOKit {status:#x})"
            )));
        }

        let status = unsafe {
            ((*task).set_command_descriptor_block)(
                task_handle,
                cdb.as_ptr() as *mut u8,
                cdb.len() as u8,
            )
        };
        if status != 0 {
            return Err(TransportError::Failed(format!(
                "macOS could not set the SCSI command (IOKit {status:#x})"
            )));
        }

        let status = unsafe { ((*task).set_timeout_duration)(task_handle, SCSI_TIMEOUT_MS) };
        if status != 0 {
            return Err(TransportError::Failed(format!(
                "macOS could not set the SCSI timeout (IOKit {status:#x})"
            )));
        }

        let mut sense = [0u8; 18];
        let mut task_status = 0u8;
        let mut moved = 0u64;
        let result = unsafe {
            ((*task).execute_task_sync)(
                task_handle,
                sense.as_mut_ptr(),
                &mut task_status,
                &mut moved,
            )
        };
        if result != 0 {
            return Err(TransportError::Failed(format!(
                "macOS SCSI command failed (IOKit {result:#x})"
            )));
        }
        if task_status != SCSI_STATUS_GOOD {
            return Err(TransportError::Failed(format!(
                "macOS SCSI command failed (status {task_status:#04x})"
            )));
        }
        if moved > length as u64 {
            return Err(TransportError::Failed(
                "macOS reported more transferred bytes than requested".into(),
            ));
        }
        Ok(moved as usize)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            release_task(self.task);
            if !self.device.is_null() {
                let _ = ((**self.device).release_exclusive_access)(self.device as *mut c_void);
                let _ = ((**self.device).guts.release)(self.device as *mut c_void);
            }
            release_plugin(self.plugin);
            IOObjectRelease(self.service);
        }
    }
}

impl Transport for Device {
    fn command(&mut self, cdb: &[u8], data: Direction<'_>) -> Result<usize, TransportError> {
        self.command_inner(cdb, data)
    }
}
