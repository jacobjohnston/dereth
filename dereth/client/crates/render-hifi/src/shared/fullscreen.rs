//! The full-screen triangle and the other shader code the passes share.

/// A triangle covering the target, with its texture coordinates.
pub const FULLSCREEN_WGSL: &str = include_str!("../../shaders/common/fullscreen.wgsl");
/// Colour-space conversions.
pub const COLOUR_WGSL: &str = include_str!("../../shaders/common/colour.wgsl");
/// Positions back from depth.
pub const DEPTH_WGSL: &str = include_str!("../../shaders/common/depth.wgsl");
/// Enlarging by the edges and sharpening by the contrast, for the upscalers.
pub const UPSCALE_WGSL: &str = include_str!("../../shaders/common/upscale.wgsl");
