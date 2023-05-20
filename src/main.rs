use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};

use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::{Win32Surface, XcbSurface, XlibSurface};
use ash::vk::{
    self, make_version, ApplicationInfo, Bool32, DebugUtilsMessageSeverityFlagsEXT,
    DebugUtilsMessageTypeFlagsEXT, DebugUtilsMessengerCallbackDataEXT,
    DebugUtilsMessengerCreateInfoEXT, DebugUtilsMessengerEXT, DeviceQueueCreateInfo,
    InstanceCreateFlags, InstanceCreateInfo, SurfaceKHR, SwapchainKHR,
};
use ash::Device;
use ash::Entry;
use ash::Instance;
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
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
    device: Device,
}

impl App {
    unsafe fn create(window: &Window) -> Self {
        let mut data = AppData::default();

        let entry = Entry::load().unwrap();
        let instance = create_instance(window, &entry, &mut data);

        let surface = match (window.raw_display_handle(), window.raw_window_handle()) {
            (RawDisplayHandle::Xcb(display), RawWindowHandle::Xcb(window)) => {
                let info = vk::XcbSurfaceCreateInfoKHR::builder()
                    .window(window.window)
                    .connection(display.connection)
                    .build();

                XcbSurface::new(&entry, &instance)
                    .create_xcb_surface(&info, None)
                    .unwrap()
            }
            (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(window)) => {
                let info = vk::XlibSurfaceCreateInfoKHR::builder()
                    .window(window.window)
                    .dpy(display.display as *mut _)
                    .build();

                XlibSurface::new(&entry, &instance)
                    .create_xlib_surface(&info, None)
                    .unwrap()
            }
            (RawDisplayHandle::Windows(_), RawWindowHandle::Win32(window)) => {
                let info = vk::Win32SurfaceCreateInfoKHR::builder()
                    .hinstance(window.hinstance)
                    .hwnd(window.hwnd);

                Win32Surface::new(&entry, &instance)
                    .create_win32_surface(&info, None)
                    .unwrap()
            }
            _ => todo!(),
        };
        data.surface = surface;

        pick_physical_device(&entry, &instance, &mut data);

        let device = create_logical_device(&entry, &instance, &mut data);
        create_swapchain(&entry, window, &instance, &device, &mut data);

        Self {
            entry,
            instance,
            data,
            device,
        }
    }

    unsafe fn render(&mut self, window: &Window) {}

    unsafe fn destroy(&mut self) {
        ash::extensions::khr::Swapchain::new(&self.instance, &self.device)
            .destroy_swapchain(self.data.swapchain, None);

        self.device.destroy_device(None);

        let debug_utils = DebugUtils::new(&self.entry, &self.instance);
        debug_utils.destroy_debug_utils_messenger(self.data.messenger, None);

        ash::extensions::khr::Surface::new(&self.entry, &self.instance)
            .destroy_surface(self.data.surface, None);
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
    physical_device: vk::PhysicalDevice,
    graphics_queue: vk::Queue,
    surface: SurfaceKHR,
    present_queue: vk::Queue,
    swapchain: vk::SwapchainKHR,
}

unsafe fn pick_physical_device(entry: &Entry, instance: &Instance, data: &mut AppData) {
    for physical_device in instance.enumerate_physical_devices().unwrap() {
        let properties = instance.get_physical_device_properties(physical_device);

        let name = read_cstr(&properties.device_name);

        if !check_physical_device(entry, instance, data, physical_device) {
            tracing::warn!("physical device not suitable: {}", name.to_string_lossy());
        } else {
            tracing::info!("selected device: {}", name.to_string_lossy());

            data.physical_device = physical_device;

            return;
        }
    }

    panic!("no device selected");
}

unsafe fn check_physical_device(
    entry: &Entry,
    instance: &Instance,
    data: &AppData,
    physical_device: vk::PhysicalDevice,
) -> bool {
    let properties = instance.get_physical_device_properties(physical_device);
    if properties.device_type != vk::PhysicalDeviceType::DISCRETE_GPU {
        tracing::warn!("no DGPU");
        return false;
    }

    let features = instance.get_physical_device_features(physical_device);
    if features.geometry_shader != vk::TRUE {
        tracing::warn!("no geometry shader");
        return false;
    }

    if QueueFamilyIndices::get(entry, instance, data, physical_device).is_none() {
        tracing::warn!("missing queue families");
        return false;
    }

    if !check_physical_device_extensions(instance, physical_device) {
        return false;
    }

    let support = SwapchainSupport::get(entry, instance, data, physical_device);
    if support.formats.is_empty() || support.present_modes.is_empty() {
        tracing::warn!("no formats or present modes");
        return false;
    }

    true
}

struct QueueFamilyIndices {
    graphics: u32,
    present: u32,
}

impl QueueFamilyIndices {
    unsafe fn get(
        entry: &Entry,
        instance: &Instance,
        data: &AppData,
        physical_device: vk::PhysicalDevice,
    ) -> Option<Self> {
        let properties = instance.get_physical_device_queue_family_properties(physical_device);

        let graphics = properties
            .iter()
            .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|i| i as u32);

        let mut present = None;
        for (index, properties) in properties.iter().enumerate() {
            if ash::extensions::khr::Surface::new(&entry, &instance)
                .get_physical_device_surface_support(physical_device, index as u32, data.surface)
                .unwrap()
            {
                present = Some(index as u32);
                break;
            }
        }

        Some(Self {
            graphics: graphics?,
            present: present?,
        })
    }
}

fn read_cstr(buf: &[i8]) -> &CStr {
    let buf = bytemuck::cast_slice(buf);

    let mut null = None;
    for (i, b) in buf.iter().enumerate() {
        if *b == 0 {
            null = Some(i);
            break;
        }
    }

    let null = null.unwrap();
    CStr::from_bytes_with_nul(&buf[0..null + 1]).unwrap()
}

unsafe fn create_logical_device(entry: &Entry, instance: &Instance, data: &mut AppData) -> Device {
    let indices = QueueFamilyIndices::get(entry, instance, data, data.physical_device).unwrap();

    let mut unique_indices = HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = &[1.0];
    let queue_infos = unique_indices
        .iter()
        .map(|i| {
            vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(*i)
                .queue_priorities(queue_priorities)
                .build()
        })
        .collect::<Vec<_>>();

    let layers = vec![VALIDATION_LAYER.as_ptr()];

    let mut extensions = DEVICE_EXTENSIONS
        .iter()
        .map(|n| n.as_ptr())
        .collect::<Vec<_>>();

    let features = vk::PhysicalDeviceFeatures::builder();

    let info = vk::DeviceCreateInfo::builder()
        .queue_create_infos(&queue_infos)
        .enabled_layer_names(&layers)
        .enabled_extension_names(&extensions)
        .enabled_features(&features);

    let device = instance
        .create_device(data.physical_device, &info, None)
        .unwrap();

    data.graphics_queue = device.get_device_queue(indices.graphics, 0);
    data.present_queue = device.get_device_queue(indices.present, 0);

    device
}

const DEVICE_EXTENSIONS: &'static [&'static CStr] = &[&ash::extensions::khr::Swapchain::name()];

unsafe fn check_physical_device_extensions(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> bool {
    let extensions = instance
        .enumerate_device_extension_properties(physical_device)
        .unwrap()
        .iter()
        .map(|e| e.extension_name)
        .collect::<HashSet<_>>();

    if DEVICE_EXTENSIONS.iter().all(|e| {
        let mut ext = [0; 256];
        unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), ext.as_mut_ptr(), e.to_bytes().len()) };

        extensions.contains(&ext)
    }) {
        true
    } else {
        false
    }
}

struct SwapchainSupport {
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapchainSupport {
    unsafe fn get(
        entry: &Entry,
        instance: &Instance,
        data: &AppData,
        physical_device: vk::PhysicalDevice,
    ) -> Self {
        let ext = ash::extensions::khr::Surface::new(&entry, &instance);

        let capabilities = ext
            .get_physical_device_surface_capabilities(physical_device, data.surface)
            .unwrap();
        let formats = ext
            .get_physical_device_surface_formats(physical_device, data.surface)
            .unwrap();
        let present_modes = ext
            .get_physical_device_surface_present_modes(physical_device, data.surface)
            .unwrap();

        Self {
            capabilities,
            formats,
            present_modes,
        }
    }
}

fn get_swapchain_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    formats
        .iter()
        .cloned()
        .find(|f| {
            f.format == vk::Format::B8G8R8A8_SRGB
                && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .unwrap_or_else(|| formats[0])
}

fn get_swapchain_present_modes(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .cloned()
        .find(|m| *m == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

fn get_swapchain_extent(window: &Window, capabilities: vk::SurfaceCapabilitiesKHR) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        let size = window.inner_size();
        vk::Extent2D::builder()
            .width(u32::clamp(
                size.width,
                capabilities.min_image_extent.width,
                capabilities.max_image_extent.width,
            ))
            .height(u32::clamp(
                size.height,
                capabilities.min_image_extent.height,
                capabilities.max_image_extent.height,
            ))
            .build()
    }
}

unsafe fn create_swapchain(
    entry: &Entry,
    window: &Window,
    instance: &Instance,
    device: &Device,
    data: &mut AppData,
) {
    let indices = QueueFamilyIndices::get(entry, instance, data, data.physical_device).unwrap();
    let support = SwapchainSupport::get(entry, instance, data, data.physical_device);

    let surface_format = get_swapchain_surface_format(&support.formats);
    let present_modes = get_swapchain_present_modes(&support.present_modes);
    let extent = get_swapchain_extent(window, support.capabilities);

    let mut image_count = support.capabilities.min_image_count + 1;
    if support.capabilities.max_image_count != 0
        && image_count > support.capabilities.max_image_count
    {
        image_count = support.capabilities.max_image_count;
    }

    let mut queue_family_indices = vec![];
    let image_sharing_mode = if indices.graphics != indices.present {
        queue_family_indices.push(indices.graphics);
        queue_family_indices.push(indices.present);
        vk::SharingMode::CONCURRENT
    } else {
        vk::SharingMode::EXCLUSIVE
    };

    let info = vk::SwapchainCreateInfoKHR::builder()
        .surface(data.surface)
        .min_image_count(image_count)
        .image_format(surface_format.format)
        .image_color_space(surface_format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(image_sharing_mode)
        .queue_family_indices(&queue_family_indices)
        .pre_transform(support.capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_modes)
        .clipped(true)
        .old_swapchain(vk::SwapchainKHR::null());

    data.swapchain = ash::extensions::khr::Swapchain::new(instance, device)
        .create_swapchain(&info, None)
        .unwrap();
}
