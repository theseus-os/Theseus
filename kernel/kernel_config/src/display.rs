/// The maximum resolution `(width, height)` of the graphical framebuffer, in pixels.
/// This is a **requested limit** and does not control what the actual
/// resolution of the graphical framebuffer will be.
///
/// We recommend matching this to the value set in
/// `kernel/nano_core/src/asm/bios/multiboot_header.asm`,
/// but it's not strictly necessary to do so.
pub const FRAMEBUFFER_MAX_RESOLUTION: (u16, u16) = (1280, 1024);
