//! How a world drawn at fewer pixels than the frame is rebuilt at the frame's size, at a render
//! scale under 100%. The first of four that can run is chosen each frame ([`choose`]):
//!
//! - **MetalFX**, on macOS: Apple's temporal scaler rebuilds the picture from the frames before
//!   it, in place of the anti-aliasing's blend ([`crate::passes::taa`]).
//! - **Temporal**: the anti-aliasing's blend kept at the frame's size. Each frame's jittered
//!   pixels are laid into a history of the frame's size, followed there by the depth and the
//!   camera's last view, held to the colours around them, and dropped where the history saw
//!   something nearer than the surface now there (it was hidden); then sharpened by contrast.
//! - **Spatial**: the tone mapped picture enlarged along its edges and sharpened by contrast, in
//!   the composite ([`crate::composite`]), modelled on the EASU and RCAS of FidelityFX Super
//!   Resolution 1. It takes the frames the temporal cannot rebuild: the first of a history, any
//!   drawn without the anti-aliasing (under a diagnostic view: at a render scale it is otherwise
//!   always on), and a world that does not fill the frame.
//! - **Plain**: the picture filtered and sharpened, as the composite always enlarged it.
//!
//! The temporal and the spatial draw with fragment shaders alone, so they run wherever the
//! presentation does; MetalFX runs where the device is Metal and MetalFX can scale by as much.
//! No menu chooses among them: `DERETH_HIFI_UPSCALER` (`metalfx`, `temporal`, `spatial` or
//! `plain`) starts the choice at that one, to compare.

/// A way to rebuild the world's picture at the frame's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Upscaler {
    /// Apple's MetalFX temporal scaler.
    MetalFx,
    /// The anti-aliasing's blend into a history of the frame's size.
    Temporal,
    /// The tone mapped picture enlarged along its edges and sharpened.
    Spatial,
    /// The picture filtered and sharpened.
    Plain,
}

impl Upscaler {
    /// Every upscaler, the one chosen first first.
    pub const ORDER: [Self; 4] = [Self::MetalFx, Self::Temporal, Self::Spatial, Self::Plain];

    /// The upscaler a word names, as `DERETH_HIFI_UPSCALER` gives it.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "metalfx" => Some(Self::MetalFx),
            "temporal" => Some(Self::Temporal),
            "spatial" => Some(Self::Spatial),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }
}

/// What can rebuild a frame's world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Can {
    /// MetalFX scales from the drawn size to the frame's on this device, the anti-aliasing draws
    /// this frame (MetalFX reads its motion), and the world fills the frame.
    pub metalfx: bool,
    /// The anti-aliasing draws this frame (its jitter and motion), and the world fills the frame.
    pub temporal: bool,
}

/// The upscaler a frame's world is rebuilt with: the first from `first` on that `can` run. The
/// spatial and the plain always can.
#[must_use]
pub fn choose(first: Upscaler, can: Can) -> Upscaler {
    Upscaler::ORDER
        .into_iter()
        .skip_while(|u| *u != first)
        .find(|u| match u {
            Upscaler::MetalFx => can.metalfx,
            Upscaler::Temporal => can.temporal,
            Upscaler::Spatial | Upscaler::Plain => true,
        })
        .unwrap_or(Upscaler::Plain)
}

/// What rebuilds a frame `chosen` takes, holding a history (`history`) or not: the temporal's
/// first frame is the spatial's, and starts the history.
#[must_use]
pub fn shown_by(chosen: Upscaler, history: bool) -> Upscaler {
    if chosen == Upscaler::Temporal && !history {
        Upscaler::Spatial
    } else {
        chosen
    }
}

/// Where the choice starts: `DERETH_HIFI_UPSCALER`'s, or MetalFX's.
#[must_use]
pub fn first() -> Upscaler {
    first_of(std::env::var("DERETH_HIFI_UPSCALER").ok().as_deref())
}

/// Where the choice starts for a value of `DERETH_HIFI_UPSCALER`.
#[must_use]
pub fn first_of(word: Option<&str>) -> Upscaler {
    word.and_then(Upscaler::named).unwrap_or(Upscaler::MetalFx)
}

/// How much the spatial upscaler sharpens, stops under the most.
pub const SPATIAL_STOPS: f32 = 0.25;

/// How much the temporal upscaler sharpens its history, stops under the most.
pub const TEMPORAL_STOPS: f32 = 0.5;

/// How much nearer than the surface now seen the temporal's history must have stood where the
/// surface stood in the last frame for it to be dropped, a share of the surface's distance.
pub const HIDDEN: f32 = 0.1;

/// The sharpening's strength for `stops` under the most: 1 at none, halved at each.
#[must_use]
pub fn sharpness(stops: f32) -> f32 {
    dereth_primitives::num::math::powf(2.0, -stops.max(0.0))
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi render scale)
    use super::*;

    #[test]
    fn metalfx_is_chosen_where_it_runs_then_the_temporal_then_the_spatial() {
        let all = Can {
            metalfx: true,
            temporal: true,
        };
        assert_eq!(choose(Upscaler::MetalFx, all), Upscaler::MetalFx);
        let off_a_mac = Can {
            metalfx: false,
            ..all
        };
        assert_eq!(choose(Upscaler::MetalFx, off_a_mac), Upscaler::Temporal);
        // The anti-aliasing off, or a world short of the frame.
        assert_eq!(choose(Upscaler::MetalFx, Can::default()), Upscaler::Spatial);
    }

    #[test]
    fn asking_for_one_starts_the_choice_there_and_never_climbs_back() {
        let all = Can {
            metalfx: true,
            temporal: true,
        };
        assert_eq!(choose(Upscaler::Temporal, all), Upscaler::Temporal);
        assert_eq!(choose(Upscaler::Spatial, all), Upscaler::Spatial);
        assert_eq!(choose(Upscaler::Plain, all), Upscaler::Plain);
        assert_eq!(
            choose(Upscaler::Temporal, Can::default()),
            Upscaler::Spatial
        );
        assert_eq!(first_of(None), Upscaler::MetalFx);
        assert_eq!(first_of(Some(" Temporal ")), Upscaler::Temporal);
        assert_eq!(first_of(Some("spatial")), Upscaler::Spatial);
        assert_eq!(first_of(Some("plain")), Upscaler::Plain);
        assert_eq!(first_of(Some("metalfx")), Upscaler::MetalFx);
        assert_eq!(
            first_of(Some("fsr")),
            Upscaler::MetalFx,
            "unknown is automatic"
        );
    }

    #[test]
    fn the_temporal_s_first_frame_is_the_spatial_s() {
        assert_eq!(shown_by(Upscaler::Temporal, false), Upscaler::Spatial);
        assert_eq!(shown_by(Upscaler::Temporal, true), Upscaler::Temporal);
        for u in [Upscaler::MetalFx, Upscaler::Spatial, Upscaler::Plain] {
            assert_eq!(shown_by(u, false), u);
        }
    }

    #[test]
    fn the_spatial_and_temporal_upscalers_draw_with_fragment_shaders_alone() {
        for (name, source, entries) in [
            (
                "composite",
                crate::composite::composite_shader(),
                &["fs_easu", "fs_rcas"][..],
            ),
            (
                "anti-aliasing",
                crate::passes::taa::shader_source(),
                &["fs_upscale", "fs_upscale_sharpen"][..],
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&source)));
            for entry in entries {
                let e = module.entry_points.iter().find(|e| e.name == *entry);
                assert_eq!(
                    e.map(|e| e.stage),
                    Some(naga::ShaderStage::Fragment),
                    "{name}: {entry}"
                );
            }
            assert!(
                module
                    .entry_points
                    .iter()
                    .all(|e| e.stage != naga::ShaderStage::Compute),
                "{name} has no compute shader"
            );
            for (_, g) in module.global_variables.iter() {
                assert!(
                    !matches!(g.space, naga::AddressSpace::Storage { .. }),
                    "{name}: {:?} is a storage buffer",
                    g.name
                );
                assert!(
                    !matches!(
                        module.types[g.ty].inner,
                        naga::TypeInner::Image {
                            class: naga::ImageClass::Storage { .. },
                            ..
                        }
                    ),
                    "{name}: {:?} is a storage texture",
                    g.name
                );
            }
        }
    }

    #[test]
    fn the_sharpening_halves_by_the_stop_and_the_constants_are_in_range() {
        assert!((sharpness(0.0) - 1.0).abs() < 1e-6);
        assert!((sharpness(1.0) - 0.5).abs() < 1e-6);
        assert!((sharpness(-3.0) - 1.0).abs() < 1e-6, "never past the most");
        for stops in [SPATIAL_STOPS, TEMPORAL_STOPS] {
            assert!((0.0..=2.0).contains(&stops));
        }
        assert!(sharpness(TEMPORAL_STOPS) < sharpness(SPATIAL_STOPS));
        assert!((0.01..0.5).contains(&HIDDEN));
    }
}
