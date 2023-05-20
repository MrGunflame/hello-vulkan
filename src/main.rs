use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};

use ash::extensions::ext::DebugUtils;
use ash::vk::{
    self, make_version, ApplicationInfo, Bool32, DebugUtilsMessageSeverityFlagsEXT,
    DebugUtilsMessageTypeFlagsEXT, DebugUtilsMessengerCallbackDataEXT,
    DebugUtilsMessengerCreateInfoEXT, DebugUtilsMessengerEXT, InstanceCreateFlags,
    InstanceCreateInfo,
};
use ash::Entry;
use ash::Instance;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowBuilder};

fn main() {
    pretty_env_logger::init();

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_inner_size(LogicalSize::new(800, 600))
        .build(&event_loop)
        .unwrap();

    let mut app = unsafe { App::create(&window) };
    let mut destroying = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            // Render a frame if our Vulkan app is not being destroyed.
            Event::MainEventsCleared if !destroying => unsafe { app.render(&window) },
            // Destroy our Vulkan app.
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                destroying = true;
                *control_flow = ControlFlow::Exit;

                unsafe {
                    app.destroy();
                }
            }
            _ => {}
        }
    });
}

struct App {
    entry: Entry,
    instance: Instance,
    data: AppData,
}

impl App {
    unsafe fn create(window: &Window) -> Self {
        let mut data = AppData::default();

        let entry = Entry::load().unwrap();
        let instance = create_instance(window, &entry, &mut data);

        Self {
            entry,
            instance,
            data,
        }
    }

    unsafe fn render(&mut self, window: &Window) {}

    unsafe fn destroy(&mut self) {
        let debug_utils = DebugUtils::new(&self.entry, &self.instance);
        debug_utils.destroy_debug_utils_messenger(self.data.messenger, None);

        self.instance.destroy_instance(None);
    }
}

unsafe fn create_instance(window: &Window, entry: &Entry, data: &mut AppData) -> Instance {
    let app_info = ApplicationInfo::builder()
        .application_name(CStr::from_bytes_with_nul(b"Hello Vulkan\0").unwrap())
        .application_version(make_version(0, 1, 0))
        .engine_name(CStr::from_bytes_with_nul(b"vk\0").unwrap())
        .engine_version(make_version(0, 1, 0))
        .api_version(make_version(1, 0, 0));

    let available_layers = entry
        .enumerate_instance_layer_properties()
        .unwrap()
        .iter()
        .map(|l| l.layer_name)
        .collect::<HashSet<_>>();

    let mut validation_layer = [0; 256];
    unsafe {
        std::ptr::copy_nonoverlapping(
            VALIDATION_LAYER.as_ptr(),
            validation_layer.as_mut_ptr(),
            VALIDATION_LAYER.to_bytes_with_nul().len(),
        )
    };

    if !available_layers.contains(&validation_layer) {
        panic!("validation layer not supported");
    }

    let layers = vec![VALIDATION_LAYER.as_ptr()];

    let extensions = get_required_instance_extensions(window)
        .iter()
        .map(|e| e.as_ptr())
        .collect::<Vec<_>>();

    let flags = InstanceCreateFlags::empty();

    let mut info = InstanceCreateInfo::builder()
        .application_info(&app_info)
        .enabled_extension_names(&extensions)
        .enabled_layer_names(&layers)
        .flags(flags);

    let mut debug_info = DebugUtilsMessengerCreateInfoEXT::builder()
        .message_severity(
            DebugUtilsMessageSeverityFlagsEXT::ERROR
                | DebugUtilsMessageSeverityFlagsEXT::WARNING
                | DebugUtilsMessageSeverityFlagsEXT::INFO
                | DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
        )
        .message_type(
            DebugUtilsMessageTypeFlagsEXT::DEVICE_ADDRESS_BINDING
                | DebugUtilsMessageTypeFlagsEXT::GENERAL
                | DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                | DebugUtilsMessageTypeFlagsEXT::VALIDATION,
        )
        .pfn_user_callback(Some(debug_callback));

    info = info.push_next(&mut debug_info);

    let instance = entry.create_instance(&info, None).unwrap();

    let debug_utils = DebugUtils::new(entry, &instance);
    data.messenger = debug_utils
        .create_debug_utils_messenger(&debug_info, None)
        .unwrap();

    instance
}

pub fn get_required_instance_extensions<'a>(
    window: &'a dyn HasRawWindowHandle,
) -> &'static [&'static CStr] {
    match window.raw_window_handle() {
        RawWindowHandle::Wayland(_) => WAYLAND,
        RawWindowHandle::Xcb(_) => XCB,
        RawWindowHandle::Xlib(_) => XLIB,
        _ => todo!(),
    }
}

const WAYLAND: &'static [&'static CStr] = &[
    ash::extensions::khr::Surface::name(),
    ash::extensions::khr::WaylandSurface::name(),
    DebugUtils::name(),
];

const XCB: &'static [&'static CStr] = &[
    ash::extensions::khr::Surface::name(),
    ash::extensions::khr::XcbSurface::name(),
    DebugUtils::name(),
];

const XLIB: &'static [&'static CStr] = &[
    ash::extensions::khr::Surface::name(),
    ash::extensions::khr::XlibSurface::name(),
    DebugUtils::name(),
];

const VALIDATION_LAYER: &'static CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(b"VK_LAYER_KHRONOS_validation\0") };

extern "system" fn debug_callback(
    severity: DebugUtilsMessageSeverityFlagsEXT,
    type_: DebugUtilsMessageTypeFlagsEXT,
    data: *const DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void,
) -> Bool32 {
    let data = unsafe { *data };
    let message = unsafe { CStr::from_ptr(data.p_message) }.to_string_lossy();

    if severity >= DebugUtilsMessageSeverityFlagsEXT::ERROR {
        tracing::error!("({:?}) {}", type_, message);
    } else if severity >= DebugUtilsMessageSeverityFlagsEXT::WARNING {
        tracing::warn!("({:?}) {}", type_, message);
    } else if severity >= DebugUtilsMessageSeverityFlagsEXT::INFO {
        tracing::debug!("({:?}) {}", type_, message);
    } else {
        tracing::trace!("({:?}) {}", type_, message);
    }

    vk::FALSE
}

#[derive(Default)]
struct AppData {
    messenger: DebugUtilsMessengerEXT,
}
