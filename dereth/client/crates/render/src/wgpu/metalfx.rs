//! MetalFX's temporal upscaling, for the high-fidelity presentation's world.
//!
//! Each frame the scaler takes the picture drawn at fewer pixels, its depth and how far each pixel
//! moved, and rebuilds the picture at the frame's size from those and the frames before it. Only
//! Metal has it: anywhere else no scaler is made ([`TemporalScaler::new`]) and the presentation
//! rebuilds its picture with upscalers of its own.
//!
//! `wgpu` does not allow raw commands and its own in one encoder, so a scale is a submission of its
//! own ([`TemporalScaler::encode`]), and everything drawn before it must have been submitted.

/// What one frame's scale reads and writes.
#[derive(Debug, Clone, Copy)]
pub struct ScaleInputs<'a> {
    /// The picture as drawn, jittered, in [`COLOUR_FORMAT`].
    pub colour: &'a wgpu::Texture,
    /// Its depth, in `Depth32Float`, near small.
    pub depth: &'a wgpu::Texture,
    /// How far each pixel moved, in [`MOTION_FORMAT`], without the jitter.
    pub motion: &'a wgpu::Texture,
    /// How little each pixel should keep of the frames before it, in [`REACTIVE_FORMAT`].
    pub reactive: &'a wgpu::Texture,
    /// Where the picture goes, at the output size, in [`COLOUR_FORMAT`].
    pub output: &'a wgpu::Texture,
    /// The frame's jitter, in the drawn picture's pixels.
    pub jitter: [f32; 2],
    /// What turns a `motion` texel into pixels toward where the surface last stood.
    pub motion_scale: [f32; 2],
    /// Forget the frames before this one.
    pub reset: bool,
}

/// The picture's format, in and out.
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The motion texture's format.
pub const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The reactive mask's format.
pub const REACTIVE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

#[cfg(target_os = "macos")]
pub use metal::TemporalScaler;

#[cfg(target_os = "macos")]
mod metal {
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2::{sel, Message};
    use objc2_metal::{MTLPixelFormat, MTLTexture};
    use objc2_metal_fx::{
        MTLFXTemporalScaler, MTLFXTemporalScalerBase, MTLFXTemporalScalerDescriptor,
    };
    use wgpu::hal::api::Metal;

    use super::ScaleInputs;

    /// A MetalFX temporal scaler, for one pair of sizes.
    #[derive(Debug)]
    pub struct TemporalScaler {
        scaler: Retained<ProtocolObject<dyn MTLFXTemporalScaler>>,
        input: (u32, u32),
        reactive: bool,
    }

    /// The texture's Metal object, on a device that is Metal.
    fn raw(texture: &wgpu::Texture) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        // SAFETY: the object is only retained here, never destroyed; `wgpu` keeps owning it.
        let hal = unsafe { texture.as_hal::<Metal>() }?;
        Some(hal.raw_handle().retain())
    }

    impl TemporalScaler {
        /// A scaler from `input` to `output` pixels; `None` where the device is not Metal, or MetalFX
        /// cannot scale it by that much.
        #[must_use]
        pub fn new(device: &wgpu::Device, input: (u32, u32), output: (u32, u32)) -> Option<Self> {
            // SAFETY: the device is only read here, and outlives the call.
            let hal = unsafe { device.as_hal::<Metal>() }?;
            let raw = hal.raw_device();
            // SAFETY: these are plain property writes on an object made here, with values in the
            // ranges the header gives, and a device that is alive; the reactive mask's are asked of
            // the object first, as they are newer than the framework's first release.
            let (scaler, reactive) = unsafe {
                if !MTLFXTemporalScalerDescriptor::supportsDevice(raw) {
                    return None;
                }
                let d = MTLFXTemporalScalerDescriptor::new();
                d.setColorTextureFormat(MTLPixelFormat::RGBA16Float);
                d.setDepthTextureFormat(MTLPixelFormat::Depth32Float);
                d.setMotionTextureFormat(MTLPixelFormat::RG16Float);
                d.setOutputTextureFormat(MTLPixelFormat::RGBA16Float);
                d.setInputWidth(input.0 as usize);
                d.setInputHeight(input.1 as usize);
                d.setOutputWidth(output.0 as usize);
                d.setOutputHeight(output.1 as usize);
                // The picture is linear light, before the tone map's exposure.
                d.setAutoExposureEnabled(true);
                let reactive = d.respondsToSelector(sel!(setReactiveMaskTextureEnabled:));
                if reactive {
                    d.setReactiveMaskTextureEnabled(true);
                    d.setReactiveMaskTextureFormat(MTLPixelFormat::R8Unorm);
                }
                (d.newTemporalScalerWithDevice(raw)?, reactive)
            };
            // SAFETY: a plain property write on the scaler just made.
            unsafe { scaler.setDepthReversed(false) };
            Some(Self {
                scaler,
                input,
                reactive,
            })
        }

        /// Scale one frame, as a submission of its own after everything already submitted. False
        /// when a texture is not Metal's, and nothing was scaled.
        #[must_use]
        pub fn encode(
            &self,
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            inputs: &ScaleInputs<'_>,
        ) -> bool {
            let (Some(colour), Some(depth), Some(motion), Some(reactive), Some(output)) = (
                raw(inputs.colour),
                raw(inputs.depth),
                raw(inputs.motion),
                raw(inputs.reactive),
                raw(inputs.output),
            ) else {
                return false;
            };
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MetalFX upscale"),
            });
            // SAFETY: this encoder records nothing through `wgpu`, and the command buffer is neither
            // ended nor destroyed here. Each texture was written by a submission before this one and
            // is alive for the call; the scaler holds its own references to them after it. Only the
            // scaler encodes into the command buffer, which has no encoder open.
            let encoded = unsafe {
                encoder.as_hal_mut::<Metal, _, _>(|hal| {
                    let Some(buffer) = hal.and_then(|h| h.raw_command_buffer()) else {
                        return false;
                    };
                    let s = &self.scaler;
                    s.setColorTexture(Some(&colour));
                    s.setDepthTexture(Some(&depth));
                    s.setMotionTexture(Some(&motion));
                    if self.reactive {
                        s.setReactiveMaskTexture(Some(&reactive));
                    }
                    s.setOutputTexture(Some(&output));
                    s.setInputContentWidth(self.input.0 as usize);
                    s.setInputContentHeight(self.input.1 as usize);
                    s.setJitterOffsetX(inputs.jitter[0]);
                    s.setJitterOffsetY(inputs.jitter[1]);
                    s.setMotionVectorScaleX(inputs.motion_scale[0]);
                    s.setMotionVectorScaleY(inputs.motion_scale[1]);
                    s.setReset(inputs.reset);
                    autoreleasepool(|_| s.encodeToCommandBuffer(buffer));
                    true
                })
            };
            queue.submit([encoder.finish()]);
            encoded
        }
    }
}

/// A scaler no platform but macOS can make.
#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
pub struct TemporalScaler {}

#[cfg(not(target_os = "macos"))]
impl TemporalScaler {
    /// There is no MetalFX here.
    #[must_use]
    pub fn new(_device: &wgpu::Device, _input: (u32, u32), _output: (u32, u32)) -> Option<Self> {
        None
    }

    /// Nothing is scaled.
    #[must_use]
    pub fn encode(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _inputs: &ScaleInputs<'_>,
    ) -> bool {
        false
    }
}
